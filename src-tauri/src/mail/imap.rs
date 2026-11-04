//! io-imap adapter (D-009): the real [`MailTransport`] over
//! `ImapClientStd`'s blocking command surface.
//!
//! Every method maps 1:1 onto an io-imap command; error mapping collapses
//! the crate's per-command error enums into the three transport classes the
//! sync engine distinguishes: IO → `Disconnected`, any SASL/LOGIN failure →
//! `Auth`, everything else → `Protocol`.
//!
//! Two documented Phase-2 simplifications (decisions.md):
//! - `idle_once` is a NOOP poll returning `Quiet`; spec §4.4 sanctions the
//!   no-IDLE polling fallback and the engine re-syncs each cycle regardless
//!   of the event. The eventful `watch_mailbox` stream is Phase-3 work.
//! - `thrid` stays `None`: X-GM-THRID is a Gmail-only FETCH item io-imap
//!   does not model; JWZ threading handles Gmail mail correctly without it.

use io_imap::client::{ImapClient, ImapClientError, ImapClientStd, ImapClientStdConnectOptions};
use io_imap::rfc3501::append::ImapMessageAppendOptions;
use io_imap::rfc3501::fetch::ImapMessageFetchOptions;
use io_imap::rfc3501::search::ImapMessageSearchOptions;
use io_imap::rfc3501::select::ImapMailboxSelectOptions;
use io_imap::rfc3501::store::ImapMessageStoreOptions;
use io_imap::types::{
    core::{AString, Atom, Vec1},
    fetch::{MacroOrMessageDataItemNames, MessageDataItem, MessageDataItemName, Section},
    flag::{Flag, FlagFetch, StoreType},
    mailbox::{ListMailbox, Mailbox},
    search::SearchKey,
};
use io_sasl::{mechanism::Sasl, rfc4616::plain::SaslPlainCreds, xoauth2::SaslXoauth2Creds};
use secrecy::SecretString;
use url::Url;

use super::headers::parse_header_block;
use super::transport::{
    BodyData, FetchedMeta, FolderInfo, IdleEvent, MailTransport, SelectData, SpecialRole,
    TransportError, TransportResult, UidRange,
};

/// Authentication for [`ImapTransport::connect`]. Secrets are carried as
/// `SecretString` end-to-end; io-sasl exposes them only when framing the
/// SASL exchange (spec §5: secrets never in logs or config).
pub enum ImapAuth {
    /// App-password login (Gmail/iCloud/Zoho/Yahoo presets, spec §5).
    Plain {
        user: String,
        password: SecretString,
    },
    /// Outlook personal accounts (XOAUTH2, spec §5).
    Xoauth2 { user: String, token: SecretString },
}

pub struct ImapTransport {
    client: ImapClientStd,
}

impl ImapTransport {
    /// Connects with implicit TLS (`imaps://host`), performs the handshake
    /// and SASL authentication, and returns the transport plus the
    /// capabilities the server advertised.
    pub fn connect(url: &Url, auth: ImapAuth) -> Result<(Self, Vec<String>), TransportError> {
        let sasl = match auth {
            ImapAuth::Plain { user, password } => Sasl::Plain(SaslPlainCreds {
                authzid: None,
                authcid: user,
                passwd: password,
            }),
            ImapAuth::Xoauth2 { user, token } => Sasl::Xoauth2(SaslXoauth2Creds {
                username: user,
                token,
            }),
        };
        let opts = ImapClientStdConnectOptions {
            sasl: Some(sasl),
            ..Default::default()
        };
        let (client, caps) = ImapClientStd::connect(url, opts)?;
        Ok((
            Self { client },
            caps.iter().map(|c| c.to_string()).collect(),
        ))
    }
}

