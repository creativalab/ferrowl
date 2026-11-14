//! Local mailbox for POP3 accounts (SQLite).
//!
//! POP3 only downloads the inbox, so for these accounts this file *is* the
//! mailbox: every message (raw, plus the fields shown in the list) lives
//! here, in fixed folders (Inbox, Sent, Archive, Trash). Unlike the cache,
//! deleting it loses mail.
//!
//! `pop3_downloaded` remembers which server messages (by UIDL) were already
//! fetched, also after they are deleted locally, so they are not fetched
//! again; it also dates each download for "keep on server for N days".

use crate::mail::{Folder, FolderKind, Header};
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const INBOX: &str = "INBOX";
pub const SENT: &str = "Sent";
pub const ARCHIVE: &str = "Archive";
pub const TRASH: &str = "Trash";
pub const SPAM: &str = "Spam";

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS local_messages (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    account TEXT NOT NULL,
    folder  TEXT NOT NULL,
    sender  TEXT NOT NULL,
    subject TEXT NOT NULL,
    preview TEXT NOT NULL,
    ts      INTEGER NOT NULL,
    seen    INTEGER NOT NULL,
    attach  INTEGER NOT NULL,
    text    TEXT NOT NULL,
    raw     BLOB NOT NULL,
    msgid   TEXT NOT NULL DEFAULT '',
    parents TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS local_list ON local_messages (account, folder, ts DESC);
CREATE TABLE IF NOT EXISTS pop3_downloaded (
    account TEXT NOT NULL,
    uidl    TEXT NOT NULL,
    at      INTEGER NOT NULL,
    PRIMARY KEY (account, uidl)
);
";

pub struct Store {
    conn: Mutex<Connection>,
}

fn path() -> Option<PathBuf> {
    Some(crate::paths::data_dir()?.join("local.sqlite"))
}

/// The shared local mailbox, or `None` if it cannot be opened.
pub fn get() -> Option<&'static Store> {
    static STORE: OnceLock<Option<Store>> = OnceLock::new();
    STORE
        .get_or_init(|| match path().map(|p| Store::open(&p)) {
            Some(Ok(s)) => Some(s),
            Some(Err(e)) => {
                eprintln!("local mailbox unavailable: {e:#}");
                None
            }
            None => None,
        })
        .as_ref()
}

