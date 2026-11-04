//! Scripted IMAP mock (spec §13 L3, D-018): an in-memory mailbox whose state
//! tests mutate between sync calls (new mail, flag changes, expunges,
//! UIDVALIDITY bumps), plus fault injection for disconnect/reconnect paths.

use std::collections::BTreeMap;

use super::transport::{
    BodyData, FetchedMeta, FolderInfo, IdleEvent, MailTransport, SelectData, TransportError,
    TransportResult, UidRange,
};

#[derive(Debug, Clone)]
pub struct MockMessage {
    pub uid: u32,
    pub flags: Vec<String>,
    pub headers: BTreeMap<String, String>,
    pub thrid: Option<String>,
    pub body: BodyData,
    pub internaldate_ms: i64,
    pub size: u64,
}

/// In-memory server state; tests own it directly and mutate between calls.
#[derive(Debug, Default, Clone)]
pub struct MockMailbox {
    pub name: String,
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub highestmodseq: Option<String>,
    pub messages: BTreeMap<u32, MockMessage>,
    pub idle_events: Vec<IdleEvent>,
}

impl MockMailbox {
    /// Adds a message and returns its UID (UIDNEXT semantics). UIDs are
    /// 1-based ascending per RFC 3501, like a real server.
    pub fn push(&mut self, msg: MockMessage) -> u32 {
        let uid = self.uidnext.max(1);
        self.uidnext = uid + 1;
        self.messages.insert(uid, msg);
        uid
    }

    /// Test-side flag mutation (server-side state change for reconcile tests).
    pub fn set_flags(&mut self, uid: u32, flags: Vec<String>) {
        if let Some(m) = self.messages.get_mut(&uid) {
            m.flags = flags;
        }
    }

    pub fn expunge(&mut self, uid: u32) {
        self.messages.remove(&uid);
    }
}

#[derive(Default)]
pub struct MockTransport {
    pub caps: Vec<String>,
    pub folders: Vec<FolderInfo>,
    /// All mailboxes; index 0 is also what `select` resolves unknown names to
    /// in single-folder tests. `selected` tracks SELECT state like a server.
    pub mailboxes: Vec<MockMailbox>,
    pub selected: Option<String>,
    /// One-shot failures: each is consumed by the next transport call,
    /// modeling a disconnect followed by a transparent reconnect.
    pub fail_next: Vec<TransportError>,
    /// Counts transport calls; tests inject a mid-backfill failure with this.
    pub calls: usize,
    pub fail_after_calls: Option<usize>,
    /// UID the APPEND path assigns next (UIDPLUS).
    pub next_append_uid: u32,
}

impl MockTransport {
    pub fn new(mailbox: MockMailbox) -> Self {
        MockTransport {
            caps: vec!["IMAP4REV1".into(), "IDLE".into(), "UIDPLUS".into()],
            mailboxes: vec![mailbox],
            ..Default::default()
        }
    }

    /// The mailbox a command operates on: the selected one, falling back to
    /// the first mailbox for scripts that never call select.
    fn active(&mut self, mailbox: &str) -> TransportResult<&mut MockMailbox> {
        let name = match self.selected.clone() {
            Some(n) => n,
            None => mailbox.to_string(),
        };
        let idx = self
            .mailboxes
            .iter()
            .position(|m| m.name == name)
            .unwrap_or(0);
        self.mailboxes
            .get_mut(idx)
            .ok_or_else(|| TransportError::Protocol("mock has no mailboxes".into()))
    }

    /// Test-side mutation handle on the selected (or first) mailbox.
    pub fn active_mailbox(&mut self) -> &mut MockMailbox {
        let idx = self
            .mailboxes
            .iter()
            .position(|m| Some(&m.name) == self.selected.as_ref())
            .unwrap_or(0);
        &mut self.mailboxes[idx]
    }

