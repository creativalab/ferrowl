//! Bridge between the UI (async, iced) and IMAP/SMTP (blocking).
//! A single shared IMAP session, guarded by a mutex; if the connection
//! drops, the session is recreated and the operation retried once.

use crate::cache;
use crate::config::Account;
use crate::i18n::{generic_error, t};
use crate::local;
use crate::mail::{self, Folder, FolderKind, FullMessage, Header, ImapSession, Outgoing};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Default)]
struct Inner {
    account: Option<Account>,
    session: Option<ImapSession>,
    /// The server is Gmail: messages carry labels (X-GM-EXT-1).
    gmail: bool,
    /// Gmail: the user's labels (raw names of the label folders).
    gmail_labels: Vec<String>,
}

#[derive(Clone, Default)]
pub struct Backend {
    inner: Arc<Mutex<Inner>>,
    /// The same as `Inner::gmail`, readable without the lock: server
    /// commands hold the lock for their whole run (seconds for the unread
    /// counts of every folder), and the apps ask this while drawing.
    gmail: Arc<AtomicBool>,
}

/// Reloads the calendars from the server after a change (errors only
/// mean the local copy catches up at the next sync).
fn refresh_calendar(account: &Account, cache: &cache::Cache, key: &str) {
    let result = cache.event_etags(key).and_then(|known| {
        let changes = crate::dav::Client::new(account, crate::dav::Kind::Calendar).sync(account, &known)?;
        cache.apply_events(key, &changes)
    });
    if let Err(e) = result {
        eprintln!("calendar refresh: {e:#}");
    }
}

/// One occurrence of an event, with its calendar's name and color.
#[derive(Debug, Clone, PartialEq)]
pub struct AgendaEntry {
    pub occurrence: crate::ical::Occurrence,
    /// The stored event (item URL) this occurrence comes from.
    pub url: String,
    pub calendar_url: String,
    pub calendar: String,
    /// "#RRGGBB" or "#RRGGBBAA" (empty when the server sets none).
    pub color: String,
}

/// Error as text: iced messages must be `Clone`.
pub type Res<T> = Result<T, String>;

/// The error in words for the user (see [`crate::errors::humanize`]).
fn text(e: anyhow::Error) -> String {
    crate::errors::humanize(&format!("{e:#}"))
}

const MAX_THROTTLE_RETRIES: u32 = 3;

/// If the server refused because of rate limits, how long to wait.
///
/// Exchange Online: "Request is throttled. Suggested Backoff Time: N
/// milliseconds"; others use [UNAVAILABLE], "too many connections", etc.
fn throttle_delay(e: &anyhow::Error) -> Option<std::time::Duration> {
    use std::time::Duration;
    let msg = format!("{e:#}").to_lowercase();
    let throttled = [
        "throttl",
        "[unavailable]",
        "too many",
        "rate limit",
        "try again later",
    ]
    .iter()
    .any(|k| msg.contains(k));
    if !throttled {
        return None;
    }
    let suggested = msg
        .split("backoff time:")
        .nth(1)
        .and_then(|rest| {
            rest.split_whitespace()
                .next()?
                .trim_end_matches('.')
                .parse::<u64>()
                .ok()
        })
        .map(Duration::from_millis);
    Some(
        suggested
            .unwrap_or(Duration::from_secs(5))
            .clamp(Duration::from_millis(500), Duration::from_secs(30)),
    )
}

fn is_connection_error(e: &anyhow::Error) -> bool {
    e.chain().any(|c| {
        c.downcast_ref::<imap::Error>()
            .is_some_and(|ie| matches!(ie, imap::Error::Io(_) | imap::Error::ConnectionLost))
            || c.downcast_ref::<std::io::Error>().is_some()
    })
}

/// Whether the session is still in step with the server after `e`: a
/// refusal (NO/BAD) ends the command cleanly; anything else (a response
/// the IMAP library could not parse, an unexpected one) may leave part of
/// the answer unread, and the next command would read it instead of its
/// own (the library then panics on the mismatched tag: seen with Outlook).
fn session_still_usable(e: &anyhow::Error) -> bool {
    !e.chain().any(|c| {
        c.downcast_ref::<imap::Error>()
            .is_some_and(|ie| !matches!(ie, imap::Error::No(_) | imap::Error::Bad(_)))
    })
}

/// Runs an operation on a session, turning a panic of the IMAP library
/// (out of step with the server) into an error.
fn guarded<T>(
    session: &mut ImapSession,
    op: &impl Fn(&mut ImapSession) -> anyhow::Result<T>,
) -> Option<anyhow::Result<T>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| op(session))).ok()
}

impl Backend {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The connected account's address (empty if none).
    fn own_address(&self) -> String {
        self.lock()
            .account
            .as_ref()
            .map(|a| a.email.clone())
            .unwrap_or_default()
    }

    /// Address suggestions for To/Cc from the account's mail.
    pub fn suggest(account: &Account, query: &str) -> Vec<(String, String)> {
        cache::get()
            .and_then(|c| c.suggest(&cache::account_key(account), query, 8).ok())
            .unwrap_or_default()
    }

    /// Saved contacts matching `query` (all when empty), by name.
    pub fn contacts(account: &Account, query: &str) -> Vec<crate::vcard::Contact> {
        cache::get()
            .and_then(|c| c.contacts(&cache::account_key(account), query).ok())
            .unwrap_or_default()
    }

