//! Mail engine (spec §4): IMAP transport abstraction, sync engine, JWZ
//! threading, scripted mock for L3 tests. SMTP send path lands in Phase 3.
pub mod headers;
pub mod imap;
pub mod mock;
pub mod sync;
pub mod thread;
pub mod transport;
