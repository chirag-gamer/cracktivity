//! `MailTransport` trait (D-009): the sync engine never touches IMAP crate
//! specifics — real traffic goes through an io-imap adapter, tests through a
//! scripted mock (spec §13 L3).

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialRole {
    Inbox,
    Sent,
    Drafts,
    Trash,
    All,
    Archive,
    Junk,
}

impl SpecialRole {
    pub fn as_str(self) -> &'static str {
        match self {
            SpecialRole::Inbox => "inbox",
            SpecialRole::Sent => "sent",
            SpecialRole::Drafts => "drafts",
            SpecialRole::Trash => "trash",
            SpecialRole::All => "all",
            SpecialRole::Archive => "archive",
            SpecialRole::Junk => "junk",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FolderInfo {
    pub name: String,
    pub role: Option<SpecialRole>,
    pub subscribed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectData {
    pub uidvalidity: u32,
    pub uidnext: u32,
    pub highestmodseq: Option<String>,
}

/// Inclusive UID interval (`from..=to`). `All` maps to `UID SEARCH ALL`/1:*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UidRange {
    All,
    Interval { from: u32, to: u32 },
}

/// Header-only FETCH result (spec §4.3 header-only backfill; always PEEK).
#[derive(Debug, Clone, Default)]
pub struct FetchedMeta {
    pub uid: u32,
    pub flags: Vec<String>,
    /// INTERNALDATE as epoch milliseconds.
    pub internaldate_ms: i64,
    pub size: u64,
    /// Unfolded headers, lower-cased field names; last occurrence wins except
    /// for list fields which join with ", ".
    pub headers: BTreeMap<String, String>,
    /// X-GM-THRID when the server provides one (D-010 short-circuit).
    pub thrid: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct BodyData {
    pub text: Option<String>,
    pub html: Option<String>,
}

/// One IDLE wait's worth of server activity (spec §4.4 steady state).
#[derive(Debug, Clone, PartialEq)]
pub enum IdleEvent {
    Quiet,
    Exists(Vec<u32>),
    FlagsChanged(Vec<u32>),
    Expunged(Vec<u32>),
}

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("connection lost: {0}")]
    Disconnected(String),
    #[error("authentication failed: {0}")]
    Auth(String),
    #[error("protocol error: {0}")]
    Protocol(String),
}

pub type TransportResult<T> = Result<T, TransportError>;

pub trait MailTransport {
    fn capabilities(&mut self) -> TransportResult<Vec<String>>;
    fn list_folders(&mut self) -> TransportResult<Vec<FolderInfo>>;
    fn select(&mut self, mailbox: &str) -> TransportResult<SelectData>;
    fn uid_fetch_headers(&mut self, range: UidRange) -> TransportResult<Vec<FetchedMeta>>;
    /// `UID SEARCH ALL` — used for expunge reconciliation without QRESYNC.
    fn uid_search_all(&mut self) -> TransportResult<Vec<u32>>;
    fn fetch_flags(&mut self, range: UidRange) -> TransportResult<Vec<(u32, Vec<String>)>>;
    fn uid_store_add(&mut self, uids: &[u32], flags: &[String]) -> TransportResult<()>;
    fn uid_store_remove(&mut self, uids: &[u32], flags: &[String]) -> TransportResult<()>;
    /// `UID MOVE` (RFC 6851) — archive path. Returns the new UID when the
    /// server reports COPYUID, else 0.
    fn uid_move(&mut self, uids: &[u32], mailbox: &str) -> TransportResult<u32>;
    /// Lazy body fetch (spec §4.5): text and html parts, PEEK semantics.
    fn fetch_body(&mut self, uid: u32) -> TransportResult<Option<BodyData>>;
    /// Waits up to `timeout_secs` for one round of server activity.
    fn idle_once(&mut self, timeout_secs: u64) -> TransportResult<IdleEvent>;
    /// APPEND with UIDPLUS-assigned UID (Sent copy, spec §4.6).
    fn append(&mut self, mailbox: &str, message: &[u8], flags: &[String]) -> TransportResult<u32>;
}
