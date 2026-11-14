//! Local cache (SQLite) and full-text index (FTS5).
//!
//! Stores, per account and folder, the message list (headers, preview, flags)
//! and the start of each body, plus the full text of messages that were
//! opened. Used to show a folder instantly and to search beyond the messages
//! loaded from the server.
//!
//! The file lives in the user's data directory with owner-only permissions.
//! It is a cache: deleting it loses nothing that is not on the server.

use crate::mail::Header;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub struct Cache {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS folders (
    account     TEXT NOT NULL,
    folder      TEXT NOT NULL,
    uidvalidity INTEGER NOT NULL,
    PRIMARY KEY (account, folder)
);
CREATE TABLE IF NOT EXISTS messages (
    id       INTEGER PRIMARY KEY,
    account  TEXT NOT NULL,
    folder   TEXT NOT NULL,
    uid      INTEGER NOT NULL,
    sender   TEXT NOT NULL,
    subject  TEXT NOT NULL,
    preview  TEXT NOT NULL,
    ts       INTEGER NOT NULL,
    seen     INTEGER NOT NULL,
    attach   INTEGER NOT NULL,
    body     TEXT NOT NULL DEFAULT '',
    full     INTEGER NOT NULL DEFAULT 0,
    msgid    TEXT NOT NULL DEFAULT '',
    parents  TEXT NOT NULL DEFAULT '',
    counted  INTEGER NOT NULL DEFAULT 0,
    UNIQUE (account, folder, uid)
);
CREATE INDEX IF NOT EXISTS messages_list ON messages (account, folder, ts DESC);
-- Address book built from mail: people who wrote to us, people we wrote to.
CREATE TABLE IF NOT EXISTS addresses (
    account  TEXT NOT NULL,
    address  TEXT NOT NULL,
    name     TEXT NOT NULL DEFAULT '',
    received INTEGER NOT NULL DEFAULT 0,
    sent     INTEGER NOT NULL DEFAULT 0,
    last_ts  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (account, address)
);
-- Contacts synced from the account's CardDAV server.
CREATE TABLE IF NOT EXISTS contacts (
    account TEXT NOT NULL,
    url     TEXT NOT NULL,
    book    TEXT NOT NULL,
    etag    TEXT NOT NULL,
    uid     TEXT NOT NULL DEFAULT '',
    name    TEXT NOT NULL,
    emails  TEXT NOT NULL DEFAULT '',
    phones  TEXT NOT NULL DEFAULT '',
    org     TEXT NOT NULL DEFAULT '',
    note    TEXT NOT NULL DEFAULT '',
    vcard   TEXT NOT NULL,
    PRIMARY KEY (account, url)
);
-- Calendars and their events synced from the account's CalDAV server.
-- first_ts/last_ts bound the event's occurrences (last_ts NULL = series
-- without end), so a time range only parses the events it may contain.
CREATE TABLE IF NOT EXISTS calendars (
    account TEXT NOT NULL,
    url     TEXT NOT NULL,
    name    TEXT NOT NULL,
    color   TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (account, url)
);
CREATE TABLE IF NOT EXISTS events (
    account  TEXT NOT NULL,
    url      TEXT NOT NULL,
    calendar TEXT NOT NULL,
    etag     TEXT NOT NULL,
    first_ts INTEGER NOT NULL,
    last_ts  INTEGER,
    ics      TEXT NOT NULL,
    PRIMARY KEY (account, url)
);
CREATE INDEX IF NOT EXISTS events_span ON events (account, first_ts);
-- Senders the user blocked (their mail goes to Spam) or allowed (their
-- mail comes back from Spam). An address, or '@domain' for a whole domain.
CREATE TABLE IF NOT EXISTS sender_rules (
    account TEXT NOT NULL,
    address TEXT NOT NULL,
    allow   INTEGER NOT NULL,
    PRIMARY KEY (account, address)
);
-- The user's rules (see rules.rs), in their order, and the newest inbox
-- message they were applied to.
CREATE TABLE IF NOT EXISTS rules (
    account  TEXT NOT NULL,
    id       TEXT NOT NULL,
    position INTEGER NOT NULL,
    json     TEXT NOT NULL,
    PRIMARY KEY (account, id)
);
CREATE TABLE IF NOT EXISTS rules_done (
    account  TEXT PRIMARY KEY,
    last_uid INTEGER NOT NULL
);
-- Our answers to invitations, by event UID (shown on the invitation even
-- without a calendar server).
CREATE TABLE IF NOT EXISTS invite_replies (
    account  TEXT NOT NULL,
    uid      TEXT NOT NULL,
    partstat TEXT NOT NULL,
    PRIMARY KEY (account, uid)
);
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    sender, subject, body,
    content = 'messages', content_rowid = 'id',
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts (rowid, sender, subject, body)
    VALUES (new.id, new.sender, new.subject, new.body);
END;
CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, sender, subject, body)
    VALUES ('delete', old.id, old.sender, old.subject, old.body);
END;
CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE OF sender, subject, body ON messages BEGIN
    INSERT INTO messages_fts (messages_fts, rowid, sender, subject, body)
    VALUES ('delete', old.id, old.sender, old.subject, old.body);
    INSERT INTO messages_fts (rowid, sender, subject, body)
    VALUES (new.id, new.sender, new.subject, new.body);
END;
";

fn path() -> Option<PathBuf> {
    Some(crate::paths::data_dir()?.join("cache.sqlite"))
}

/// The shared cache, or `None` if it cannot be opened (the app then works
/// without it).
/// The "address book" of contacts kept only on this device (accounts
/// without a CardDAV server, or when saving there failed).
pub const LOCAL_BOOK: &str = "local";

/// The calendar kept on this device, for accounts without a calendar server
/// (its events' URLs start with `local:`). Server syncs leave it alone.
pub const LOCAL_CALENDAR: &str = "local:calendar";

pub fn get() -> Option<&'static Cache> {
    static CACHE: OnceLock<Option<Cache>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let path = path()?;
            match Cache::open(&path) {
                Ok(c) => Some(c),
                Err(e) => {
                    eprintln!("cache disabled: {e:#}");
                    None
                }
            }
        })
        .as_ref()
}