/// io-imap reports each command's failure through its own error enum behind
/// a `From` variant of [`ImapClientError`]; only the IO and auth classes
/// carry sync-relevant meaning, everything else is a protocol error.
impl From<ImapClientError> for TransportError {
    fn from(err: ImapClientError) -> Self {
        match err {
            ImapClientError::Io(e) => TransportError::Disconnected(e.to_string()),
            // Every SASL/LOGIN failure is an auth failure regardless of
            // mechanism; each variant wraps its own error type.
            ImapClientError::Login(e) => TransportError::Auth(e.to_string()),
            ImapClientError::AuthLogin(e) => TransportError::Auth(e.to_string()),
            ImapClientError::AuthPlain(e) => TransportError::Auth(e.to_string()),
            ImapClientError::AuthAnonymous(e) => TransportError::Auth(e.to_string()),
            ImapClientError::AuthOAuthBearer(e) => TransportError::Auth(e.to_string()),
            ImapClientError::AuthXOAuth2(e) => TransportError::Auth(e.to_string()),
            other => TransportError::Protocol(other.to_string()),
        }
    }
}

fn mailbox_str(mailbox: &Mailbox<'_>) -> String {
    match mailbox {
        Mailbox::Inbox => "INBOX".to_string(),
        Mailbox::Other(other) => String::from_utf8_lossy(other.inner().as_ref()).into_owned(),
    }
}

fn mailbox_owned(name: &str) -> TransportResult<Mailbox<'static>> {
    Mailbox::try_from(name.to_string())
        .map_err(|e| TransportError::Protocol(format!("invalid mailbox {name:?}: {e}")))
}

fn sequence_set(range: UidRange) -> TransportResult<String> {
    match range {
        UidRange::All => Ok("1:*".to_string()),
        UidRange::Interval { from, to } => Ok(format!("{from}:{to}")),
    }
}

fn sequence_set_of_uids(uids: &[u32]) -> TransportResult<String> {
    // Sort so the set stays canonical ("1,5,9" not "9,1,5"); servers accept
    // either but sorted sets read better in logs.
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    Ok(sorted
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(","))
}

fn flags_of(items: Vec<FlagFetch<'static>>) -> Vec<String> {
    items
        .into_iter()
        .map(|f| match f {
            FlagFetch::Flag(flag) => flag.to_string(),
            FlagFetch::Recent => "\\Recent".to_string(),
        })
        .collect()
}

/// The header field names the backfill needs — exactly spec §4.3's list
/// (RFC 5322 core, threading chain, list unsubscribe). Requesting them
/// explicitly keeps the FETCH small. X-GM-THRID is absent: Gmail provides
/// it as a FETCH item, not a header, and io-imap does not model that item.
const HEADER_FIELDS: [&str; 11] = [
    "FROM",
    "TO",
    "CC",
    "SUBJECT",
    "DATE",
    "MESSAGE-ID",
    "IN-REPLY-TO",
    "REFERENCES",
    "REPLY-TO",
    "LIST-UNSUBSCRIBE",
    "LIST-UNSUBSCRIBE-POST",
];

fn header_section() -> Section<'static> {
    let names = HEADER_FIELDS
        .iter()
        .map(|f| AString::try_from(*f).expect("header field name is a valid astring"))
        .collect::<Vec<_>>();
    Section::HeaderFields(
        None,
        Vec1::try_from(names).expect("header field list is never empty"),
    )
}

fn fetch_options() -> ImapMessageFetchOptions {
    ImapMessageFetchOptions {
        uid: true,
        modifiers: vec![],
    }
}

impl MailTransport for ImapTransport {
    fn capabilities(&mut self) -> TransportResult<Vec<String>> {
        Ok(self
            .client
            .capability()?
            .iter()
            .map(|c| c.to_string())
            .collect())
    }

