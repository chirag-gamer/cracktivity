//! Versioned schema migrations (spec §3). Version 0 = fresh database.
//! Each migration runs inside a transaction; `user_version` is bumped after.
//! Migrations are append-only once shipped (like decisions.md).

/// v1 — full initial schema per spec §3. All tables carry created_at/updated_at,
/// FKs enforced (PRAGMA foreign_keys = ON at open), FTS5 external-content
/// table over messages kept in sync by triggers.
pub const V1: &str = r#"
CREATE TABLE accounts(
  id INTEGER PRIMARY KEY,
  provider TEXT NOT NULL CHECK(provider IN ('gmail','outlook','icloud','zoho','yahoo','imap')),
  email TEXT NOT NULL,
  display_name TEXT,
  color TEXT,
  ord INTEGER NOT NULL DEFAULT 0,
  auth_type TEXT NOT NULL CHECK(auth_type IN ('app_password','oauth')),
  credential_ref TEXT NOT NULL,
  sync_state TEXT NOT NULL DEFAULT 'idle',
  backfill_days INTEGER NOT NULL DEFAULT 90,
  enabled INTEGER NOT NULL DEFAULT 1,
  last_full_sync_at INTEGER,
  last_idle_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE folders(
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  name TEXT NOT NULL,
  role TEXT CHECK(role IN ('inbox','sent','drafts','trash','all','archive','junk') OR role IS NULL),
  uidvalidity INTEGER NOT NULL DEFAULT 1,
  uidnext INTEGER NOT NULL DEFAULT 1,
  highestmodseq TEXT,
  highest_uid INTEGER NOT NULL DEFAULT 0,
  subscribed INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(account_id, name)
);

CREATE TABLE messages(
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
  uid INTEGER NOT NULL,
  message_id TEXT,
  in_reply_to TEXT,
  references_raw TEXT,
  subject TEXT,
  from_json TEXT,
  to_json TEXT,
  cc_json TEXT,
  reply_to_json TEXT,
  date_sent INTEGER,
  date_recv INTEGER,
  size INTEGER,
  flags TEXT,
  snippet TEXT,
  body_text TEXT,
  body_html TEXT,
  headers_json TEXT,
  list_unsubscribe TEXT,
  list_unsub_post TEXT,
  thrid TEXT,
  unread INTEGER NOT NULL DEFAULT 1,
  starred INTEGER NOT NULL DEFAULT 0,
  importance TEXT CHECK(importance IN ('important','other') OR importance IS NULL),
  importance_reasons_json TEXT,
  tracking_count INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE(account_id, folder_id, uid)
);

CREATE TABLE threads(
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  subject_norm TEXT,
  participants_json TEXT,
  last_activity INTEGER,
  importance TEXT CHECK(importance IN ('important','other') OR importance IS NULL),
  needs_followup INTEGER NOT NULL DEFAULT 0,
  followup_since INTEGER,
  summary TEXT,
  summary_model TEXT,
  summary_at INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE thread_msgs(
  thread_id INTEGER NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
  message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  PRIMARY KEY(thread_id, message_id)
);

CREATE TABLE attachments(
  id INTEGER PRIMARY KEY,
  message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  filename TEXT,
  mime TEXT,
  size INTEGER,
  cid TEXT,
  part_path TEXT,
  cached INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE contacts(
  id INTEGER PRIMARY KEY,
  account_id INTEGER REFERENCES accounts(id) ON DELETE CASCADE,
  email TEXT NOT NULL,
  name TEXT,
  sent_to INTEGER NOT NULL DEFAULT 0,
  recv_from INTEGER NOT NULL DEFAULT 0,
  is_vip INTEGER NOT NULL DEFAULT 0,
  last_seen INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_contacts_email ON contacts(email);

CREATE TABLE sender_stats(
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  sender_email TEXT NOT NULL,
  opened INTEGER NOT NULL DEFAULT 0,
  deleted_unread INTEGER NOT NULL DEFAULT 0,
  received INTEGER NOT NULL DEFAULT 0,
  classification_overrides TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY(account_id, sender_email)
);

CREATE TABLE drafts(
  id INTEGER PRIMARY KEY,
  account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
  thread_id INTEGER REFERENCES threads(id) ON DELETE SET NULL,
  to_json TEXT,
  cc_json TEXT,
  subject TEXT,
  body_text TEXT,
  in_reply_to TEXT,
  status TEXT NOT NULL CHECK(status IN ('draft','queued','sending','failed')),
  error TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE settings(
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE blocklist(
  id INTEGER PRIMARY KEY,
  pattern TEXT NOT NULL,
  kind TEXT NOT NULL CHECK(kind IN ('domain','url_param','pixel_regex')),
  source TEXT,
  added_at INTEGER NOT NULL
);

CREATE TABLE audit_log(
  id INTEGER PRIMARY KEY,
  at INTEGER NOT NULL,
  actor TEXT NOT NULL CHECK(actor IN ('mcp','user')),
  action TEXT NOT NULL,
  detail_json TEXT
);

CREATE VIRTUAL TABLE messages_fts USING fts5(
  body_text, subject, snippet, from_json,
  content='messages', content_rowid='id'
);

CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, body_text, subject, snippet, from_json)
  VALUES (new.id, new.body_text, new.subject, new.snippet, new.from_json);
END;

CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, body_text, subject, snippet, from_json)
  VALUES ('delete', old.id, old.body_text, old.subject, old.snippet, old.from_json);
END;

CREATE TRIGGER messages_fts_update AFTER UPDATE OF body_text, subject, snippet, from_json ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, body_text, subject, snippet, from_json)
  VALUES ('delete', old.id, old.body_text, old.subject, old.snippet, old.from_json);
  INSERT INTO messages_fts(rowid, body_text, subject, snippet, from_json)
  VALUES (new.id, new.body_text, new.subject, new.snippet, new.from_json);
END;

CREATE INDEX idx_messages_folder_date ON messages(account_id, folder_id, date_recv DESC);
CREATE INDEX idx_messages_thrid ON messages(thrid);
CREATE INDEX idx_messages_unread ON messages(unread);
CREATE INDEX idx_folders_account ON folders(account_id);
"#;

/// v2 — generic-IMAP connection data (FR-1 manual entry): preset providers
/// resolve host/port from `providers::ALL`; manual accounts store theirs.
pub const V2: &str = r"
ALTER TABLE accounts ADD COLUMN imap_host TEXT;
ALTER TABLE accounts ADD COLUMN imap_port INTEGER;
";

/// v3 — Phase-4 features: `messages.opened_at` latches the first reader open
/// per message (sender_stats.opened increments exactly once — FR-14 open
/// ranking), and a partial index serves the subscription-cleanup view.
pub const V3: &str = r"
ALTER TABLE messages ADD COLUMN opened_at INTEGER;
CREATE INDEX idx_messages_list_unsub ON messages(list_unsubscribe) WHERE list_unsubscribe IS NOT NULL;
";

/// v4 — SMTP submission endpoints (FR-18): presets resolve theirs from
/// `providers::ALL`; manual accounts store theirs explicitly.
pub const V4: &str = r"
ALTER TABLE accounts ADD COLUMN smtp_host TEXT;
ALTER TABLE accounts ADD COLUMN smtp_port INTEGER;
";

/// v5 — per-sender image allowlist (FR-15/FR-19: "Always for this sender").
/// NULL = ask per message; 1/0 = decided.
pub const V5: &str = r"
ALTER TABLE sender_stats ADD COLUMN show_images INTEGER;
";

pub const MIGRATIONS: &[(&str, &str)] = &[
    ("1_create_core_schema", V1),
    ("2_account_imap_host", V2),
    ("3_opened_at_and_cleanup_index", V3),
    ("4_account_smtp_host", V4),
    ("5_sender_show_images", V5),
];