    /// Saves a contact written in the app: a new one (empty `url`) on the
    /// account's contacts server when it has one, otherwise on this device;
    /// a stored one where it is. Returns where it is now.
    pub async fn save_contact(account: Account, mut contact: crate::vcard::Contact) -> Res<String> {
        tokio::task::spawn_blocking(move || {
            let cache = cache::get().ok_or_else(generic_error)?;
            let key = cache::account_key(&account);
            let stored = if contact.url.is_empty() {
                None
            } else {
                cache.contact_place(&key, &contact.url).map_err(text)?
            };
            // A stored contact keeps its vCard UID (its identity on the server).
            if let Some((_, _, uid)) = &stored
                && contact.uid.is_empty()
            {
                contact.uid = uid.clone();
            }
            if contact.uid.is_empty() {
                contact.uid = format!("{}@ferrowl", crate::events::uuid_like());
            }
            if contact.name.trim().is_empty() {
                contact.name = contact.emails.first().cloned().unwrap_or_default();
            }
            // A stored contact changes only what the form shows: the rest of
            // its card (address, birthday, photo...) is kept.
            let original = match &stored {
                Some(_) => cache.contact_vcard(&key, &contact.url).map_err(text)?,
                None => None,
            };
            let vcard = match original.filter(|v| !v.trim().is_empty()) {
                Some(original) => crate::vcard::merge(&original, &contact),
                None => crate::vcard::write(&contact),
            };
            let (url, book) = if contact.url.is_empty() {
                let books = cache.address_books(&key).unwrap_or_default();
                match books.first() {
                    Some(book) => {
                        let id = crate::events::uuid_like();
                        let url = crate::carddav::create(&account, book, &id, &vcard).map_err(text)?;
                        (url, book.clone())
                    }
                    None => (
                        format!("local:{}", crate::events::uuid_like()),
                        cache::LOCAL_BOOK.to_string(),
                    ),
                }
            } else {
                let (book, etag) = stored
                    .map(|(book, etag, _)| (book, etag))
                    .unwrap_or_else(|| (cache::LOCAL_BOOK.to_string(), String::new()));
                if book != cache::LOCAL_BOOK {
                    crate::dav::Client::new(&account, crate::dav::Kind::Contacts)
                        .put(&contact.url, &vcard, "text/vcard; charset=utf-8", Some(&etag))
                        .map_err(text)?;
                }
                (contact.url.clone(), book)
            };
            cache
                .add_contact(&key, &url, &book, &contact, &vcard)
                .map_err(text)?;
            Ok(url)
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// Deletes a contact, from the contacts server too when it is there.
    pub async fn delete_contact(account: Account, url: String) -> Res<()> {
        tokio::task::spawn_blocking(move || {
            let cache = cache::get().ok_or_else(generic_error)?;
            let key = cache::account_key(&account);
            if let Some((book, etag, _)) = cache.contact_place(&key, &url).map_err(text)?
                && book != cache::LOCAL_BOOK
            {
                let etag = Some(etag.as_str()).filter(|e| !e.is_empty());
                crate::dav::Client::new(&account, crate::dav::Kind::Contacts)
                    .delete(&url, etag)
                    .map_err(text)?;
            }
            cache.delete_contact(&key, &url).map_err(text)
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// Brings the cached contacts up to date with the account's CardDAV
    /// server. Returns how many contacts are saved.
    pub async fn sync_contacts(account: Account) -> Res<usize> {
        tokio::task::spawn_blocking(move || {
            let cache = cache::get().ok_or_else(generic_error)?;
            let key = cache::account_key(&account);
            let known = cache.contact_etags(&key).map_err(text)?;
            let changes = crate::carddav::sync(&account, &known).map_err(text)?;
            cache.apply_contacts(&key, &changes).map_err(text)?;
            Ok(cache.contacts(&key, "").map_err(text)?.len())
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// Brings the cached calendars up to date with the account's CalDAV
    /// server. Returns how many calendars there are.
    pub async fn sync_calendar(account: Account) -> Res<usize> {
        tokio::task::spawn_blocking(move || {
            let cache = cache::get().ok_or_else(generic_error)?;
            let key = cache::account_key(&account);
            let known = cache.event_etags(&key).map_err(text)?;
            let changes = crate::dav::Client::new(&account, crate::dav::Kind::Calendar)
                .sync(&account, &known)
                .map_err(text)?;
            cache.apply_events(&key, &changes).map_err(text)?;
            Ok(changes.collections.len())
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// Event occurrences overlapping `[from, to)` (timestamps), sorted, from
    /// the cached calendars.
    pub fn agenda(account: &Account, from: i64, to: i64) -> Vec<AgendaEntry> {
        let Some(cache) = cache::get() else {
            return Vec::new();
        };
        let key = cache::account_key(account);
        let mut calendars = cache.calendars(&key).unwrap_or_default();
        calendars.extend(
            Self::calendars(account)
                .into_iter()
                .filter(|c| c.url == cache::LOCAL_CALENDAR),
        );
        cache
            .occurrences(&key, from, to)
            .unwrap_or_default()
            .into_iter()
            .map(|(calendar_url, url, occurrence)| {
                let cal = calendars.iter().find(|c| c.url == calendar_url);
                AgendaEntry {
                    url,
                    calendar_url,
                    calendar: cal.map(|c| c.name.clone()).unwrap_or_default(),
                    color: cal.map(|c| c.color.clone()).unwrap_or_default(),
                    occurrence,
                }
            })
            .collect()
    }

    /// The account's calendars (from the cache).
    /// The account's calendars; without a calendar server, the one kept on
    /// this device.
    pub fn calendars(account: &Account) -> Vec<crate::dav::Collection> {
        let local = crate::dav::Collection {
            url: cache::LOCAL_CALENDAR.to_string(),
            name: t("calendar_local").to_string(),
            color: String::new(),
        };
        let Some(c) = cache::get() else {
            return vec![local];
        };
        let key = cache::account_key(account);
        let mut calendars = c.calendars(&key).unwrap_or_default();
        // Without a calendar server, or with events made on the phone before
        // the account had one: the calendar on this device is listed too.
        if calendars.is_empty() || c.has_local_events(&key).unwrap_or(false) {
            calendars.push(local);
        }
        calendars
    }

    /// The form for a stored event (by item URL).
    pub fn event_draft(account: &Account, url: &str) -> Option<crate::events::Draft> {
        let (_, _, ics) = cache::get()?.event(&cache::account_key(account), url).ok()??;
        crate::events::Draft::from_ics(&ics)
    }

    /// Saves an event: changes the stored one at `url`, or creates a new
    /// one in `calendar` (its URL; empty = the first calendar). Returns the
    /// text to show.
    pub async fn save_event(
        account: Account,
        calendar: String,
        url: Option<String>,
        draft: crate::events::Draft,
    ) -> Res<String> {
        if !draft.is_valid() {
            return Err(t("event_bad_time").to_string());
        }
        tokio::task::spawn_blocking(move || {
            let cache = cache::get().ok_or_else(generic_error)?;
            let key = cache::account_key(&account);
            // The calendar on this device: no server involved.
            let local = match &url {
                Some(url) => url.starts_with("local:"),
                None => calendar == cache::LOCAL_CALENDAR || cache.calendars(&key).map_err(text)?.is_empty(),
            };
            if local {
                let (url, data) = match url {
                    Some(url) => {
                        let (_, _, ics) = cache
                            .event(&key, &url)
                            .map_err(text)?
                            .ok_or_else(|| t("err_event_changed").to_string())?;
                        (url, crate::events::update(&ics, &draft))
                    }
                    None => {
                        let (uid, data) = crate::events::create(&draft);
                        (format!("local:{uid}"), data)
                    }
                };
                cache.put_local_event(&key, &url, &data).map_err(text)?;
                return Ok(t("event_saved").to_string());
            }
            let client = crate::dav::Client::new(&account, crate::dav::Kind::Calendar);
            const ICS: &str = "text/calendar; charset=utf-8";
            match url {
                Some(url) => {
                    let (_, etag, ics) = cache
                        .event(&key, &url)
                        .map_err(text)?
                        .ok_or_else(|| t("err_event_changed").to_string())?;
                    let data = crate::events::update(&ics, &draft);
                    client.put(&url, &data, ICS, Some(&etag)).map_err(text)?;
                }
                None => {
                    let calendars = cache.calendars(&key).map_err(text)?;
                    let cal = calendars
                        .iter()
                        .find(|c| c.url == calendar)
                        .or(calendars.first())
                        .ok_or_else(|| t("err_no_calendar").to_string())?;
                    let (uid, data) = crate::events::create(&draft);
                    client
                        .put(&crate::invite::item_url(&cal.url, &uid), &data, ICS, None)
                        .map_err(text)?;
                }
            }
            refresh_calendar(&account, cache, &key);
            Ok(t("event_saved").to_string())
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// Deletes a stored event (a series with all its occurrences).
    pub async fn delete_event(account: Account, url: String) -> Res<String> {
        tokio::task::spawn_blocking(move || {
            let cache = cache::get().ok_or_else(generic_error)?;
            let key = cache::account_key(&account);
            if url.starts_with("local:") {
                cache.delete_event(&key, &url).map_err(text)?;
                return Ok(t("event_deleted").to_string());
            }
            let etag = cache.event(&key, &url).map_err(text)?.map(|(_, etag, _)| etag);
            crate::dav::Client::new(&account, crate::dav::Kind::Calendar)
                .delete(&url, etag.as_deref())
                .map_err(text)?;
            let changes = crate::dav::Changes {
                collections: cache.calendars(&key).map_err(text)?,
                gone: vec![url],
                ..Default::default()
            };
            cache.apply_events(&key, &changes).map_err(text)?;
            Ok(t("event_deleted").to_string())
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// Our earlier answer to an invitation (by event UID), if any.
    pub fn invitation_reply(account: &Account, uid: &str) -> Option<crate::invite::Reply> {
        cache::get()
            .and_then(|c| c.invite_reply(&cache::account_key(account), uid).ok().flatten())
            .and_then(|p| crate::invite::Reply::from_partstat(&p))
    }

    /// Whether the event is in the cached calendar (to offer removing it
    /// when cancelled).
    pub fn in_calendar(account: &Account, uid: &str) -> bool {
        cache::get()
            .and_then(|c| c.event_by_uid(&cache::account_key(account), uid).ok().flatten())
            .is_some()
    }

    /// Answers an invitation: mails the reply to the organizer (copy in
    /// Sent), remembers it, and stores the event in the calendar (removes
    /// it when declined) if the account has a calendar server. Returns the
    /// text to show.
    pub async fn respond_invitation(
        &self,
        ics: String,
        reply: crate::invite::Reply,
        sent_folder: Option<String>,
    ) -> Res<String> {
        let account = self
            .lock()
            .account
            .clone()
            .ok_or_else(|| t("err_no_account").to_string())?;
        let mail = crate::invite::reply_mail(&account, &ics, reply).map_err(text)?;
        let warning = self.send(mail, sent_folder).await?;
        let uid = crate::invite::read(&ics).map(|i| i.uid).unwrap_or_default();
        self.cache(|c, key| c.note_invite_reply(key, &uid, reply.partstat()));
        let saved = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let cache = cache::get().ok_or_else(|| anyhow::anyhow!(generic_error()))?;
            let key = cache::account_key(&account);
            let existing = cache.event_by_uid(&key, &uid)?;
            // No calendar server: the calendar on this device keeps it.
            let local = existing
                .as_ref()
                .is_some_and(|(url, _)| url.starts_with("local:"))
                || cache.calendars(&key)?.is_empty();
            if local {
                let url = existing
                    .map(|(url, _)| url)
                    .unwrap_or_else(|| format!("local:{uid}"));
                if reply == crate::invite::Reply::Declined {
                    cache.delete_event(&key, &url)?;
                } else {
                    let data = crate::invite::calendar_copy(&ics, &account.email, reply);
                    cache.put_local_event(&key, &url, &data)?;
                }
                return Ok(());
            }
            crate::invite::save_to_calendar(&account, &ics, reply, existing)?;
            // Refresh the local copy so the calendar shows the change.
            let known = cache.event_etags(&key)?;
            let changes =
                crate::dav::Client::new(&account, crate::dav::Kind::Calendar).sync(&account, &known)?;
            cache.apply_events(&key, &changes)
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?;
        let mut notice = match saved {
            Ok(()) if reply != crate::invite::Reply::Declined => t("invite_saved").to_string(),
            Ok(()) => t("invite_sent").to_string(),
            // No calendar server: the reply is all there is to do.
            Err(e) if e.to_string() == t("err_caldav_none") => t("invite_sent").to_string(),
            Err(e) => crate::i18n::tf("invite_not_saved", &[("err", &text(e))]),
        };
        if let Some(w) = warning {
            notice = format!("{notice}\n{}", crate::i18n::tf("sent_no_copy", &[("err", &w)]));
        }
        Ok(notice)
    }

    /// Removes a cancelled event from the calendar.
    pub async fn remove_cancelled(account: Account, uid: String) -> Res<String> {
        tokio::task::spawn_blocking(move || {
            let cache = cache::get().ok_or_else(generic_error)?;
            let key = cache::account_key(&account);
            if let Some((url, etag)) = cache.event_by_uid(&key, &uid).map_err(text)? {
                if url.starts_with("local:") {
                    cache.delete_event(&key, &url).map_err(text)?;
                    return Ok(t("invite_removed").to_string());
                }
                crate::invite::remove_from_calendar(&account, &url, &etag).map_err(text)?;
                let changes = crate::dav::Changes {
                    collections: cache.calendars(&key).map_err(text)?,
                    gone: vec![url],
                    ..Default::default()
                };
                cache.apply_events(&key, &changes).map_err(text)?;
            }
            Ok(t("invite_removed").to_string())
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// The connected account, if it reads mail with POP3.
    fn pop3_account(&self) -> Option<Account> {
        self.lock().account.clone().filter(Account::is_pop3)
    }

    async fn blocking<T, F>(&self, f: F) -> Res<T>
    where
        T: Send + 'static,
        F: FnOnce(&Backend) -> Res<T> + Send + 'static,
    {
        let me = self.clone();
        // An error here means the thread panicked: internal error.
        tokio::task::spawn_blocking(move || f(&me))
            .await
            .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    fn with_session_sync<T>(&self, op: impl Fn(&mut ImapSession) -> anyhow::Result<T>) -> Res<T> {
        let mut g = self.lock();
        let account = g.account.clone().ok_or_else(|| t("err_no_account").to_string())?;
        let mut attempt = 0u32;
        loop {
            let result: anyhow::Result<T> = (|| {
                if let Some(s) = g.session.as_mut() {
                    match guarded(s, &op) {
                        Some(Ok(v)) => return Ok(v),
                        Some(Err(e)) if !is_connection_error(&e) => {
                            // Out of step with the server: the next command
                            // starts on a new connection.
                            if !session_still_usable(&e) {
                                // Development builds: which answer it was.
                                if cfg!(debug_assertions) {
                                    eprintln!("imap: new connection after: {e:#}");
                                }
                                g.session = None;
                            }
                            return Err(e);
                        }
                        // Connection lost, or the library panicked: once more
                        // on a new connection.
                        _ => g.session = None,
                    }
                }
                let mut session = mail::connect(&account)?;
                match guarded(&mut session, &op) {
                    Some(Ok(v)) => {
                        g.session = Some(session);
                        Ok(v)
                    }
                    Some(Err(e)) => {
                        if session_still_usable(&e) {
                            g.session = Some(session);
                        }
                        Err(e)
                    }
                    None => Err(anyhow::anyhow!("{}", t("err_generic"))),
                }
            })();
            match result {
                Ok(v) => return Ok(v),
                // The server asked to slow down: wait and try again a few times.
                Err(e) => match throttle_delay(&e) {
                    Some(delay) if attempt < MAX_THROTTLE_RETRIES => {
                        attempt += 1;
                        std::thread::sleep(delay * attempt);
                    }
                    _ => return Err(text(e)),
                },
            }
        }
    }

    /// Connects an account (replaces any previous session).
    pub async fn connect(&self, account: Account) -> Res<Vec<Folder>> {
        self.blocking(move |b| {
            if account.is_pop3() {
                // Checks the login; the mailbox itself is local.
                crate::pop3::Pop3::connect(&account)
                    .and_then(|p| p.quit())
                    .map_err(text)?;
                let mut g = b.lock();
                if let Some(mut old) = g.session.take() {
                    let _ = old.logout();
                }
                g.gmail = false;
                b.gmail.store(false, Ordering::Relaxed);
                g.account = Some(account);
                return Ok(local::folders());
            }
            let mut session = mail::connect(&account).map_err(text)?;
            let folders = mail::list_folders(&mut session).map_err(text)?;
            let gmail = mail::has_gmail_labels(&mut session);
            let mut g = b.lock();
            g.gmail = gmail;
            b.gmail.store(gmail, Ordering::Relaxed);
            g.gmail_labels = if gmail {
                folders
                    .iter()
                    .filter(|f| {
                        f.kind == FolderKind::Other
                            && f.selectable
                            && !f.name.starts_with("[Gmail]")
                            && !f.name.starts_with("[Google Mail]")
                    })
                    .map(|f| f.name.clone())
                    .collect()
            } else {
                Vec::new()
            };
            if let Some(mut old) = g.session.take() {
                let _ = old.logout();
            }
            g.account = Some(account);
            g.session = Some(session);
            Ok(folders)
        })
        .await
    }

    /// Tests the credentials without touching the current session.
    /// Returns the message to show.
    pub async fn test(account: Account) -> Res<String> {
        tokio::task::spawn_blocking(move || {
            if account.is_pop3() {
                let mut p = crate::pop3::Pop3::connect(&account).map_err(text)?;
                let n = p.uidl().map_err(text)?.len();
                let _ = p.quit();
                return Ok(crate::i18n::tf("test_ok_pop3", &[("n", &n.to_string())]));
            }
            let mut s = mail::connect(&account).map_err(text)?;
            let n = mail::list_folders(&mut s).map_err(text)?.len();
            let _ = s.logout();
            Ok(crate::i18n::tf("test_ok", &[("n", &n.to_string())]))
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// A whole message of any account, on a connection of its own and
    /// without marking it read (other apps reading it). Not for POP3.
    pub async fn peek_message(account: Account, folder: String, uid: u32) -> Res<FullMessage> {
        tokio::task::spawn_blocking(move || {
            if account.is_pop3() {
                return Err(t("err_no_account").to_string());
            }
            let own = crate::crypto::own_key(&account.email).map(|k| (k, account.pgp_passphrase.clone()));
            let keys = mail::PgpKeys {
                own,
                contact: &crate::crypto::contact_key,
            };
            let mut s = mail::connect(&account).map_err(text)?;
            let msg = mail::peek_message(&mut s, &folder, uid, &keys).map_err(text);
            let _ = s.logout();
            msg
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// What a new-mail notification shows (see [`mail::peek_inbox`]), on a
    /// connection of its own, without touching the current session or the
    /// cache. Not for POP3 accounts.
    pub async fn peek(account: Account, after: u32, limit: u32) -> Res<mail::Peek> {
        tokio::task::spawn_blocking(move || {
            if account.is_pop3() {
                return Err(t("err_no_account").to_string());
            }
            let mut s = mail::connect(&account).map_err(text)?;
            let peek = mail::peek_inbox(&mut s, after, limit).map_err(text);
            let _ = s.logout();
            peek
        })
        .await
        .map_err(|e| crate::errors::humanize(&format!("{e:#}")))?
    }

    /// Cache key of the connected account.
    fn key(&self) -> Option<String> {
        self.lock().account.as_ref().map(cache::account_key)
    }

    /// Runs a cache update; failures only disable that update (the cache is
    /// an optimization, the server is the source of truth).
    fn cache(&self, f: impl FnOnce(&cache::Cache, &str) -> anyhow::Result<()>) {
        if let (Some(c), Some(key)) = (cache::get(), self.key())
            && let Err(e) = f(c, &key)
        {
            eprintln!("cache: {e:#}");
        }
    }

    /// Newest page of a folder; also refreshes the cache. Returns the
    /// sequence number to continue indexing from (see `index_older`).
    pub async fn headers(&self, folder: String) -> Res<(Vec<Header>, u32)> {
        self.blocking(move |b| {
            if let Some(account) = b.pop3_account() {
                // Newest messages first; the rest continues in `index_older`.
                let left = pop3_sync(&account, POP3_FIRST_BATCH).map_err(text)?;
                return Ok((local_headers(&account, &folder), if left > 0 { 2 } else { 0 }));
            }
            // Already cached: only what changed (much faster than the page).
            if let Some(out) = b.headers_delta(&folder)? {
                return Ok(out);
            }
            let mut page = b.with_session_sync(|s| mail::fetch_headers(s, &folder))?;
            // Gmail: the labels of the listed messages and, in the inbox,
            // their tab (Primary, Promotions, Social…).
            let (gmail, names) = {
                let g = b.lock();
                (g.gmail, g.gmail_labels.clone())
            };
            if gmail {
                let uids: Vec<u32> = page.headers.iter().map(|h| h.uid).collect();
                if let Ok(mut labels) = b.with_session_sync(|s| mail::fetch_labels(s, &folder, &uids, &names))
                {
                    for h in page.headers.iter_mut() {
                        h.labels = labels.remove(&h.uid).unwrap_or_default();
                    }
                }
                if folder.eq_ignore_ascii_case("INBOX")
                    && let Ok(cats) = b.with_session_sync(|s| mail::fetch_categories(s, &folder, &uids))
                {
                    for h in page.headers.iter_mut() {
                        h.category = cats.get(&h.uid).copied().unwrap_or_default();
                    }
                }
            }
            let own = b.own_address();
            b.cache(|c, key| {
                if let Some(v) = page.uidvalidity {
                    c.check_uidvalidity(key, &folder, v)?;
                }
                c.store_headers(key, &folder, &page.headers, &own)?;
                let uids: Vec<u32> = page.headers.iter().map(|h| h.uid).collect();
                if let Some(min) = uids.iter().min() {
                    c.prune(key, &folder, *min, &uids)?;
                }
                Ok(())
            });
            Ok((page.headers, page.first_seq))
        })
        .await
    }

    /// Refresh from the cache plus the server's changes: new messages are
    /// downloaded (with their Gmail labels and tab), the rest only gets its
    /// read/starred state and disappears if deleted. `None` = no usable
    /// cache, or too much changed: the caller fetches the whole page.
    fn headers_delta(&self, folder: &str) -> Res<Option<(Vec<Header>, u32)>> {
        let (Some(c), Some(key)) = (cache::get(), self.key()) else {
            return Ok(None);
        };
        let (validity, window) = match (
            c.uidvalidity(&key, folder),
            c.newest_uids(&key, folder, mail::PAGE_SIZE),
        ) {
            (Ok(Some(v)), Ok(w)) if !w.is_empty() => (v, w),
            _ => return Ok(None),
        };
        let known_max = window.iter().copied().max().unwrap_or(0);
        let window_min = window.iter().copied().min().unwrap_or(0);
        let Some(mut delta) =
            self.with_session_sync(|s| mail::fetch_delta(s, folder, known_max, window_min))?
        else {
            return Ok(None);
        };
        if delta.uidvalidity != Some(validity) {
            return Ok(None);
        }
        let (gmail, names) = {
            let g = self.lock();
            (g.gmail, g.gmail_labels.clone())
        };
        if gmail && !delta.new.is_empty() {
            let uids: Vec<u32> = delta.new.iter().map(|h| h.uid).collect();
            if let Ok(mut labels) = self.with_session_sync(|s| mail::fetch_labels(s, folder, &uids, &names)) {
                for h in delta.new.iter_mut() {
                    h.labels = labels.remove(&h.uid).unwrap_or_default();
                }
            }
            if folder.eq_ignore_ascii_case("INBOX")
                && let Ok(cats) = self.with_session_sync(|s| mail::fetch_categories(s, folder, &uids))
            {
                for h in delta.new.iter_mut() {
                    h.category = cats.get(&h.uid).copied().unwrap_or_default();
                }
            }
        }
        let own = self.own_address();
        let mut present: Vec<u32> = delta.flags.iter().map(|(u, _, _)| *u).collect();
        present.extend(delta.new.iter().map(|h| h.uid));
        c.store_headers(&key, folder, &delta.new, &own).map_err(text)?;
        c.set_flags_many(&key, folder, &delta.flags).map_err(text)?;
        c.prune(&key, folder, window_min, &present).map_err(text)?;
        let headers = c.headers(&key, folder, mail::PAGE_SIZE).map_err(text)?;
        let shown = headers.len() as u32;
        let first_seq = delta.exists.saturating_sub(shown.saturating_sub(1)).max(1);
        Ok(Some((headers, first_seq)))
    }

    /// Indexes up to `count` messages older than sequence `before` into the
    /// cache. Returns where to continue, or `None` when the folder is done
    /// (or the cache already holds as many messages as the server).
    pub async fn index_older(&self, folder: String, before: u32, count: u32) -> Res<Option<u32>> {
        self.blocking(move |b| {
            if let Some(account) = b.pop3_account() {
                let left = pop3_sync(&account, count as usize).map_err(text)?;
                return Ok((left > 0).then_some(2));
            }
            if before <= 1 {
                return Ok(None);
            }
            let page = b.with_session_sync(|s| mail::fetch_older(s, &folder, before, count))?;
            let own = b.own_address();
            let mut done = page.first_seq <= 1;
            b.cache(|c, key| {
                c.store_headers(key, &folder, &page.headers, &own)?;
                let (cached, _) = c.folder_stats(key, &folder)?;
                done |= cached >= page.exists;
                Ok(())
            });
            Ok((!done).then_some(page.first_seq))
        })
        .await
    }

    /// Cached list of a folder (instant, before the server answers).
    /// Up to `limit` messages of a folder from the local copy (which the
    /// background indexing fills with older messages), newest first.
    pub fn cached_headers_up_to(account: &Account, folder: &str, limit: u32) -> Vec<Header> {
        if account.is_pop3() {
            return local_headers(account, folder);
        }
        cache::get()
            .and_then(|c| c.headers(&cache::account_key(account), folder, limit).ok())
            .unwrap_or_default()
    }

    pub fn cached_headers(account: &Account, folder: &str) -> Vec<Header> {
        if account.is_pop3() {
            return local_headers(account, folder);
        }
        cache::get()
            .and_then(|c| {
                c.headers(&cache::account_key(account), folder, mail::PAGE_SIZE)
                    .ok()
            })
            .unwrap_or_default()
    }

    /// Full-text search in the cached messages of a folder.
    /// The search box ([crate::query]): free words through the full-text
    /// index, operators applied to the results; with operators only, the
    /// messages stored for the folder are filtered.
    pub fn search_query(account: &Account, folder: &str, text: &str) -> Vec<Header> {
        let q = crate::query::Query::parse(text);
        let found = if !q.words.is_empty() {
            Self::search(account, folder, &q.words)
        } else if q.has_filters() {
            Self::cached_headers_up_to(account, folder, 5000)
        } else {
            Vec::new()
        };
        found.into_iter().filter(|h| q.matches(h)).collect()
    }

    pub fn search(account: &Account, folder: &str, words: &[String]) -> Vec<Header> {
        if account.is_pop3() {
            return local::get()
                .and_then(|s| s.search(&cache::account_key(account), folder, words, 500).ok())
                .unwrap_or_default();
        }
        cache::get()
            .and_then(|c| c.search(&cache::account_key(account), folder, words, 500).ok())
            .unwrap_or_default()
    }

    pub async fn unseen(&self, folders: Vec<String>) -> Res<HashMap<String, u32>> {
        self.blocking(move |b| {
            if let Some(account) = b.pop3_account() {
                let key = cache::account_key(&account);
                let store = local::get().ok_or_else(generic_error)?;
                return Ok(folders
                    .into_iter()
                    .filter_map(|f| Some((f.clone(), store.unseen(&key, &f).ok()?)))
                    .collect());
            }
            let (counts, in_step) = b.with_session_sync(|s| Ok(mail::unseen_counts(s, &folders)))?;
            if !in_step {
                b.lock().session = None;
            }
            Ok(counts)
        })
        .await
    }

    pub async fn message(&self, folder: String, uid: u32) -> Res<FullMessage> {
        self.blocking(move |b| {
            let account = b.lock().account.clone();
            let own = account
                .as_ref()
                .and_then(|a| crate::crypto::own_key(&a.email).map(|k| (k, a.pgp_passphrase.clone())));
            let keys = mail::PgpKeys {
                own,
                contact: &crate::crypto::contact_key,
            };
            if let Some(account) = b.pop3_account() {
                let raw = local_op(&account, |s, key| s.raw(key, uid))?
                    .ok_or_else(|| t("err_not_found").to_string())?;
                let msg = mail::parse_full(uid, &raw, &keys).map_err(text)?;
                // Opening reads it, as an IMAP fetch of the body does.
                local_op(&account, |s, key| s.set_seen(key, uid, true))?;
                return Ok(msg);
            }
            let msg = b.with_session_sync(|s| mail::fetch_message(s, &folder, uid, &keys))?;
            b.cache(|c, key| c.store_body(key, &folder, uid, &msg.body));
            Ok(msg)
        })
        .await
    }

    pub async fn set_seen(&self, folder: String, uid: u32, seen: bool) -> Res<()> {
        self.set_seen_many(folder, vec![uid], seen).await
    }

    /// Marks several messages read or unread with one server command.
    pub async fn set_seen_many(&self, folder: String, uids: Vec<u32>, seen: bool) -> Res<()> {
        self.blocking(move |b| {
            if let Some(account) = b.pop3_account() {
                return local_op(&account, |s, key| {
                    uids.iter().try_for_each(|&u| s.set_seen(key, u, seen))
                });
            }
            b.with_session_sync(|s| mail::set_seen(s, &folder, &uids, seen))?;
            b.cache(|c, key| uids.iter().try_for_each(|&u| c.set_seen(key, &folder, u, seen)));
            Ok(())
        })
        .await
    }

    /// Whether the connected account is Gmail (messages have labels).
    /// Never waits for a server command in progress.
    pub fn is_gmail(&self) -> bool {
        self.gmail.load(Ordering::Relaxed)
    }

    /// Adds or removes a Gmail label (raw IMAP name) on messages.
    pub async fn set_label_many(&self, folder: String, uids: Vec<u32>, label: String, on: bool) -> Res<()> {
        self.blocking(move |b| {
            b.with_session_sync(|s| mail::set_gmail_label(s, &folder, &uids, &label, on))?;
            b.cache(|c, key| {
                uids.iter()
                    .try_for_each(|&u| c.set_label(key, &folder, u, &label, on))
            });
            Ok(())
        })
        .await
    }

    /// Stars or unstars messages (\\Flagged) with one server command.
    pub async fn set_flagged_many(&self, folder: String, uids: Vec<u32>, flagged: bool) -> Res<()> {
        self.blocking(move |b| {
            if let Some(account) = b.pop3_account() {
                return local_op(&account, |s, key| {
                    uids.iter().try_for_each(|&u| s.set_flagged(key, u, flagged))
                });
            }
            b.with_session_sync(|s| mail::set_flagged(s, &folder, &uids, flagged))?;
            b.cache(|c, key| {
                uids.iter()
                    .try_for_each(|&u| c.set_flagged(key, &folder, u, flagged))
            });
            Ok(())
        })
        .await
    }

    pub async fn move_to(&self, folder: String, uid: u32, target: String) -> Res<()> {
        self.move_many(folder, vec![uid], target).await
    }

    /// Moves several messages with one server command.
    pub async fn move_many(&self, folder: String, uids: Vec<u32>, target: String) -> Res<()> {
        self.blocking(move |b| {
            if let Some(account) = b.pop3_account() {
                return local_op(&account, |s, key| {
                    uids.iter().try_for_each(|&u| s.move_to(key, u, &target))
                });
            }
            b.with_session_sync(|s| mail::move_message(s, &folder, &uids, &target))?;
            b.cache(|c, key| uids.iter().try_for_each(|&u| c.remove(key, &folder, u)));
            Ok(())
        })
        .await
    }

    /// Archive: moves to the Archive folder, creating it if missing.
    /// Returns the name of the folder used.
    pub async fn archive(&self, folder: String, uid: u32, folders: Vec<Folder>) -> Res<String> {
        self.archive_many(folder, vec![uid], folders).await
    }

    pub async fn archive_many(&self, folder: String, uids: Vec<u32>, folders: Vec<Folder>) -> Res<String> {
        self.blocking(move |b| {
            if let Some(account) = b.pop3_account() {
                return local_op(&account, |s, key| {
                    uids.iter().try_for_each(|&u| s.move_to(key, u, local::ARCHIVE))
                })
                .map(|_| local::ARCHIVE.to_string());
            }
            b.with_session_sync(|s| {
                let target = match folders.iter().find(|f| f.kind == FolderKind::Archive) {
                    Some(f) => f.name.clone(),
                    None => mail::create_folder(s, &folders, "Archive")?,
                };
                mail::move_message(s, &folder, &uids, &target)?;
                Ok(target)
            })
            .inspect(|_| b.cache(|c, key| uids.iter().try_for_each(|&u| c.remove(key, &folder, u))))
        })
        .await
    }

    /// Moves messages to Spam (created if missing) and marks them `$Junk`
    /// for the server's filter. Returns the Spam folder's name.
    pub async fn report_spam(&self, folder: String, uids: Vec<u32>, folders: Vec<Folder>) -> Res<String> {
        self.junk(folder, uids, folders, true).await
    }

    /// Moves messages back from Spam to the inbox, marked `$NotJunk`.
    pub async fn not_spam(&self, folder: String, uids: Vec<u32>, folders: Vec<Folder>) -> Res<String> {
        self.junk(folder, uids, folders, false).await
    }

    async fn junk(&self, folder: String, uids: Vec<u32>, folders: Vec<Folder>, spam: bool) -> Res<String> {
        self.blocking(move |b| {
            if let Some(account) = b.pop3_account() {
                let to = if spam { local::SPAM } else { local::INBOX };
                return local_op(&account, |s, key| {
                    uids.iter().try_for_each(|&u| s.move_to(key, u, to))
                })
                .map(|_| to.to_string());
            }
            b.with_session_sync(|s| {
                let target = if spam {
                    match folders.iter().find(|f| f.kind == FolderKind::Spam) {
                        Some(f) => f.name.clone(),
                        None => mail::create_folder(s, &folders, "Spam")?,
                    }
                } else {
                    folders
                        .iter()
                        .find(|f| f.kind == FolderKind::Inbox)
                        .map_or_else(|| "INBOX".to_string(), |f| f.name.clone())
                };
                let (add, remove) = if spam {
                    (["$Junk"], ["$NotJunk"])
                } else {
                    (["$NotJunk"], ["$Junk"])
                };
                mail::set_keywords(s, &folder, &uids, &add, &remove);
                mail::move_message(s, &folder, &uids, &target)?;
                Ok(target)
            })
            .inspect(|_| b.cache(|c, key| uids.iter().try_for_each(|&u| c.remove(key, &folder, u))))
        })
        .await
    }

    /// Blocks (`allow` false) or allows a sender: an address, or "@domain".
    pub fn set_sender_rule(account: &Account, address: &str, allow: bool) -> Res<()> {
        cache::get()
            .ok_or_else(generic_error)?
            .set_sender_rule(&cache::account_key(account), address, allow)
            .map_err(text)
    }

    pub fn remove_sender_rule(account: &Account, address: &str) {
        if let Some(c) = cache::get() {
            let _ = c.remove_sender_rule(&cache::account_key(account), address);
        }
    }

    /// The blocked (`allow` false) or allowed senders.
    pub fn sender_rules(account: &Account, allow: bool) -> Vec<String> {
        cache::get()
            .and_then(|c| c.sender_rules(&cache::account_key(account), allow).ok())
            .unwrap_or_default()
    }

    /// Messages a sender list moves: from blocked senders (`to_spam`), or
    /// from allowed ones (out of Spam).
    pub fn ruled(account: &Account, headers: &[Header], to_spam: bool) -> Vec<u32> {
        let Some(c) = cache::get() else {
            return Vec::new();
        };
        let key = cache::account_key(account);
        headers
            .iter()
            .filter(|h| c.sender_rule(&key, &h.from_address).ok().flatten() == Some(!to_spam))
            .map(|h| h.uid)
            .collect()
    }

    /// Applies the sender lists to a freshly loaded folder: in the inbox,
    /// mail from blocked senders goes to Spam; in Spam, mail from allowed
    /// senders comes back to the inbox. Returns the moved messages (to
    /// take out of the list on screen).
    /// The account's rules (see [crate::rules]), in their order.
    pub fn rules(account: &Account) -> Vec<crate::rules::Rule> {
        cache::get()
            .and_then(|c| c.rules(&cache::account_key(account)).ok())
            .unwrap_or_default()
    }

    pub fn save_rule(account: &Account, rule: &crate::rules::Rule) -> Res<()> {
        cache::get()
            .ok_or_else(generic_error)?
            .save_rule(&cache::account_key(account), rule)
            .map_err(text)
    }

    pub fn delete_rule(account: &Account, id: &str) -> Res<()> {
        cache::get()
            .ok_or_else(generic_error)?
            .delete_rule(&cache::account_key(account), id)
            .map_err(text)
    }

    /// Which of `headers` an enabled rule keeps from being notified.
    pub fn silenced(account: &Account, headers: &[Header]) -> Vec<u32> {
        let rules = Self::rules(account);
        headers
            .iter()
            .filter(|h| crate::rules::silenced(&rules, h))
            .map(|h| h.uid)
            .collect()
    }

    /// The inbox messages on the device a condition would pick (newest
    /// first, at most `limit`): the preview of a rule being written.
    pub fn rule_preview(account: &Account, condition: &str, limit: usize) -> Vec<Header> {
        let rule = crate::rules::Rule {
            id: String::new(),
            name: String::new(),
            condition: condition.to_string(),
            actions: Vec::new(),
            enabled: true,
        };
        let mut found: Vec<Header> = Self::cached_headers_up_to(account, "INBOX", 500)
            .into_iter()
            .filter(|h| rule.matches(h))
            .collect();
        found.sort_by_key(|h| std::cmp::Reverse(h.timestamp));
        found.truncate(limit);
        found
    }

    /// Applies the rules to the inbox's new messages (each once; the first
    /// run only remembers where the inbox is). Returns what was done.
    pub async fn apply_rules(
        &self,
        folder: String,
        headers: Vec<Header>,
        folders: Vec<Folder>,
    ) -> Res<crate::rules::Plan> {
        let inbox = folders
            .iter()
            .any(|f| f.name == folder && f.kind == FolderKind::Inbox);
        let Some(account) = self.lock().account.clone() else {
            return Ok(Default::default());
        };
        let (Some(newest), true) = (headers.iter().map(|h| h.uid).max(), inbox) else {
            return Ok(Default::default());
        };
        let rules = Self::rules(&account);
        let Some(c) = cache::get() else {
            return Ok(Default::default());
        };
        let key = cache::account_key(&account);
        let done = c.rules_done(&key).ok().flatten();
        let fresh: Vec<Header> = match done {
            Some(done) if !rules.is_empty() => headers.into_iter().filter(|h| h.uid > done).collect(),
            _ => Vec::new(),
        };
        let plan = crate::rules::plan(&rules, &fresh);
        if !plan.mark_read.is_empty() {
            self.set_seen_many(folder.clone(), plan.mark_read.clone(), true)
                .await?;
        }
        if !plan.star.is_empty() {
            self.set_flagged_many(folder.clone(), plan.star.clone(), true)
                .await?;
        }
        if !plan.archive.is_empty() {
            self.archive_many(folder.clone(), plan.archive.clone(), folders.clone())
                .await?;
        }
        for (target, uids) in &plan.moves {
            self.move_many(folder.clone(), uids.clone(), target.clone())
                .await?;
        }
        if done.is_none_or(|d| newest > d) {
            let _ = c.set_rules_done(&key, newest);
        }
        Ok(plan)
    }

    pub async fn apply_sender_rules(
        &self,
        folder: String,
        headers: Vec<Header>,
        folders: Vec<Folder>,
    ) -> Res<Vec<u32>> {
        let kind = folders
            .iter()
            .find(|f| f.name == folder)
            .map_or(FolderKind::Other, |f| f.kind);
        let to_spam = match kind {
            FolderKind::Inbox => true,
            FolderKind::Spam => false,
            _ => return Ok(Vec::new()),
        };
        let Some(account) = self.lock().account.clone() else {
            return Ok(Vec::new());
        };
        let uids = Self::ruled(&account, &headers, to_spam);
        if uids.is_empty() {
            return Ok(uids);
        }
        self.junk(folder, uids.clone(), folders, to_spam).await?;
        Ok(uids)
    }

    pub async fn delete_forever(&self, folder: String, uid: u32) -> Res<()> {
        self.delete_forever_many(folder, vec![uid]).await
    }

    pub async fn delete_forever_many(&self, folder: String, uids: Vec<u32>) -> Res<()> {
        self.blocking(move |b| {
            if let Some(account) = b.pop3_account() {
                return local_op(&account, |s, key| uids.iter().try_for_each(|&u| s.delete(key, u)));
            }
            b.with_session_sync(|s| mail::delete_permanently(s, &folder, &uids))?;
            b.cache(|c, key| uids.iter().try_for_each(|&u| c.remove(key, &folder, u)));
            Ok(())
        })
        .await
    }

    /// Sends and saves a copy to Sent. `Ok(Some(err))` = sent, copy not saved.
    pub async fn send(&self, mail: Outgoing, sent_folder: Option<String>) -> Res<Option<String>> {
        self.blocking(move |b| {
            let account = b
                .lock()
                .account
                .clone()
                .ok_or_else(|| t("err_no_account").to_string())?;
            let raw = mail::send(&account, &mail).map_err(text)?;
            let to: Vec<(String, String)> = [&mail.to, &mail.cc, &mail.bcc]
                .iter()
                .flat_map(|f| mail::parse_recipients(f))
                .collect();
            b.cache(|c, key| c.note_sent(key, &to, chrono::Utc::now().timestamp()));
            // New recipients become contacts (may take a server round trip:
            // not while sending).
            let (who, recipients) = (account.clone(), to.clone());
            std::thread::spawn(move || save_new_contacts(&who, &recipients));
            if account.is_pop3() {
                // Sent copy in the local mailbox.
                return Ok(local_op(&account, |s, key| s.insert(key, local::SENT, &raw, true)).err());
            }
            let Some(folder) = sent_folder else {
                return Ok(None);
            };
            match b.with_session_sync(|s| mail::append_sent(s, &folder, &raw)) {
                Ok(()) => Ok(None),
                Err(e) => Ok(Some(e)),
            }
        })
        .await
    }
}

/// Saves the recipients that are not contacts yet: on the account's
/// contacts server (CardDAV) when it has one, otherwise on this device.
/// Failures only mean the contact is not saved: nothing to tell the user.
fn save_new_contacts(account: &Account, recipients: &[(String, String)]) {
    let Some(cache) = cache::get() else { return };
    let key = cache::account_key(account);
    let own = account.email.trim().to_lowercase();
    let books = cache.address_books(&key).unwrap_or_default();
    for (name, address) in recipients {
        let address = address.trim().to_lowercase();
        if !address.contains('@') || address == own || cache.has_contact_email(&key, &address).unwrap_or(true)
        {
            continue;
        }
        let id = crate::events::uuid_like();
        let name = if name.trim().is_empty() {
            address.clone()
        } else {
            name.trim().to_string()
        };
        let contact = crate::vcard::Contact {
            uid: format!("{id}@ferrowl"),
            name,
            emails: vec![address.clone()],
            ..Default::default()
        };
        let vcard = crate::vcard::build(&contact.uid, &contact.name, &address);
        let on_server =
            books
                .first()
                .and_then(|book| match crate::carddav::create(account, book, &id, &vcard) {
                    Ok(url) => Some((url, book.clone())),
                    Err(e) => {
                        eprintln!("contacts: saving {address} on the server failed: {e:#}");
                        None
                    }
                });
        let (url, book) = on_server.unwrap_or_else(|| (format!("local:{id}"), cache::LOCAL_BOOK.to_string()));
        if let Err(e) = cache.add_contact(&key, &url, &book, &contact, &vcard) {
            eprintln!("contacts: {e:#}");
        }
    }
}

// ---------------------------------------------------------------- POP3

/// Runs an operation on the local mailbox of a POP3 account.
fn local_op<T>(account: &Account, f: impl FnOnce(&local::Store, &str) -> anyhow::Result<T>) -> Res<T> {
    let store = local::get().ok_or_else(generic_error)?;
    f(store, &cache::account_key(account)).map_err(text)
}

fn local_headers(account: &Account, folder: &str) -> Vec<Header> {
    local_op(account, |s, key| s.headers(key, folder, 2000)).unwrap_or_default()
}

/// New POP3 messages downloaded before the list is shown.
const POP3_FIRST_BATCH: usize = 40;

/// Downloads up to `max` new messages (newest first) into the local inbox
/// and deletes from the server those kept longer than the account allows.
/// Returns how many new messages are still to download.
fn pop3_sync(account: &Account, max: usize) -> anyhow::Result<usize> {
    // POP3 servers lock the mailbox: one session at a time.
    static RUNNING: Mutex<()> = Mutex::new(());
    let _one = RUNNING.lock().unwrap_or_else(|p| p.into_inner());

    let store = local::get().ok_or_else(|| anyhow::anyhow!(generic_error()))?;
    let key = cache::account_key(account);
    let mut pop = crate::pop3::Pop3::connect(account)?;
    let listing = pop.uidl()?;
    let mut known = store.downloaded(&key)?;

    // Higher numbers are newer messages.
    let pending: Vec<&crate::pop3::Listing> = listing
        .iter()
        .rev()
        .filter(|l| !known.contains_key(&l.uidl))
        .collect();
    for item in pending.iter().take(max) {
        let raw = pop.retr(item.number)?;
        store.add_downloaded(&key, &item.uidl, &raw)?;
        if let Some(c) = cache::get() {
            let h = mail::header_from_raw(0, &raw, false);
            let _ = c.store_address_book(&key, &account.email, &h);
        }
        known.insert(item.uidl.clone(), local::now());
    }
    let left = pending.len().saturating_sub(max);

    if let Some(days) = account.pop3_keep_days {
        let limit = i64::from(days) * 86_400;
        let now = local::now();
        for item in &listing {
            if known.get(&item.uidl).is_some_and(|at| now - at >= limit) {
                pop.dele(item.number)?;
            }
        }
    }
    pop.quit()?;

    // Forget what is no longer on the server (deleted here or elsewhere).
    let on_server: std::collections::HashSet<&str> = listing.iter().map(|l| l.uidl.as_str()).collect();
    let gone: Vec<String> = known
        .into_keys()
        .filter(|u| !on_server.contains(u.as_str()))
        .collect();
    store.forget_downloaded(&key, &gone)?;
    Ok(left)
}

#[cfg(test)]
mod tests {
    use super::throttle_delay;
    use std::time::Duration;

    #[test]
    fn recognizes_throttling() {
        let e =
            anyhow::anyhow!("Bad Response: Request is throttled. Suggested Backoff Time: 2453 milliseconds");
        assert_eq!(throttle_delay(&e), Some(Duration::from_millis(2453)));
        let e = anyhow::anyhow!("NO [UNAVAILABLE] Server busy, try again later");
        assert_eq!(throttle_delay(&e), Some(Duration::from_secs(5)));
        let e = anyhow::anyhow!("Request is throttled. Suggested Backoff Time: 999999 milliseconds");
        assert_eq!(throttle_delay(&e), Some(Duration::from_secs(30)));
        assert_eq!(
            throttle_delay(&anyhow::anyhow!("NO [AUTHENTICATIONFAILED] Invalid credentials")),
            None
        );
    }
}