    fn list_folders(&mut self) -> TransportResult<Vec<FolderInfo>> {
        let reference = mailbox_owned("")?;
        let pattern =
            ListMailbox::try_from("*").map_err(|e| TransportError::Protocol(e.to_string()))?;
        let rows = self.client.list(reference, pattern)?;
        let subscribed: Vec<String> = self
            .client
            .lsub(
                mailbox_owned("")?,
                ListMailbox::try_from("*").map_err(|e| TransportError::Protocol(e.to_string()))?,
            )?
            .into_iter()
            .map(|(mb, _, _)| mailbox_str(&mb))
            .collect();

        Ok(rows
            .into_iter()
            .map(|(mailbox, _delimiter, attrs)| {
                // SPECIAL-USE attributes arrive as extension atoms ("\Sent",
                // "\Trash", …); Display renders them back with the backslash.
                let role = attrs
                    .iter()
                    .find_map(|attr| match attr.to_string().as_str() {
                        "\\Sent" => Some(SpecialRole::Sent),
                        "\\Drafts" => Some(SpecialRole::Drafts),
                        "\\Trash" => Some(SpecialRole::Trash),
                        "\\Junk" => Some(SpecialRole::Junk),
                        "\\All" => Some(SpecialRole::All),
                        "\\Archive" => Some(SpecialRole::Archive),
                        _ => None,
                    });
                let name = mailbox_str(&mailbox);
                FolderInfo {
                    subscribed: subscribed.iter().any(|s| s == &name),
                    name,
                    role,
                }
            })
            .collect())
    }

    fn select(&mut self, mailbox: &str) -> TransportResult<SelectData> {
        let data = self.client.select(
            mailbox_owned(mailbox)?,
            ImapMailboxSelectOptions { parameters: vec![] },
        )?;
        Ok(SelectData {
            uidvalidity: data.uid_validity.map(|n| n.get()).unwrap_or(0),
            uidnext: data.uid_next.map(|n| n.get()).unwrap_or(0),
            highestmodseq: data.highest_mod_seq.map(|s| s.to_string()),
        })
    }

    fn uid_fetch_headers(&mut self, range: UidRange) -> TransportResult<Vec<FetchedMeta>> {
        let set = sequence_set(range)?;
        let items = MacroOrMessageDataItemNames::MessageDataItemNames(vec![
            MessageDataItemName::Flags,
            MessageDataItemName::InternalDate,
            MessageDataItemName::Rfc822Size,
            MessageDataItemName::BodyExt {
                section: Some(header_section()),
                partial: None,
                peek: true,
            },
        ]);
        let map = self.client.fetch(
            set.as_str()
                .try_into()
                .map_err(|e| TransportError::Protocol(format!("bad sequence set {set:?}: {e}")))?,
            items,
            fetch_options(),
        )?;

        Ok(map
            .into_iter()
            .map(|(seq, items)| {
                let mut meta = FetchedMeta {
                    uid: seq.get(),
                    ..Default::default()
                };
                for item in items {
                    match item {
                        MessageDataItem::Flags(flags) => meta.flags = flags_of(flags),
                        MessageDataItem::InternalDate(date) => {
                            meta.internaldate_ms = date.as_ref().timestamp_millis();
                        }
                        MessageDataItem::Rfc822Size(size) => meta.size = u64::from(size),
                        MessageDataItem::BodyExt { data, .. } => {
                            if let Some(bytes) = data.into_option() {
                                meta.headers = parse_header_block(&String::from_utf8_lossy(&bytes));
                            }
                        }
                        _ => {}
                    }
                }
                meta
            })
            .collect())
    }

    fn uid_search_all(&mut self) -> TransportResult<Vec<u32>> {
        let keys = Vec1::try_from(vec![SearchKey::All]).expect("one search key is never empty");
        Ok(self
            .client
            .search(keys, ImapMessageSearchOptions { uid: true })?
            .into_iter()
            .map(|uid| uid.get())
            .collect())
    }

    fn fetch_flags(&mut self, range: UidRange) -> TransportResult<Vec<(u32, Vec<String>)>> {
        let set = sequence_set(range)?;
        let items =
            MacroOrMessageDataItemNames::MessageDataItemNames(vec![MessageDataItemName::Flags]);
        let map = self.client.fetch(
            set.as_str()
                .try_into()
                .map_err(|e| TransportError::Protocol(format!("bad sequence set {set:?}: {e}")))?,
            items,
            fetch_options(),
        )?;
        Ok(map
            .into_iter()
            .map(|(seq, items)| {
                let flags = items
                    .into_iter()
                    .find_map(|item| match item {
                        MessageDataItem::Flags(flags) => Some(flags_of(flags)),
                        _ => None,
                    })
                    .unwrap_or_default();
                (seq.get(), flags)
            })
            .collect())
    }

