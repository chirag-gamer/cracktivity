//! SMTP submission + Sent APPEND (FR-18, spec §4.6): send via the account's
//! submission endpoint, then append a copy (marked `\Seen`) to the provider's
//! Sent folder and mirror it into the local sent folder so follow-ups (FR-13)
//! and threads see it immediately. Failures surface with the reason — the
//! draft stays retryable in the outbox (FR-18 AC).

use lettre::message::Mailbox;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::store::dao;
use crate::store::{Store, StoreError};

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("draft not found")]
    NotFound,
    #[error("account has no SMTP endpoint configured")]
    NoSmtpEndpoint,
    #[error("{0}")]
    BadAddress(String),
    #[error("sending failed: {0}")]
    Smtp(String),
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("transport: {0}")]
    Transport(#[from] crate::mail::transport::TransportError),
    #[error(transparent)]
    Action(#[from] super::actions::ActionError),
}

impl From<lettre::error::Error> for SendError {
    fn from(e: lettre::error::Error) -> Self {
        SendError::Smtp(e.to_string())
    }
}

impl From<lettre::transport::smtp::Error> for SendError {
    fn from(e: lettre::transport::smtp::Error) -> Self {
        SendError::Smtp(e.to_string())
    }
}

impl From<rusqlite::Error> for SendError {
    fn from(e: rusqlite::Error) -> Self {
        SendError::Store(StoreError::from(e))
    }
}

/// Everything needed to submit one message.
#[derive(Debug, Clone)]
pub struct SubmitParams {
    pub from_email: String,
    pub to: Vec<Addr>,
    pub cc: Vec<Addr>,
    pub subject: String,
    pub body_text: String,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
}

use crate::store::dao::Addr;

/// Composes the RFC 5322 message; the same bytes go to SMTP and to the Sent
/// APPEND so the provider's copy matches what was sent byte-for-byte.
pub fn compose(params: &SubmitParams) -> Result<Vec<u8>, SendError> {
    let from: Mailbox = params
        .from_email
        .parse()
        .map_err(|_| SendError::BadAddress(params.from_email.clone()))?;
    let mut builder = Message::builder().from(from).subject(&params.subject);
    for to in &params.to {
        builder = builder.to(mailbox_of(&to.email)?);
    }
    for cc in &params.cc {
        builder = builder.cc(mailbox_of(&cc.email)?);
    }
    if let Some(irt) = &params.in_reply_to {
        builder = builder.in_reply_to(irt.clone());
    }
    if let Some(references) = &params.references {
        builder = builder.references(references.clone());
    }
    // CRLF-terminated plain text body (RFC 5322 line discipline).
    let body = params.body_text.replace("\r\n", "\n").replace('\n', "\r\n");
    Ok(builder.body(body)?.formatted())
}

fn mailbox_of(email: &str) -> Result<Mailbox, SendError> {
    email
        .parse()
        .map_err(|_| SendError::BadAddress(email.to_string()))
}

/// SMTP submission. Port 465 = implicit TLS wrapper; 587 = STARTTLS
/// (spec §4.6 matrix).
pub fn submit(
    host: &str,
    port: u16,
    user: &str,
    password: &str,
    from: &str,
    recipients: &[&str],
    message: &[u8],
) -> Result<(), SendError> {
    let from_mailbox: Mailbox = from
        .parse()
        .map_err(|_| SendError::BadAddress(from.into()))?;
    let rcpts = recipients
        .iter()
        .map(|r| {
            r.parse::<Mailbox>()
                .map_err(|_| SendError::BadAddress((*r).into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    crate::platform::netlog::record(host);
    let envelope = lettre::address::Envelope::new(
        Some(from_mailbox.email),
        rcpts.into_iter().map(|m| m.email).collect(),
    )
    .map_err(|e| SendError::Smtp(e.to_string()))?;
    let transport = if port == 465 {
        SmtpTransport::relay(host)?
    } else {
        SmtpTransport::starttls_relay(host)?
    }
    .port(port)
    .credentials(Credentials::new(user.to_string(), password.to_string()))
    .build();
    transport.send_raw(&envelope, message)?;
    Ok(())
}

/// The full send path for one draft: compose → submit → Sent APPEND → local
/// sent row → contact note → draft removed (FR-17 AC: sending a draft
/// removes it from Drafts).
pub fn send_draft(
    t: &mut dyn crate::mail::transport::MailTransport,
    store: &Store,
    draft_id: i64,
    password: &str,
) -> Result<(), SendError> {
    let conn = store.conn();
    let draft = dao::draft_by_id(conn, draft_id)?.ok_or(SendError::NotFound)?;
    let account_id = draft.account_id;
    let from_email: String = conn.query_row(
        "SELECT email FROM accounts WHERE id = ?1",
        [account_id],
        |r| r.get(0),
    )?;
    let (smtp_host, smtp_port) = smtp_endpoint_of(conn, account_id)?;

    // Threading headers follow the replied-to message.
    let (in_reply_to, references) = match &draft.in_reply_to {
        Some(mid) => {
            let refs: Option<String> = conn
                .query_row(
                    "SELECT references_raw FROM messages WHERE message_id = ?1 LIMIT 1",
                    [mid],
                    |r| r.get(0),
                )
                .ok();
            (Some(mid.clone()), refs.map(|r| format!("{r} {mid}")))
        }
        None => (None, None),
    };

    let params = SubmitParams {
        from_email: from_email.clone(),
        to: draft.to.clone(),
        cc: draft.cc.clone(),
        subject: draft.subject.clone().unwrap_or_default(),
        body_text: draft.body_text.clone().unwrap_or_default(),
        in_reply_to: in_reply_to.clone(),
        references,
    };
    let raw = compose(&params)?;
    let recipients: Vec<&str> = draft
        .to
        .iter()
        .chain(draft.cc.iter())
        .map(|a| a.email.as_str())
        .collect();
    submit(
        &smtp_host,
        smtp_port,
        &from_email,
        password,
        &from_email,
        &recipients,
        &raw,
    )?;

    record_sent(t, store, account_id, draft_id, &params, in_reply_to, &raw)?;
    dao::note_sent(conn, &draft.to)?;
    dao::delete_draft(conn, draft_id)?;
    Ok(())
}

/// Post-submission bookkeeping (FR-18 Sent APPEND + FR-13 local mirror):
/// appends a `\Seen` copy to the account's Sent folder and mirrors the row
/// into the local sent folder so the follow-up view sees it immediately.
pub fn record_sent(
    t: &mut dyn crate::mail::transport::MailTransport,
    store: &Store,
    account_id: i64,
    draft_id: i64,
    params: &SubmitParams,
    in_reply_to: Option<String>,
    raw: &[u8],
) -> Result<(), SendError> {
    let conn = store.conn();
    let folders = t.list_folders()?;
    let sent_name = folders
        .iter()
        .find(|f| f.role == Some(crate::mail::transport::SpecialRole::Sent))
        .map(|f| f.name.clone());
    let new_uid = match &sent_name {
        Some(name) => t.append(name, raw, &["\\Seen".to_string()])?,
        None => 0,
    };

    if let Some(name) = sent_name {
        if let Some(folder) = dao::folder_state_by_name(conn, account_id, &name)? {
            let now = dao::now_ms();
            let thrid = format!("sent-{draft_id}-{now}");
            dao::insert_messages(
                conn,
                account_id,
                folder.id,
                &[dao::NewMessage {
                    uid: if new_uid > 0 { new_uid } else { folder.uidnext },
                    message_id: Some(format!("<sent-{draft_id}-{now}@{}>", params.from_email)),
                    in_reply_to,
                    subject: params.subject.clone().into(),
                    from: Some(vec![Addr {
                        name: None,
                        email: params.from_email.clone(),
                    }]),
                    to: Some(params.to.clone()),
                    cc: (!params.cc.is_empty()).then(|| params.cc.clone()),
                    date_sent: Some(now),
                    date_recv: Some(now),
                    flags: vec!["\\Seen".to_string()],
                    thrid: Some(thrid),
                    ..Default::default()
                }],
            )?;
        }
    }
    Ok(())
}

/// Which submission endpoint an account uses: preset constant, else the
/// stored manual value (V4 columns).
pub fn smtp_endpoint_of(
    conn: &rusqlite::Connection,
    account_id: i64,
) -> Result<(String, u16), SendError> {
    let (provider, stored_host, stored_port): (String, Option<String>, Option<u16>) = conn
        .query_row(
            "SELECT provider, smtp_host, smtp_port FROM accounts WHERE id = ?1",
            [account_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(StoreError::from)?;
    if let Some(preset) = crate::platform::providers::ALL
        .iter()
        .find(|p| p.id == provider)
    {
        if !preset.smtp_host.is_empty() {
            return Ok((preset.smtp_host.into(), preset.smtp_port));
        }
    }
    stored_host
        .filter(|h| !h.trim().is_empty())
        .map(|h| (h, stored_port.unwrap_or(587)))
        .ok_or(SendError::NoSmtpEndpoint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::mock::{MockMailbox, MockTransport};
    use crate::mail::transport::{FolderInfo, SpecialRole};
    use crate::store::dao::{Addr, NewAccount};

    /// The composed message carries the addressing + threading headers and
    /// CRLF line endings (RFC 5322 discipline).
    #[test]
    fn compose_produces_rfc5322_message() {
        let raw = compose(&SubmitParams {
            from_email: "me@example.com".into(),
            to: vec![Addr {
                name: None,
                email: "them@example.org".into(),
            }],
            cc: vec![Addr {
                name: None,
                email: "other@example.net".into(),
            }],
            subject: "Re: lunch".into(),
            body_text: "line one\nline two\n".into(),
            in_reply_to: Some("<orig@example.org>".into()),
            references: Some("<a@x> <orig@example.org>".into()),
        })
        .unwrap();
        let text = String::from_utf8_lossy(&raw);
        assert!(text.contains("From: me@example.com"), "{text}");
        assert!(text.contains("To: them@example.org"));
        assert!(text.contains("Cc: other@example.net"));
        assert!(text.contains("Subject: Re: lunch"));
        assert!(text.contains("In-Reply-To: <orig@example.org>"));
        assert!(text.contains("References: <a@x> <orig@example.org>"));
        assert!(text.contains("Date: "));
        assert!(text.contains("line one\r\nline two"), "CRLF bodies: {text}");
    }

    /// The post-send bookkeeping over the mock: a `\Seen` APPEND lands in the
    /// Sent folder and a mirrored row appears in the local sent folder (the
    /// input FR-13 needs). SMTP submit itself is L4/GreenMail territory.
    #[test]
    fn record_sent_appends_and_mirrors() {
        let dir = tempfile::tempdir().unwrap();
        let target = "cracktivity.test.smtp.record";
        let store = Store::open(dir.path(), target).unwrap();
        let account = dao::add_account(
            store.conn(),
            &NewAccount {
                provider: "gmail",
                email: "me@example.com",
                display_name: None,
                auth_type: "app_password",
                credential_ref: target,
                imap_host: None,
                imap_port: None,
                smtp_host: None,
                smtp_port: None,
            },
        )
        .unwrap();
        let mut transport = MockTransport::new(MockMailbox {
            name: "Sent".into(),
            uidvalidity: 1,
            uidnext: 1,
            ..Default::default()
        });
        // The local mirror row lands in the store's Sent folder row, which a
        // real deployment has (sent sync); create it for the fixture.
        dao::upsert_folder(
            store.conn(),
            account,
            "Sent",
            Some("sent"),
            &crate::store::dao::FolderUpsert {
                uidvalidity: 1,
                uidnext: 1,
                highestmodseq: None,
            },
        )
        .unwrap()
        .unwrap();
        transport.folders = vec![
            FolderInfo {
                name: "INBOX".into(),
                role: Some(SpecialRole::Inbox),
                subscribed: true,
            },
            FolderInfo {
                name: "Sent".into(),
                role: Some(SpecialRole::Sent),
                subscribed: true,
            },
        ];
        let params = SubmitParams {
            from_email: "me@example.com".into(),
            to: vec![Addr {
                name: None,
                email: "them@example.org".into(),
            }],
            cc: vec![],
            subject: "Ping".into(),
            body_text: "body".into(),
            in_reply_to: None,
            references: None,
        };
        let raw = compose(&params).unwrap();
        record_sent(&mut transport, &store, account, 42, &params, None, &raw).unwrap();

        assert_eq!(
            transport.active_mailbox().messages.len(),
            1,
            "APPEND landed"
        );
        assert!(transport
            .active_mailbox()
            .messages
            .values()
            .next()
            .unwrap()
            .flags
            .contains(&"\\Seen".into()));
        let row: (String, String, i64) = store
            .conn()
            .query_row(
                "SELECT m.thrid, m.subject, m.unread FROM messages m
                 JOIN folders f ON f.id = m.folder_id AND f.role = 'sent'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(row.1, "Ping");
        assert_eq!(row.2, 0, "sent mail is read");
        assert!(row.0.starts_with("sent-42-"));

        // Endpoint resolution: preset constant for gmail.
        assert_eq!(
            smtp_endpoint_of(store.conn(), account).unwrap(),
            ("smtp.gmail.com".into(), 465)
        );
        crate::platform::keyring::init();
        let _ = crate::platform::keyring::delete_secret(target);
    }
}