    fn tick(&mut self) -> TransportResult<()> {
        self.calls += 1;
        if !self.fail_next.is_empty() {
            return Err(self.fail_next.remove(0));
        }
        if let Some(limit) = self.fail_after_calls {
            if self.calls > limit {
                return Err(TransportError::Disconnected("injected mid-sync".into()));
            }
        }
        Ok(())
    }
}

fn uids_in_range(mb: &MockMailbox, range: UidRange) -> Vec<u32> {
    match range {
        UidRange::All => mb.messages.keys().copied().collect(),
        UidRange::Interval { from, to } => mb.messages.range(from..=to).map(|(k, _)| *k).collect(),
    }
}

impl MailTransport for MockTransport {
    fn capabilities(&mut self) -> TransportResult<Vec<String>> {
        self.tick()?;
        Ok(self.caps.clone())
    }

    fn list_folders(&mut self) -> TransportResult<Vec<FolderInfo>> {
        self.tick()?;
        Ok(self.folders.clone())
    }

    fn select(&mut self, mailbox: &str) -> TransportResult<SelectData> {
        self.tick()?;
        let mb = self
            .mailboxes
            .iter_mut()
            .find(|m| m.name == mailbox)
            .ok_or_else(|| TransportError::Protocol(format!("no such mailbox: {mailbox}")))?;
        self.selected = Some(mailbox.to_string());
        Ok(SelectData {
            uidvalidity: mb.uidvalidity,
            uidnext: mb.uidnext,
            highestmodseq: mb.highestmodseq.clone(),
        })
    }

    fn uid_fetch_headers(&mut self, range: UidRange) -> TransportResult<Vec<FetchedMeta>> {
        self.tick()?;
        let mb = self.active("")?;
        Ok(uids_in_range(mb, range)
            .into_iter()
            .map(|uid| {
                let m = &mb.messages[&uid];
                FetchedMeta {
                    uid,
                    flags: m.flags.clone(),
                    internaldate_ms: m.internaldate_ms,
                    size: m.size,
                    headers: m.headers.clone(),
                    thrid: m.thrid.clone(),
                }
            })
            .collect())
    }

    fn uid_search_all(&mut self) -> TransportResult<Vec<u32>> {
        self.tick()?;
        let mb = self.active("")?;
        Ok(mb.messages.keys().copied().collect())
    }

    fn fetch_flags(&mut self, range: UidRange) -> TransportResult<Vec<(u32, Vec<String>)>> {
        self.tick()?;
        let mb = self.active("")?;
        Ok(uids_in_range(mb, range)
            .into_iter()
            .map(|uid| (uid, mb.messages[&uid].flags.clone()))
            .collect())
    }

    fn uid_store_add(&mut self, uids: &[u32], flags: &[String]) -> TransportResult<()> {
        self.tick()?;
        let mb = self.active("")?;
        for uid in uids {
            if let Some(m) = mb.messages.get_mut(uid) {
                m.flags.extend(flags.iter().cloned());
            }
        }
        Ok(())
    }

    fn uid_store_remove(&mut self, uids: &[u32], flags: &[String]) -> TransportResult<()> {
        self.tick()?;
        let mb = self.active("")?;
        for uid in uids {
            if let Some(m) = mb.messages.get_mut(uid) {
                m.flags.retain(|f| !flags.contains(f));
            }
        }
        Ok(())
    }

    fn fetch_body(&mut self, uid: u32) -> TransportResult<Option<BodyData>> {
        self.tick()?;
        let mb = self.active("")?;
        Ok(mb.messages.get(&uid).map(|m| m.body.clone()))
    }

    fn idle_once(&mut self, _timeout_secs: u64) -> TransportResult<IdleEvent> {
        self.tick()?;
        let mb = self.active("")?;
        if mb.idle_events.is_empty() {
            Ok(IdleEvent::Quiet)
        } else {
            Ok(mb.idle_events.remove(0))
        }
    }

    fn append(
        &mut self,
        _mailbox: &str,
        _message: &[u8],
        _flags: &[String],
    ) -> TransportResult<u32> {
        self.tick()?;
        self.next_append_uid += 1;
        Ok(self.next_append_uid)
    }
}