impl Cache {
    pub fn open(path: &std::path::Path) -> Result<Cache> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).context("opening cache")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Self::with_connection(conn)
    }

    fn with_connection(conn: Connection) -> Result<Cache> {
        conn.execute_batch(SCHEMA).context("creating cache schema")?;
        // Columns added after the first release (conversations).
        for (column, ddl) in [
            (
                "msgid",
                "ALTER TABLE messages ADD COLUMN msgid TEXT NOT NULL DEFAULT ''",
            ),
            (
                "parents",
                "ALTER TABLE messages ADD COLUMN parents TEXT NOT NULL DEFAULT ''",
            ),
            (
                "counted",
                "ALTER TABLE messages ADD COLUMN counted INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "flagged",
                "ALTER TABLE messages ADD COLUMN flagged INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "size",
                "ALTER TABLE messages ADD COLUMN size INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "labels",
                "ALTER TABLE messages ADD COLUMN labels TEXT NOT NULL DEFAULT ''",
            ),
            (
                "category",
                "ALTER TABLE messages ADD COLUMN category INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "from_addr",
                "ALTER TABLE messages ADD COLUMN from_addr TEXT NOT NULL DEFAULT ''",
            ),
        ] {
            if !has_column(&conn, "messages", column)? {
                conn.execute_batch(ddl)?;
            }
        }
        // 1: messages without a Date header were cached with timestamp 0
        // before the server's arrival time was used; drop those folders so
        // the next sync downloads them again with a date.
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(
                "DELETE FROM folders WHERE (account, folder) IN
                     (SELECT DISTINCT account, folder FROM messages WHERE ts = 0);
                 DELETE FROM messages WHERE (account, folder) IN
                     (SELECT DISTINCT account, folder FROM messages WHERE ts = 0);
                 PRAGMA user_version = 1;",
            )?;
        }
        // 2: the sender's address was not stored, so lists read from the
        // cache could not be matched by blocked/allowed senders and `from:`
        // rules; download everything once more to fill it in.
        if version < 2 {
            conn.execute_batch(
                "DELETE FROM folders;
                 DELETE FROM messages;
                 PRAGMA user_version = 2;",
            )?;
        }
        Ok(Cache {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn in_memory() -> Cache {
        Self::with_connection(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Records the folder's UIDVALIDITY; if it changed, the cached UIDs are
    /// meaningless and the folder is emptied.
    pub fn check_uidvalidity(&self, account: &str, folder: &str, uidvalidity: u32) -> Result<()> {
        let conn = self.lock();
        let known: Option<u32> = conn
            .query_row(
                "SELECT uidvalidity FROM folders WHERE account = ?1 AND folder = ?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()?;
        if known != Some(uidvalidity) {
            conn.execute(
                "DELETE FROM messages WHERE account = ?1 AND folder = ?2",
                params![account, folder],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO folders (account, folder, uidvalidity) VALUES (?1, ?2, ?3)",
                params![account, folder, uidvalidity],
            )?;
            // The rules' progress is an inbox UID too: kept, it would hide
            // the renumbered mail until it passed the old newest UID.
            // Forgotten, the next run starts again from the newest message.
            if folder.eq_ignore_ascii_case("INBOX") {
                conn.execute("DELETE FROM rules_done WHERE account = ?1", params![account])?;
            }
        }
        Ok(())
    }

    /// Inserts or updates headers. Bodies already stored in full are kept.
    /// Stores list entries. Messages seen for the first time also feed the
    /// address book (`own` = the account's address, to tell sent from received).
    pub fn store_headers(&self, account: &str, folder: &str, headers: &[Header], own: &str) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        {
            // Each message feeds the address book once: new ones, and those
            // cached before the address book existed (counted = 0).
            let mut counted = tx.prepare_cached(
                "SELECT 1 FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3 AND counted = 1",
            )?;
            for h in headers {
                if !counted.exists(params![account, folder, h.uid])? {
                    harvest(&tx, account, own, h)?;
                }
            }
        }
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO messages (account, folder, uid, sender, subject, preview, ts, seen, attach, body, msgid, parents, counted, flagged, size, labels, category, from_addr)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13, ?14, ?15, ?16, ?17)
                 ON CONFLICT (account, folder, uid) DO UPDATE SET
                    counted = 1,
                    seen = excluded.seen,
                    flagged = excluded.flagged,
                    labels = excluded.labels,
                    category = excluded.category,
                    size = CASE WHEN excluded.size > 0 THEN excluded.size ELSE messages.size END,
                    msgid = excluded.msgid,
                    parents = excluded.parents,
                    sender = excluded.sender,
                    from_addr = excluded.from_addr,
                    subject = excluded.subject,
                    preview = excluded.preview,
                    attach = excluded.attach,
                    body = CASE WHEN messages.full THEN messages.body ELSE excluded.body END",
            )?;
            for h in headers {
                stmt.execute(params![
                    account,
                    folder,
                    h.uid,
                    h.from,
                    h.subject,
                    h.preview,
                    h.timestamp,
                    h.seen,
                    h.has_attachment,
                    h.text,
                    h.message_id,
                    h.parents.join(" "),
                    h.flagged,
                    h.size,
                    h.labels.join("\n"),
                    h.category.code(),
                    h.from_address
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Removes cached messages with UID ≥ `from_uid` that the server no longer
    /// lists (deleted or moved elsewhere).
    pub fn prune(&self, account: &str, folder: &str, from_uid: u32, present: &[u32]) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let cached: Vec<u32> = {
            let mut stmt =
                tx.prepare("SELECT uid FROM messages WHERE account = ?1 AND folder = ?2 AND uid >= ?3")?;
            stmt.query_map(params![account, folder, from_uid], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        let present: std::collections::HashSet<u32> = present.iter().copied().collect();
        for uid in cached.into_iter().filter(|u| !present.contains(u)) {
            tx.execute(
                "DELETE FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                params![account, folder, uid],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn store_body(&self, account: &str, folder: &str, uid: u32, body: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE messages SET body = ?4, full = 1, seen = 1
             WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            params![account, folder, uid, body],
        )?;
        Ok(())
    }

    /// Adds or removes one Gmail label of a cached message.
    pub fn set_label(&self, account: &str, folder: &str, uid: u32, label: &str, on: bool) -> Result<()> {
        let conn = self.lock();
        let current: Option<String> = conn
            .query_row(
                "SELECT labels FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                params![account, folder, uid],
                |r| r.get(0),
            )
            .optional()?;
        let Some(current) = current else { return Ok(()) };
        let mut labels: Vec<&str> = current.lines().filter(|l| !l.is_empty() && *l != label).collect();
        if on {
            labels.push(label);
        }
        conn.execute(
            "UPDATE messages SET labels = ?4 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            params![account, folder, uid, labels.join("\n")],
        )?;
        Ok(())
    }

    pub fn set_flagged(&self, account: &str, folder: &str, uid: u32, flagged: bool) -> Result<()> {
        self.lock().execute(
            "UPDATE messages SET flagged = ?4 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            params![account, folder, uid, flagged],
        )?;
        Ok(())
    }

    pub fn set_seen(&self, account: &str, folder: &str, uid: u32, seen: bool) -> Result<()> {
        self.lock().execute(
            "UPDATE messages SET seen = ?4 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            params![account, folder, uid, seen],
        )?;
        Ok(())
    }

    pub fn remove(&self, account: &str, folder: &str, uid: u32) -> Result<()> {
        self.lock().execute(
            "DELETE FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3",
            params![account, folder, uid],
        )?;
        Ok(())
    }

    /// Deletes everything stored for an account: messages (and their search
    /// index), folders, address book, contacts, calendars, events, sender
    /// rules and invitation replies. Used when the account is removed.
    pub fn forget_account(&self, account: &str) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        for table in [
            "messages",
            "folders",
            "addresses",
            "contacts",
            "calendars",
            "events",
            "sender_rules",
            "rules",
            "rules_done",
            "invite_replies",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE account = ?1"),
                params![account],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// The most recent `limit` messages of a folder.
    pub fn headers(&self, account: &str, folder: &str, limit: u32) -> Result<Vec<Header>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT uid, sender, subject, preview, ts, seen, attach, msgid, parents, flagged, size, labels, category, from_addr FROM messages
             WHERE account = ?1 AND folder = ?2 ORDER BY ts DESC, uid DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![account, folder, limit], row_to_header)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The folder's UIDVALIDITY as last seen, if the folder is cached.
    pub fn uidvalidity(&self, account: &str, folder: &str) -> Result<Option<u32>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT uidvalidity FROM folders WHERE account = ?1 AND folder = ?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// UIDs of the newest `limit` cached messages (highest UIDs first).
    pub fn newest_uids(&self, account: &str, folder: &str, limit: u32) -> Result<Vec<u32>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT uid FROM messages WHERE account = ?1 AND folder = ?2 ORDER BY uid DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![account, folder, limit], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Updates read and starred state of cached messages: (uid, seen, flagged).
    pub fn set_flags_many(&self, account: &str, folder: &str, flags: &[(u32, bool, bool)]) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        for (uid, seen, flagged) in flags {
            tx.execute(
                "UPDATE messages SET seen = ?4, flagged = ?5 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                params![account, folder, uid, seen, flagged],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Number of cached messages and lowest cached UID of a folder.
    pub fn folder_stats(&self, account: &str, folder: &str) -> Result<(u32, Option<u32>)> {
        Ok(self.lock().query_row(
            "SELECT COUNT(*), MIN(uid) FROM messages WHERE account = ?1 AND folder = ?2",
            params![account, folder],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    /// Feeds the address book with one message (POP3 downloads).
    pub fn store_address_book(&self, account: &str, own: &str, h: &Header) -> Result<()> {
        harvest(&self.lock(), account, own, h)
    }

    /// Recorded that we sent to these addresses (right after sending).
    pub fn note_sent(&self, account: &str, recipients: &[(String, String)], ts: i64) -> Result<()> {
        let conn = self.lock();
        let h = Header {
            from_address: "\u{0}me".into(),
            recipients: recipients.to_vec(),
            timestamp: ts,
            ..Default::default()
        };
        harvest(&conn, account, "\u{0}me", &h)
    }

    /// Address suggestions for a To/Cc field: addresses starting with, or
    /// names containing, `query`; people we write to first, then the most
    /// frequent and recent senders.
    pub fn suggest(&self, account: &str, query: &str, limit: u32) -> Result<Vec<(String, String)>> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let like = like_escape;
        // Saved contacts come first.
        let mut out: Vec<(String, String)> = Vec::new();
        for c in self.contacts(account, &q)? {
            let name_hit = c.name.to_lowercase().contains(&q);
            for addr in &c.emails {
                if (name_hit || addr.to_lowercase().contains(&q)) && out.len() < limit as usize {
                    out.push((c.name.clone(), addr.to_lowercase()));
                }
            }
        }
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT name, address FROM addresses
             WHERE account = ?1
               AND (address LIKE ?2 ESCAPE '\\' OR lower(name) LIKE ?3 ESCAPE '\\')
             ORDER BY sent * 5 + received DESC, last_ts DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(
            params![
                account,
                format!("{}%", like(&q)),
                format!("%{}%", like(&q)),
                limit * 2
            ],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )?;
        for row in rows {
            let (name, addr) = row?;
            if !NO_REPLY.iter().any(|n| addr.contains(n)) && !out.iter().any(|(_, a)| *a == addr) {
                out.push((name, addr));
            }
        }
        out.truncate(limit as usize);
        Ok(out)
    }

    /// Card URL and ETag of every cached contact (to compare with the
    /// server).
    pub fn contact_etags(&self, account: &str) -> Result<Vec<(String, String)>> {
        let conn = self.lock();
        // Contacts kept only on this device are not on the server.
        let mut stmt =
            conn.prepare_cached("SELECT url, etag FROM contacts WHERE account = ?1 AND book != ?2")?;
        let rows = stmt.query_map(params![account, LOCAL_BOOK], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Stores what a CardDAV sync changed. Contacts of address books that
    /// no longer exist are removed too.
    pub fn apply_contacts(&self, account: &str, changes: &crate::carddav::Changes) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        {
            let mut put = tx.prepare_cached(
                "INSERT OR REPLACE INTO contacts
                 (account, url, book, etag, uid, name, emails, phones, org, note, vcard)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            for (book, card) in &changes.cards {
                let c = &card.contact;
                put.execute(params![
                    account,
                    card.url,
                    book,
                    card.etag,
                    c.uid,
                    c.name,
                    c.emails.join("\n"),
                    c.phones.join("\n"),
                    c.organization,
                    c.note,
                    card.vcard
                ])?;
            }
            let mut del = tx.prepare_cached("DELETE FROM contacts WHERE account = ?1 AND url = ?2")?;
            for url in &changes.gone {
                del.execute(params![account, url])?;
            }
            let books: Vec<&str> = changes.books.iter().map(|b| b.url.as_str()).collect();
            let mut stmt = tx.prepare_cached("SELECT DISTINCT book FROM contacts WHERE account = ?1")?;
            let stored: Vec<String> = stmt
                .query_map(params![account], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            for book in stored
                .iter()
                .filter(|b| b.as_str() != LOCAL_BOOK && !books.contains(&b.as_str()))
            {
                tx.execute(
                    "DELETE FROM contacts WHERE account = ?1 AND book = ?2",
                    params![account, book],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// URLs of the account's address books on its CardDAV server (known
    /// from the last sync).
    pub fn address_books(&self, account: &str) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT DISTINCT book FROM contacts WHERE account = ?1 AND book != ?2 ORDER BY book",
        )?;
        let rows = stmt.query_map(params![account, LOCAL_BOOK], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Whether a saved contact has this address.
    pub fn has_contact_email(&self, account: &str, email: &str) -> Result<bool> {
        let email = email.trim().to_lowercase();
        let conn = self.lock();
        let mut stmt = conn.prepare_cached("SELECT emails FROM contacts WHERE account = ?1")?;
        let rows = stmt.query_map(params![account], |r| r.get::<_, String>(0))?;
        for emails in rows {
            if emails?.lines().any(|e| e.trim().to_lowercase() == email) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Stores a contact just created: on the server (`book` is its address
    /// book; the next sync brings its ETag) or only here ([LOCAL_BOOK]).
    pub fn add_contact(
        &self,
        account: &str,
        url: &str,
        book: &str,
        contact: &crate::vcard::Contact,
        vcard: &str,
    ) -> Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT OR REPLACE INTO contacts
             (account, url, book, etag, uid, name, emails, phones, org, note, vcard)
             VALUES (?1, ?2, ?3, '', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                account,
                url,
                book,
                contact.uid,
                contact.name,
                contact.emails.join("\n"),
                contact.phones.join("\n"),
                contact.organization,
                contact.note,
                vcard
            ],
        )?;
        Ok(())
    }

    /// A stored contact's address book, ETag and vCard UID (to change it
    /// on the server keeping its identity).
    pub fn contact_place(&self, account: &str, url: &str) -> Result<Option<(String, String, String)>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT book, etag, uid FROM contacts WHERE account = ?1 AND url = ?2",
                params![account, url],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?)
    }

    /// A stored contact's whole vCard, as last saved or downloaded.
    pub fn contact_vcard(&self, account: &str, url: &str) -> Result<Option<String>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT vcard FROM contacts WHERE account = ?1 AND url = ?2",
                params![account, url],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn delete_contact(&self, account: &str, url: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM contacts WHERE account = ?1 AND url = ?2",
            params![account, url],
        )?;
        Ok(())
    }

    /// Contacts whose name, address, organization or phone contains
    /// `query` (all of them when empty), by name.
    pub fn contacts(&self, account: &str, query: &str) -> Result<Vec<crate::vcard::Contact>> {
        let q = format!("%{}%", like_escape(&query.trim().to_lowercase()));
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT uid, name, emails, phones, org, note, url FROM contacts
             WHERE account = ?1
               AND (lower(name) LIKE ?2 ESCAPE '\\' OR lower(emails) LIKE ?2 ESCAPE '\\'
                    OR lower(org) LIKE ?2 ESCAPE '\\' OR phones LIKE ?2 ESCAPE '\\')
             ORDER BY name COLLATE NOCASE",
        )?;
        let lines =
            |s: String| -> Vec<String> { s.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect() };
        let rows = stmt.query_map(params![account, q], |r| {
            Ok(crate::vcard::Contact {
                url: r.get(6)?,
                uid: r.get(0)?,
                name: r.get(1)?,
                emails: lines(r.get(2)?),
                phones: lines(r.get(3)?),
                organization: r.get(4)?,
                note: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Item URL and ETag of every cached event (to compare with the server).
    pub fn event_etags(&self, account: &str) -> Result<Vec<(String, String)>> {
        let conn = self.lock();
        // Events kept only on this device are not on the server.
        let mut stmt =
            conn.prepare_cached("SELECT url, etag FROM events WHERE account = ?1 AND calendar != ?2")?;
        let rows = stmt.query_map(params![account, LOCAL_CALENDAR], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Stores what a CalDAV sync changed. The calendars list is replaced;
    /// events of calendars that no longer exist are removed.
    pub fn apply_events(&self, account: &str, changes: &crate::dav::Changes) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM calendars WHERE account = ?1", params![account])?;
        for c in &changes.collections {
            tx.execute(
                "INSERT OR REPLACE INTO calendars (account, url, name, color) VALUES (?1, ?2, ?3, ?4)",
                params![account, c.url, c.name, c.color],
            )?;
        }
        {
            let mut put = tx.prepare_cached(
                "INSERT OR REPLACE INTO events (account, url, calendar, etag, first_ts, last_ts, ics)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (calendar, item) in &changes.items {
                // Objects without events (tasks, journals) are not kept.
                let Some((first, last)) = crate::ical::no_panic(|| crate::ical::span(&crate::ical::parse(&item.data))).flatten() else {
                    continue;
                };
                put.execute(params![
                    account, item.url, calendar, item.etag, first, last, item.data
                ])?;
            }
            let mut del = tx.prepare_cached("DELETE FROM events WHERE account = ?1 AND url = ?2")?;
            for url in &changes.gone {
                del.execute(params![account, url])?;
            }
        }
        tx.execute(
            "DELETE FROM events WHERE account = ?1 AND calendar != ?2
             AND calendar NOT IN (SELECT url FROM calendars WHERE account = ?1)",
            params![account, LOCAL_CALENDAR],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// A stored event by item URL: (calendar URL, ETag, iCalendar text).
    pub fn event(&self, account: &str, url: &str) -> Result<Option<(String, String, String)>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT calendar, etag, ics FROM events WHERE account = ?1 AND url = ?2",
                params![account, url],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?)
    }

    /// Stores an event of the calendar on this device. False when the data
    /// holds no event.
    pub fn put_local_event(&self, account: &str, url: &str, ics: &str) -> Result<bool> {
        let Some((first, last)) = crate::ical::no_panic(|| crate::ical::span(&crate::ical::parse(ics))).flatten() else {
            return Ok(false);
        };
        self.lock().execute(
            "INSERT OR REPLACE INTO events (account, url, calendar, etag, first_ts, last_ts, ics)
             VALUES (?1, ?2, ?3, '', ?4, ?5, ?6)",
            params![account, url, LOCAL_CALENDAR, first, last, ics],
        )?;
        Ok(true)
    }

    /// Whether the calendar on this device holds events.
    pub fn has_local_events(&self, account: &str) -> Result<bool> {
        Ok(self.lock().query_row(
            "SELECT EXISTS(SELECT 1 FROM events WHERE account = ?1 AND calendar = ?2)",
            params![account, LOCAL_CALENDAR],
            |r| r.get(0),
        )?)
    }

    pub fn delete_event(&self, account: &str, url: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM events WHERE account = ?1 AND url = ?2",
            params![account, url],
        )?;
        Ok(())
    }

    /// URL and ETag of the stored event with this UID, if any.
    pub fn event_by_uid(&self, account: &str, uid: &str) -> Result<Option<(String, String)>> {
        if uid.is_empty() {
            return Ok(None);
        }
        let conn = self.lock();
        let mut stmt = conn
            .prepare_cached("SELECT url, etag, ics FROM events WHERE account = ?1 AND instr(ics, ?2) > 0")?;
        let rows = stmt.query_map(params![account, uid], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (url, etag, ics) = row?;
            // The text matched somewhere: make sure it is the UID.
            if crate::ical::parse(&ics).events.iter().any(|e| e.uid == uid) {
                return Ok(Some((url, etag)));
            }
        }
        Ok(None)
    }

    pub fn note_invite_reply(&self, account: &str, uid: &str, partstat: &str) -> Result<()> {
        self.lock().execute(
            "INSERT OR REPLACE INTO invite_replies (account, uid, partstat) VALUES (?1, ?2, ?3)",
            params![account, uid, partstat],
        )?;
        Ok(())
    }

    /// Our answer to the invitation with this UID, if we gave one.
    pub fn invite_reply(&self, account: &str, uid: &str) -> Result<Option<String>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT partstat FROM invite_replies WHERE account = ?1 AND uid = ?2",
                params![account, uid],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Blocks (`allow` false) or allows a sender ("name@domain" or
    /// "@domain"). A sender is in one list at a time.
    pub fn set_sender_rule(&self, account: &str, address: &str, allow: bool) -> Result<()> {
        let address = normalize_sender(address);
        if address.is_empty() {
            return Ok(());
        }
        self.lock().execute(
            "INSERT OR REPLACE INTO sender_rules (account, address, allow) VALUES (?1, ?2, ?3)",
            params![account, address, allow],
        )?;
        Ok(())
    }

    pub fn remove_sender_rule(&self, account: &str, address: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM sender_rules WHERE account = ?1 AND address = ?2",
            params![account, normalize_sender(address)],
        )?;
        Ok(())
    }

    /// The blocked (`allow` false) or allowed senders, sorted.
    pub fn sender_rules(&self, account: &str, allow: bool) -> Result<Vec<String>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT address FROM sender_rules WHERE account = ?1 AND allow = ?2 ORDER BY address",
        )?;
        let rows = stmt.query_map(params![account, allow], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The rule for a sender's address: Some(true) allowed, Some(false)
    /// blocked, None neither. The address wins over its domain.
    pub fn sender_rule(&self, account: &str, address: &str) -> Result<Option<bool>> {
        let address = normalize_sender(address);
        let Some((_, domain)) = address.rsplit_once('@') else {
            return Ok(None);
        };
        let conn = self.lock();
        let mut stmt =
            conn.prepare_cached("SELECT allow FROM sender_rules WHERE account = ?1 AND address = ?2")?;
        for key in [address.clone(), format!("@{domain}")] {
            if let Some(allow) = stmt.query_row(params![account, key], |r| r.get(0)).optional()? {
                return Ok(Some(allow));
            }
        }
        Ok(None)
    }

    /// The account's rules, in their order.
    pub fn rules(&self, account: &str) -> Result<Vec<crate::rules::Rule>> {
        let conn = self.lock();
        let mut stmt =
            conn.prepare_cached("SELECT json FROM rules WHERE account = ?1 ORDER BY position, id")?;
        let rows = stmt.query_map(params![account], |r| r.get::<_, String>(0))?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|json| serde_json::from_str(&json).ok())
            .collect())
    }

    /// Adds a rule (at the end) or replaces the one with its id (in place).
    pub fn save_rule(&self, account: &str, rule: &crate::rules::Rule) -> Result<()> {
        let json = serde_json::to_string(rule)?;
        let conn = self.lock();
        let position: i64 = conn
            .query_row(
                "SELECT position FROM rules WHERE account = ?1 AND id = ?2",
                params![account, rule.id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(conn.query_row(
                "SELECT COALESCE(MAX(position) + 1, 0) FROM rules WHERE account = ?1",
                params![account],
                |r| r.get(0),
            )?);
        conn.execute(
            "INSERT OR REPLACE INTO rules (account, id, position, json) VALUES (?1, ?2, ?3, ?4)",
            params![account, rule.id, position, json],
        )?;
        Ok(())
    }

    pub fn delete_rule(&self, account: &str, id: &str) -> Result<()> {
        self.lock().execute(
            "DELETE FROM rules WHERE account = ?1 AND id = ?2",
            params![account, id],
        )?;
        Ok(())
    }

    /// The newest inbox message the rules already saw (None: never ran).
    pub fn rules_done(&self, account: &str) -> Result<Option<u32>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT last_uid FROM rules_done WHERE account = ?1",
                params![account],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_rules_done(&self, account: &str, last_uid: u32) -> Result<()> {
        self.lock().execute(
            "INSERT OR REPLACE INTO rules_done (account, last_uid) VALUES (?1, ?2)",
            params![account, last_uid],
        )?;
        Ok(())
    }

    /// The account's calendars.
    pub fn calendars(&self, account: &str) -> Result<Vec<crate::dav::Collection>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT url, name, color FROM calendars WHERE account = ?1 ORDER BY name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![account], |r| {
            Ok(crate::dav::Collection {
                url: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Occurrences overlapping `[from, to)` (timestamps), with the URL of
    /// their calendar, sorted by start.
    pub fn occurrences(
        &self,
        account: &str,
        from: i64,
        to: i64,
    ) -> Result<Vec<(String, String, crate::ical::Occurrence)>> {
        let stored: Vec<(String, String, String)> = {
            let conn = self.lock();
            let mut stmt = conn.prepare_cached(
                "SELECT calendar, url, ics FROM events
                 WHERE account = ?1 AND first_ts < ?3 AND (last_ts IS NULL OR last_ts >= ?2)",
            )?;
            let rows = stmt.query_map(params![account, from, to], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let mut out: Vec<(String, String, crate::ical::Occurrence)> = stored
            .into_iter()
            .flat_map(|(calendar, url, ics)| {
                // One broken event must not take the whole agenda down.
                crate::ical::no_panic(|| crate::ical::occurrences(&crate::ical::parse(&ics), from, to))
                    .unwrap_or_default()
                    .into_iter()
                    .map(move |o| (calendar.clone(), url.clone(), o))
            })
            .collect();
        out.sort_by_key(|(_, _, o)| (o.start, !o.all_day, o.end));
        Ok(out)
    }

    /// Full-text search in one folder. Each word matches as a prefix,
    /// accents and case are ignored; all words must match.
    pub fn search(&self, account: &str, folder: &str, words: &[String], limit: u32) -> Result<Vec<Header>> {
        let Some(query) = fts_query(words) else {
            return Ok(Vec::new());
        };
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT m.uid, m.sender, m.subject, m.preview, m.ts, m.seen, m.attach, m.msgid, m.parents, m.flagged, m.size, m.labels, m.category, m.from_addr
             FROM messages_fts f JOIN messages m ON m.id = f.rowid
             WHERE messages_fts MATCH ?3 AND m.account = ?1 AND m.folder = ?2
             ORDER BY m.ts DESC LIMIT ?4",
        )?;
        let rows = stmt.query_map(params![account, folder, query, limit], row_to_header)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn row_to_header(r: &rusqlite::Row<'_>) -> rusqlite::Result<Header> {
    Ok(Header {
        uid: r.get(0)?,
        from: r.get(1)?,
        subject: r.get(2)?,
        preview: r.get(3)?,
        timestamp: r.get(4)?,
        seen: r.get(5)?,
        has_attachment: r.get(6)?,
        text: String::new(),
        message_id: r.get(7)?,
        parents: r
            .get::<_, String>(8)?
            .split_whitespace()
            .map(str::to_owned)
            .collect(),
        flagged: r.get(9)?,
        size: r.get(10)?,
        labels: r
            .get::<_, String>(11)?
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
            .collect(),
        category: crate::mail::Category::from_code(r.get(12)?),
        from_address: r.get(13)?,
        ..Default::default()
    })
}

/// Counts a message in the address book: its sender as "received", or, for
/// messages we sent, its recipients as "sent".
fn harvest(conn: &Connection, account: &str, own: &str, h: &Header) -> Result<()> {
    let own = own.trim().to_lowercase();
    let from = h.from_address.trim().to_lowercase();
    let mut stmt = conn.prepare_cached(
        "INSERT INTO addresses (account, address, name, received, sent, last_ts) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (account, address) DO UPDATE SET
            received = received + excluded.received,
            sent = sent + excluded.sent,
            name = CASE WHEN excluded.name != '' THEN excluded.name ELSE addresses.name END,
            last_ts = MAX(last_ts, excluded.last_ts)",
    )?;
    if !own.is_empty() && from == own {
        for (name, addr) in &h.recipients {
            let addr = addr.trim().to_lowercase();
            if addr != own && addr.contains('@') {
                stmt.execute(params![account, addr, name, 0, 1, h.timestamp])?;
            }
        }
    } else if from.contains('@') {
        let name = if h.from.contains('@') { "" } else { h.from.as_str() };
        stmt.execute(params![account, from, name, 1, 0, h.timestamp])?;
    }
    Ok(())
}

/// Lower case, without spaces or angle brackets.
fn normalize_sender(s: &str) -> String {
    s.trim().trim_matches(['<', '>']).trim().to_lowercase()
}

/// Escapes LIKE wildcards (used with `ESCAPE '\\'`).
fn like_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

/// Automated senders are never suggested as recipients.
const NO_REPLY: [&str; 5] = [
    "noreply",
    "no-reply",
    "donotreply",
    "do-not-reply",
    "nonrispondere",
];

/// Whether a table already has a column (for adding columns to old files).
pub(crate) fn has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt.query_map([], |r| r.get::<_, String>(1))?;
    for name in names {
        if name? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Builds an FTS5 query from user words: each word quoted (so operators and
/// punctuation are literal) and used as a prefix.
fn fts_query(words: &[String]) -> Option<String> {
    let terms: Vec<String> = words
        .iter()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.trim().is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// Cache key of an account (same address on two servers stays separate).
pub fn account_key(account: &crate::config::Account) -> String {
    format!("{}@{}", account.username, account.imap_host)
}

/// Deletes the cached data of a removed account from the device. The
/// cache only holds copies of what is on the servers, so nothing is lost;
/// a POP3 account's local mailbox (`local.sqlite`) is left alone.
pub fn forget_account(account: &crate::config::Account) {
    if let Some(cache) = get()
        && let Err(e) = cache.forget_account(&account_key(account))
    {
        eprintln!("could not clear cached data of the removed account: {e:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forget_account_clears_only_that_account() {
        let c = Cache::in_memory();
        c.check_uidvalidity("a", "INBOX", 1).unwrap();
        c.store_headers("a", "INBOX", &[h(1, "x", "secret", "hidden words")], "")
            .unwrap();
        c.store_headers("b", "INBOX", &[h(1, "y", "kept", "other words")], "")
            .unwrap();
        c.set_sender_rule("a", "spam@x.it", false).unwrap();
        c.forget_account("a").unwrap();
        assert!(c.headers("a", "INBOX", 10).unwrap().is_empty());
        assert!(c.uidvalidity("a", "INBOX").unwrap().is_none());
        assert!(c.sender_rules("a", false).unwrap().is_empty());
        assert!(c.search("a", "INBOX", &["hidden".into()], 10).unwrap().is_empty());
        assert_eq!(c.headers("b", "INBOX", 10).unwrap().len(), 1);
        assert_eq!(c.search("b", "INBOX", &["other".into()], 10).unwrap().len(), 1);
    }

    fn h(uid: u32, from: &str, subject: &str, text: &str) -> Header {
        Header {
            uid,
            from: from.into(),
            subject: subject.into(),
            preview: text.chars().take(20).collect(),
            timestamp: uid as i64,
            seen: false,
            has_attachment: false,
            text: text.into(),
            ..Default::default()
        }
    }

    #[test]
    fn caches_without_sender_addresses_are_dropped_once() {
        let path = std::env::temp_dir().join(format!("ferrowl-cache-test-{}.sqlite", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut from_mario = h(1, "x", "dated", "");
        from_mario.from_address = "mario@z.it".into();
        {
            let c = Cache::open(&path).unwrap();
            c.store_headers("a", "INBOX", std::slice::from_ref(&from_mario), "")
                .unwrap();
            c.conn
                .lock()
                .unwrap()
                .execute_batch("PRAGMA user_version = 1")
                .unwrap();
        }
        {
            let c = Cache::open(&path).unwrap();
            assert!(c.headers("a", "INBOX", 10).unwrap().is_empty());
            c.store_headers("a", "INBOX", &[from_mario], "").unwrap();
        }
        // Read back from the cache, the sender's address is still there
        // (blocked senders and `from:` rules match on it).
        let c = Cache::open(&path).unwrap();
        let cached = c.headers("a", "INBOX", 10).unwrap();
        assert_eq!(cached[0].from_address, "mario@z.it");
        assert_eq!(
            c.search("a", "INBOX", &["dated".into()], 10).unwrap()[0].from_address,
            "mario@z.it"
        );
        drop(c);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn local_events_survive_server_syncs() {
        let c = Cache::in_memory();
        let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261110T090000Z\r\nDTEND:20261110T100000Z\r\nSUMMARY:Dentista\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        assert!(c.put_local_event("acc", "local:a", ics).unwrap());
        // A server sync (no calendars there) neither reports it nor removes it.
        assert!(c.event_etags("acc").unwrap().is_empty());
        c.apply_events("acc", &crate::dav::Changes::default()).unwrap();
        assert!(c.event("acc", "local:a").unwrap().is_some());
        assert_eq!(
            c.event_by_uid("acc", "a").unwrap().map(|(url, _)| url),
            Some("local:a".to_string())
        );
        c.delete_event("acc", "local:a").unwrap();
        assert!(c.event("acc", "local:a").unwrap().is_none());
    }

    #[test]
    fn rules_keep_their_order() {
        use crate::rules::{Action, Rule};
        let c = Cache::in_memory();
        let rule = |id: &str, condition: &str| Rule {
            id: id.into(),
            name: String::new(),
            condition: condition.into(),
            actions: vec![Action::MarkRead],
            enabled: true,
        };
        c.save_rule("a", &rule("2", "from:b")).unwrap();
        c.save_rule("a", &rule("1", "from:a")).unwrap();
        // Replaced in place, not moved to the end.
        c.save_rule("a", &rule("2", "from:c")).unwrap();
        let saved = c.rules("a").unwrap();
        assert_eq!(
            saved.iter().map(|r| r.condition.as_str()).collect::<Vec<_>>(),
            ["from:c", "from:a"]
        );
        c.delete_rule("a", "2").unwrap();
        assert_eq!(c.rules("a").unwrap().len(), 1);
        assert!(c.rules("b").unwrap().is_empty());
        assert_eq!(c.rules_done("a").unwrap(), None);
        c.set_rules_done("a", 42).unwrap();
        assert_eq!(c.rules_done("a").unwrap(), Some(42));
    }

    #[test]
    fn new_inbox_uidvalidity_resets_rules_progress() {
        let c = Cache::in_memory();
        c.check_uidvalidity("a", "INBOX", 1).unwrap();
        c.set_rules_done("a", 900).unwrap();
        c.set_rules_done("b", 50).unwrap();
        // Same validity, other folders: the progress stays.
        c.check_uidvalidity("a", "INBOX", 1).unwrap();
        c.check_uidvalidity("a", "Archive", 7).unwrap();
        assert_eq!(c.rules_done("a").unwrap(), Some(900));
        // The server renumbered the inbox: the old UIDs mean nothing.
        c.check_uidvalidity("a", "INBOX", 2).unwrap();
        assert_eq!(c.rules_done("a").unwrap(), None);
        assert_eq!(c.rules_done("b").unwrap(), Some(50));
    }

    #[test]
    fn sender_rules() {
        let c = Cache::in_memory();
        c.set_sender_rule("a", " <Spam@Bad.example> ", false).unwrap();
        c.set_sender_rule("a", "@friends.example", true).unwrap();
        assert_eq!(c.sender_rule("a", "spam@bad.example").unwrap(), Some(false));
        assert_eq!(c.sender_rule("a", "anyone@friends.example").unwrap(), Some(true));
        assert_eq!(c.sender_rule("a", "other@bad.example").unwrap(), None);
        // The address wins over its domain; one list at a time.
        c.set_sender_rule("a", "@bad.example", false).unwrap();
        c.set_sender_rule("a", "spam@bad.example", true).unwrap();
        assert_eq!(c.sender_rule("a", "spam@bad.example").unwrap(), Some(true));
        assert_eq!(c.sender_rules("a", false).unwrap(), ["@bad.example"]);
        assert_eq!(
            c.sender_rules("a", true).unwrap(),
            ["@friends.example", "spam@bad.example"]
        );
        c.remove_sender_rule("a", "SPAM@bad.example").unwrap();
        assert_eq!(
            c.sender_rule("a", "spam@bad.example").unwrap(),
            Some(false),
            "domain rule again"
        );
    }

    #[test]
    fn stars_are_stored_and_changed() {
        let c = Cache::in_memory();
        let h = |uid: u32, flagged: bool| Header {
            uid,
            timestamp: uid as i64,
            flagged,
            ..Default::default()
        };
        c.store_headers("a", "INBOX", &[h(1, true), h(2, false)], "")
            .unwrap();
        let stars = |c: &Cache| -> Vec<(u32, bool)> {
            c.headers("a", "INBOX", 10)
                .unwrap()
                .iter()
                .map(|h| (h.uid, h.flagged))
                .collect()
        };
        assert_eq!(stars(&c), [(2, false), (1, true)]);
        c.set_flagged("a", "INBOX", 2, true).unwrap();
        c.set_flagged("a", "INBOX", 1, false).unwrap();
        assert_eq!(stars(&c), [(2, true), (1, false)]);
        // A refresh from the server wins.
        c.store_headers("a", "INBOX", &[h(1, true)], "").unwrap();
        assert_eq!(stars(&c)[1], (1, true));
    }

    #[test]
    fn gmail_labels_are_stored_and_changed() {
        let c = Cache::in_memory();
        let h = Header {
            uid: 7,
            labels: vec!["Lavoro".into()],
            ..Default::default()
        };
        c.store_headers("a", "INBOX", &[h], "").unwrap();
        c.set_label("a", "INBOX", 7, "Clienti", true).unwrap();
        assert_eq!(
            c.headers("a", "INBOX", 5).unwrap()[0].labels,
            ["Lavoro", "Clienti"]
        );
        c.set_label("a", "INBOX", 7, "Lavoro", false).unwrap();
        assert_eq!(c.headers("a", "INBOX", 5).unwrap()[0].labels, ["Clienti"]);
    }

    #[test]
    fn address_book_from_mail() {
        let c = Cache::in_memory();
        let mail = |uid: u32, from: &str, addr: &str, to: &[(&str, &str)]| Header {
            uid,
            from: from.into(),
            from_address: addr.into(),
            recipients: to.iter().map(|(n, a)| (n.to_string(), a.to_string())).collect(),
            timestamp: uid as i64,
            ..Default::default()
        };
        let list = [
            mail(1, "Ada Lovelace", "ada@x.org", &[]),
            mail(2, "Ada Lovelace", "ada@x.org", &[]),
            mail(3, "News", "noreply@shop.it", &[]),
            mail(4, "Me", "me@y.org", &[("Bob Smith", "bob@z.org")]),
        ];
        c.store_headers("a", "INBOX", &list, "me@y.org").unwrap();
        // Stored again (refresh): not counted twice.
        c.store_headers("a", "INBOX", &list, "me@y.org").unwrap();

        let names = |q: &str| c.suggest("a", q, 5).unwrap();
        assert_eq!(names("ada"), vec![("Ada Lovelace".into(), "ada@x.org".into())]);
        assert_eq!(names("smith"), vec![("Bob Smith".into(), "bob@z.org".into())]);
        assert!(names("noreply").is_empty(), "automated senders are skipped");
        assert!(names("me@").is_empty(), "our own address is not stored");

        // People we write to come before frequent senders.
        c.note_sent("a", &[("".into(), "ada2@x.org".into())], 9).unwrap();
        c.note_sent("a", &[("".into(), "ada2@x.org".into())], 10).unwrap();
        assert_eq!(names("ada")[0].1, "ada2@x.org");
    }

    #[test]
    fn store_search_prune() {
        let c = Cache::in_memory();
        c.check_uidvalidity("a", "INBOX", 1).unwrap();
        c.store_headers(
            "a",
            "INBOX",
            &[
                h(
                    1,
                    "Giulia Ferri",
                    "Preventivo portale",
                    "Ciao Fabio, ecco il preventivo aggiornato",
                ),
                h(2, "Banca", "Estratto conto", "Il tuo estratto conto di ottobre"),
                h(3, "Città di Roma", "Avviso", "Comunicazione importante"),
            ],
            "",
        )
        .unwrap();
        let words = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
        assert_eq!(c.search("a", "INBOX", &words("preven"), 10).unwrap().len(), 1);
        assert_eq!(
            c.search("a", "INBOX", &words("estratto ottobre"), 10).unwrap()[0].uid,
            2
        );
        // Accents ignored.
        assert_eq!(c.search("a", "INBOX", &words("citta"), 10).unwrap()[0].uid, 3);
        // Operators are literal.
        assert!(
            c.search("a", "INBOX", &words("\"OR NEAR("), 10)
                .unwrap()
                .is_empty()
        );
        // Full body replaces the snippet and stays on later header refreshes.
        c.store_body("a", "INBOX", 2, "testo completo con parola rara zibaldone")
            .unwrap();
        c.store_headers("a", "INBOX", &[h(2, "Banca", "Estratto conto", "snippet")], "")
            .unwrap();
        assert_eq!(c.search("a", "INBOX", &words("zibaldone"), 10).unwrap().len(), 1);
        // Prune removes messages gone from the server.
        c.prune("a", "INBOX", 1, &[1, 3]).unwrap();
        assert_eq!(c.headers("a", "INBOX", 10).unwrap().len(), 2);
        assert!(
            c.search("a", "INBOX", &words("zibaldone"), 10)
                .unwrap()
                .is_empty()
        );
        // Only from the given UID up: older messages stay...
        c.prune("a", "INBOX", 3, &[]).unwrap();
        assert_eq!(c.headers("a", "INBOX", 10).unwrap()[0].uid, 1);
        // ...unless the whole folder is checked (a folder emptied elsewhere).
        c.prune("a", "INBOX", 0, &[]).unwrap();
        assert!(c.headers("a", "INBOX", 10).unwrap().is_empty());
        // UIDVALIDITY change empties the folder.
        c.check_uidvalidity("a", "INBOX", 2).unwrap();
        assert!(c.headers("a", "INBOX", 10).unwrap().is_empty());
    }

    #[test]
    fn contacts_are_stored_searched_and_suggested_first() {
        use crate::carddav::{Book, Card, Changes};
        let c = Cache::in_memory();
        let card = |url: &str, name: &str, email: &str| {
            (
                "https://d/book/".to_owned(),
                Card {
                    url: url.into(),
                    etag: "1".into(),
                    vcard: String::new(),
                    contact: crate::vcard::Contact {
                        name: name.into(),
                        emails: vec![email.into()],
                        ..Default::default()
                    },
                },
            )
        };
        let books = vec![Book {
            url: "https://d/book/".into(),
            name: "B".into(),
            ..Default::default()
        }];
        c.apply_contacts(
            "acc",
            &Changes {
                books: books.clone(),
                cards: vec![
                    card("u1", "Marta Neri", "marta@x.it"),
                    card("u2", "Luca Blu", "luca@y.it"),
                ],
                gone: vec![],
            },
        )
        .unwrap();
        // A frequent correspondent with the same prefix comes after the contact.
        let h = Header {
            from: "Mario".into(),
            from_address: "mario@z.it".into(),
            timestamp: 5,
            ..Default::default()
        };
        c.store_address_book("acc", "me@x.it", &h).unwrap();
        let s = c.suggest("acc", "mar", 8).unwrap();
        assert_eq!(s[0], ("Marta Neri".into(), "marta@x.it".into()));
        assert_eq!(s[1].1, "mario@z.it");
        assert_eq!(c.contacts("acc", "y.it").unwrap()[0].name, "Luca Blu");
        assert_eq!(c.contact_etags("acc").unwrap().len(), 2);

        // Deleted card, then the whole book gone.
        c.apply_contacts(
            "acc",
            &Changes {
                books,
                cards: vec![],
                gone: vec!["u2".into()],
            },
        )
        .unwrap();
        assert_eq!(c.contacts("acc", "").unwrap().len(), 1);
        c.apply_contacts("acc", &Changes::default()).unwrap();
        assert!(c.contacts("acc", "").unwrap().is_empty());
    }

    #[test]
    fn local_contacts_survive_syncs() {
        use crate::carddav::Changes;
        let c = Cache::in_memory();
        let contact = crate::vcard::Contact {
            uid: "1@ferrowl".into(),
            name: "Ada".into(),
            emails: vec!["ada@x.org".into()],
            ..Default::default()
        };
        c.add_contact("acc", "local:1", LOCAL_BOOK, &contact, "").unwrap();
        assert!(c.has_contact_email("acc", "ADA@x.org").unwrap());
        assert!(!c.has_contact_email("acc", "bob@x.org").unwrap());
        // Not sent to the server's sync, not removed by it.
        assert!(c.contact_etags("acc").unwrap().is_empty());
        assert!(c.address_books("acc").unwrap().is_empty());
        c.apply_contacts("acc", &Changes::default()).unwrap();
        assert_eq!(c.contacts("acc", "").unwrap().len(), 1);
    }

    #[test]
    fn events_are_stored_and_expanded_in_a_range() {
        use crate::dav::{Changes, Collection, Item};
        let c = Cache::in_memory();
        let ics = |uid: &str, rule: &str| {
            format!(
                "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:{uid}\nSUMMARY:{uid}\nDTSTART:20261102T090000Z\nDTEND:20261102T100000Z\n{rule}END:VEVENT\nEND:VCALENDAR\n"
            )
        };
        let item = |url: &str, data: String| {
            (
                "https://d/cal/".to_owned(),
                Item {
                    url: url.into(),
                    etag: "1".into(),
                    data,
                },
            )
        };
        let cals = vec![Collection {
            url: "https://d/cal/".into(),
            name: "Lavoro".into(),
            color: "#A63A0CFF".into(),
        }];
        c.apply_events(
            "acc",
            &Changes {
                collections: cals.clone(),
                items: vec![
                    item("u1", ics("once", "")),
                    item("u2", ics("daily", "RRULE:FREQ=DAILY\n")),
                    item(
                        "u3",
                        "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:t\nEND:VTODO\nEND:VCALENDAR\n".into(),
                    ),
                ],
                gone: vec![],
            },
        )
        .unwrap();
        assert_eq!(c.event_etags("acc").unwrap().len(), 2, "tasks are not kept");
        // 2-4 Nov: the one-off on the 2nd plus three daily occurrences.
        let day = 86_400;
        let from = 1_793_577_600; // 2026-11-02T00:00:00Z
        let list = c.occurrences("acc", from, from + 3 * day).unwrap();
        assert_eq!(list.len(), 4);
        // A year later only the endless series remains.
        let later = c.occurrences("acc", from + 365 * day, from + 366 * day).unwrap();
        assert_eq!(later.len(), 1);
        assert_eq!(later[0].2.summary, "daily");
        assert_eq!(c.calendars("acc").unwrap(), cals);

        c.apply_events("acc", &Changes::default()).unwrap();
        assert!(c.event_etags("acc").unwrap().is_empty(), "calendar removed");
    }
}