    fn uid_store_add(&mut self, uids: &[u32], flags: &[String]) -> TransportResult<()> {
        self.store_flags(uids, flags, StoreType::Add)
    }

    fn uid_store_remove(&mut self, uids: &[u32], flags: &[String]) -> TransportResult<()> {
        self.store_flags(uids, flags, StoreType::Remove)
    }

    fn fetch_body(&mut self, uid: u32) -> TransportResult<Option<BodyData>> {
        let set = format!("{uid}:*");
        let items =
            MacroOrMessageDataItemNames::MessageDataItemNames(vec![MessageDataItemName::BodyExt {
                section: None,
                partial: None,
                peek: true,
            }]);
        let map = self.client.fetch(
            set.as_str()
                .try_into()
                .map_err(|e| TransportError::Protocol(format!("bad sequence set {set:?}: {e}")))?,
            items,
            fetch_options(),
        )?;
        let Some((_, items)) = map.into_iter().next() else {
            return Ok(None);
        };
        let raw = items.into_iter().find_map(|item| match item {
            MessageDataItem::BodyExt { data, .. } => data.into_option(),
            _ => None,
        });
        let Some(raw) = raw else {
            // The server answered OK without a body (NIL literal): an empty
            // body is data, not an error.
            return Ok(Some(BodyData::default()));
        };

        // MIME extraction happens here, in the transport layer, so the store
        // keeps raw text/html exactly as spec §3 defines; sanitization is a
        // render-time concern (Phase 3).
        let parsed = mail_parser::MessageParser::default().parse(&raw);
        let (text, html) = match parsed {
            Some(message) => (
                message.body_text(0).map(|c| c.into_owned()),
                message.body_html(0).map(|c| c.into_owned()),
            ),
            None => (None, None),
        };
        Ok(Some(BodyData { text, html }))
    }

    fn idle_once(&mut self, timeout_secs: u64) -> TransportResult<IdleEvent> {
        self.client.noop()?;
        std::thread::sleep(std::time::Duration::from_secs(timeout_secs));
        Ok(IdleEvent::Quiet)
    }

    fn append(&mut self, mailbox: &str, message: &[u8], flags: &[String]) -> TransportResult<u32> {
        let flags = parse_flag_list(flags)?;
        let opts = ImapMessageAppendOptions {
            flags,
            date: None,
            non_sync: false,
        };
        // APPENDUID is (uid_validity, uid) for a single message; servers
        // without UIDPLUS return None and the caller sees 0.
        Ok(self
            .client
            .append(mailbox_owned(mailbox)?, message, opts)?
            .1
            .map_or(0, |(_, uid)| uid))
    }
}

impl ImapTransport {
    fn store_flags(
        &mut self,
        uids: &[u32],
        flags: &[String],
        kind: StoreType,
    ) -> TransportResult<()> {
        if uids.is_empty() || flags.is_empty() {
            return Ok(());
        }
        let set = sequence_set_of_uids(uids)?;
        let flags = parse_flag_list(flags)?;
        let _echo = self.client.store(
            set.as_str()
                .try_into()
                .map_err(|e| TransportError::Protocol(format!("bad sequence set {set:?}: {e}")))?,
            kind,
            flags,
            ImapMessageStoreOptions { uid: true },
        )?;
        Ok(())
    }
}

/// `Flag<'static>` from owned strings: known system flags map to their
/// variants via `Flag::system`, `\…` others become `Extension`, bare words
/// become `Keyword` — exactly how `Flag::try_from(&str)` resolves, but on
/// owned data so the result outlives the input slice.
fn parse_flag_list(flags: &[String]) -> TransportResult<Vec<Flag<'static>>> {
    flags
        .iter()
        .map(|f| {
            let stripped = f.strip_prefix('\\').unwrap_or(f);
            let atom = Atom::try_from(stripped.to_owned())
                .map_err(|e| TransportError::Protocol(format!("invalid flag {f:?}: {e}")))?;
            Ok(if f.starts_with('\\') {
                Flag::system(atom)
            } else {
                Flag::keyword(atom)
            })
        })
        .collect()
}
