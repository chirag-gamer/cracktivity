//! Mail engine (spec §4): IMAP transport abstraction, sync engine, JWZ
//! threading, user actions (read/star/archive/corrections), scripted mock
//! for L3 tests. SMTP send path lands with Phase 3 compose work.
pub mod actions;
pub mod headers;
pub mod imap;
pub mod mock;
pub mod sync;
pub mod thread;
pub mod transport;