/// The fixed folders of a POP3 account.
pub fn folders() -> Vec<Folder> {
    [
        (INBOX, FolderKind::Inbox),
        (SENT, FolderKind::Sent),
        (ARCHIVE, FolderKind::Archive),
        (SPAM, FolderKind::Spam),
        (TRASH, FolderKind::Trash),
    ]
    .into_iter()
    .map(|(name, kind)| Folder {
        name: name.into(),
        path: vec![name.into()],
        delimiter: None,
        kind,
        selectable: true,
    })
    .collect()
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Store {
    pub fn open(path: &std::path::Path) -> Result<Store> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let conn = Connection::open(path).context("opening local mailbox")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Self::with_connection(conn)
    }

    fn with_connection(conn: Connection) -> Result<Store> {
        conn.execute_batch(SCHEMA).context("creating local mailbox")?;
        for (column, ddl) in [
            (
                "msgid",
                "ALTER TABLE local_messages ADD COLUMN msgid TEXT NOT NULL DEFAULT ''",
            ),
            (
                "parents",
                "ALTER TABLE local_messages ADD COLUMN parents TEXT NOT NULL DEFAULT ''",
            ),
            (
                "flagged",
                "ALTER TABLE local_messages ADD COLUMN flagged INTEGER NOT NULL DEFAULT 0",
            ),
            (
                "size",
                "ALTER TABLE local_messages ADD COLUMN size INTEGER NOT NULL DEFAULT 0",
            ),
        ] {
            if !crate::cache::has_column(&conn, "local_messages", column)? {
                conn.execute_batch(ddl)?;
            }
        }
        // The sender's address (for blocked/allowed senders and `from:`
        // rules), read once from the stored messages when the column is new.
        if !crate::cache::has_column(&conn, "local_messages", "from_addr")? {
            conn.execute_batch("ALTER TABLE local_messages ADD COLUMN from_addr TEXT NOT NULL DEFAULT ''")?;
            let rows: Vec<(u32, Vec<u8>)> = {
                let mut stmt = conn.prepare("SELECT id, raw FROM local_messages")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for (id, raw) in rows {
                let h = crate::mail::header_from_raw(0, &raw, false);
                conn.execute(
                    "UPDATE local_messages SET from_addr = ?2 WHERE id = ?1",
                    params![id, h.from_address],
                )?;
            }
        }
        Ok(Store {
            conn: Mutex::new(conn),
        })
    }

    #[cfg(test)]
    pub fn in_memory() -> Store {
        Self::with_connection(Connection::open_in_memory().unwrap()).unwrap()
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Adds a message to a folder; returns its id (used as UID).
    pub fn insert(&self, account: &str, folder: &str, raw: &[u8], seen: bool) -> Result<u32> {
        insert(&self.lock(), account, folder, raw, seen)
    }

    /// Stores a message downloaded from the server and remembers its UIDL,
    /// in one transaction (no duplicates if the app stops halfway).
    pub fn add_downloaded(&self, account: &str, uidl: &str, raw: &[u8]) -> Result<u32> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let id = insert(&tx, account, INBOX, raw, false)?;
        tx.execute(
            "INSERT OR REPLACE INTO pop3_downloaded (account, uidl, at) VALUES (?1, ?2, ?3)",
            params![account, uidl, now()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    /// UIDLs already downloaded, with the download time.
    pub fn downloaded(&self, account: &str) -> Result<HashMap<String, i64>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached("SELECT uidl, at FROM pop3_downloaded WHERE account = ?1")?;
        let rows = stmt.query_map(params![account], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Forgets UIDLs that are no longer on the server.
    pub fn forget_downloaded(&self, account: &str, uidls: &[String]) -> Result<()> {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        for uidl in uidls {
            tx.execute(
                "DELETE FROM pop3_downloaded WHERE account = ?1 AND uidl = ?2",
                params![account, uidl],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn headers(&self, account: &str, folder: &str, limit: u32) -> Result<Vec<Header>> {
        let conn = self.lock();
        let mut stmt = conn.prepare_cached(
            "SELECT id, sender, subject, preview, ts, seen, attach, msgid, parents, flagged, CASE WHEN size > 0 THEN size ELSE length(raw) END, from_addr FROM local_messages
             WHERE account = ?1 AND folder = ?2 ORDER BY ts DESC, id DESC LIMIT ?3",
        )?;
        let rows = stmt.query_map(params![account, folder, limit], row_to_header)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Messages of a folder containing every word (sender, subject or text).
    pub fn search(&self, account: &str, folder: &str, words: &[String], limit: u32) -> Result<Vec<Header>> {
        let conn = self.lock();
        let mut sql = String::from(
            "SELECT id, sender, subject, preview, ts, seen, attach, msgid, parents, flagged, CASE WHEN size > 0 THEN size ELSE length(raw) END, from_addr FROM local_messages
             WHERE account = ?1 AND folder = ?2",
        );
        let patterns: Vec<String> = words
            .iter()
            .map(|w| {
                format!(
                    "%{}%",
                    w.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
                )
            })
            .collect();
        for i in 0..patterns.len() {
            let n = i + 4;
            sql.push_str(&format!(
                " AND (sender LIKE ?{n} ESCAPE '\\' OR subject LIKE ?{n} ESCAPE '\\' OR text LIKE ?{n} ESCAPE '\\')"
            ));
        }
        sql.push_str(" ORDER BY ts DESC, id DESC LIMIT ?3");
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![&account, &folder, &limit];
        values.extend(patterns.iter().map(|p| p as &dyn rusqlite::ToSql));
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(values.as_slice(), row_to_header)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn raw(&self, account: &str, id: u32) -> Result<Option<Vec<u8>>> {
        Ok(self
            .lock()
            .query_row(
                "SELECT raw FROM local_messages WHERE account = ?1 AND id = ?2",
                params![account, id],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn unseen(&self, account: &str, folder: &str) -> Result<u32> {
        Ok(self.lock().query_row(
            "SELECT COUNT(*) FROM local_messages WHERE account = ?1 AND folder = ?2 AND seen = 0",
            params![account, folder],
            |r| r.get(0),
        )?)
    }

    pub fn set_flagged(&self, account: &str, id: u32, flagged: bool) -> Result<()> {
        self.lock().execute(
            "UPDATE local_messages SET flagged = ?3 WHERE account = ?1 AND id = ?2",
            params![account, id, flagged],
        )?;
        Ok(())
    }

    pub fn set_seen(&self, account: &str, id: u32, seen: bool) -> Result<()> {
        self.lock().execute(
            "UPDATE local_messages SET seen = ?3 WHERE account = ?1 AND id = ?2",
            params![account, id, seen],
        )?;
        Ok(())
    }

    pub fn move_to(&self, account: &str, id: u32, folder: &str) -> Result<()> {
        self.lock().execute(
            "UPDATE local_messages SET folder = ?3 WHERE account = ?1 AND id = ?2",
            params![account, id, folder],
        )?;
        Ok(())
    }

    pub fn delete(&self, account: &str, id: u32) -> Result<()> {
        self.lock().execute(
            "DELETE FROM local_messages WHERE account = ?1 AND id = ?2",
            params![account, id],
        )?;
        Ok(())
    }
}

fn insert(conn: &Connection, account: &str, folder: &str, raw: &[u8], seen: bool) -> Result<u32> {
    let h = crate::mail::header_from_raw(0, raw, seen);
    // Messages without a Date header sort as "now".
    let ts = if h.timestamp > 0 { h.timestamp } else { now() };
    conn.execute(
        "INSERT INTO local_messages (account, folder, sender, subject, preview, ts, seen, attach, text, raw, msgid, parents, size, from_addr)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            account,
            folder,
            h.from,
            h.subject,
            h.preview,
            ts,
            seen,
            h.has_attachment,
            h.text,
            raw,
            h.message_id,
            h.parents.join(" "),
            h.size,
            h.from_address
        ],
    )?;
    u32::try_from(conn.last_insert_rowid()).context("local mailbox is full")
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
        from_address: r.get(11)?,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &[u8] = b"From: Ada <ada@example.org>\r\nSubject: Engines\r\nDate: Mon, 2 Nov 2026 10:00:00 +0000\r\n\r\nAbout the analytical engine.\r\n";

    #[test]
    fn stores_moves_searches_and_deletes() {
        let s = Store::in_memory();
        let id = s.add_downloaded("acc", "uidl-1", RAW).unwrap();
        assert!(s.downloaded("acc").unwrap().contains_key("uidl-1"));
        assert_eq!(s.unseen("acc", INBOX).unwrap(), 1);

        let list = s.headers("acc", INBOX, 10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].subject, "Engines");
        assert_eq!(list[0].from_address, "ada@example.org");
        assert_eq!(
            s.search("acc", INBOX, &["analytical".into()], 10).unwrap().len(),
            1
        );
        assert_eq!(s.search("acc", INBOX, &["100%".into()], 10).unwrap().len(), 0);

        s.set_seen("acc", id, true).unwrap();
        assert_eq!(s.unseen("acc", INBOX).unwrap(), 0);
        s.set_flagged("acc", id, true).unwrap();
        assert!(s.headers("acc", INBOX, 10).unwrap()[0].flagged);
        s.move_to("acc", id, ARCHIVE).unwrap();
        assert!(s.headers("acc", INBOX, 10).unwrap().is_empty());
        assert_eq!(s.raw("acc", id).unwrap().as_deref(), Some(RAW));
        assert_eq!(s.raw("other", id).unwrap(), None);

        s.delete("acc", id).unwrap();
        assert_eq!(s.raw("acc", id).unwrap(), None);
        // Still known as downloaded: not fetched again.
        assert!(s.downloaded("acc").unwrap().contains_key("uidl-1"));
        s.forget_downloaded("acc", &["uidl-1".into()]).unwrap();
        assert!(s.downloaded("acc").unwrap().is_empty());
    }

    #[test]
    fn sender_addresses_are_filled_in_for_older_mailboxes() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO local_messages (account, folder, sender, subject, preview, ts, seen, attach, text, raw)
             VALUES ('acc', ?1, 'Ada', 'Engines', '', 1, 0, 0, '', ?2)",
            params![INBOX, RAW],
        )
        .unwrap();
        let s = Store::with_connection(conn).unwrap();
        assert_eq!(s.headers("acc", INBOX, 10).unwrap()[0].from_address, "ada@example.org");
    }
}
