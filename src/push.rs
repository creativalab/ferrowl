//! Push: a second IMAP connection kept in IDLE on the inbox. When the
//! server reports a change, the app checks for new mail right away instead
//! of waiting for the periodic refresh.
//!
//! Servers without IDLE are left to the periodic refresh.

use crate::app::Message;
use crate::config::Account;
use crate::mail;
use iced::futures::SinkExt;
use iced::{Subscription, futures::Stream};
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// IDLE is re-issued periodically (RFC 2177 asks for less than 29 minutes).
const IDLE_TIMEOUT: Duration = Duration::from_secs(9 * 60);
/// IDLE is waited on in steps this long, checking in between whether the
/// watch is still wanted: a dropped watch (account switched) logs out
/// within a step instead of keeping its connection for minutes, so quick
/// switching does not pile up connections.
const IDLE_STEP: Duration = Duration::from_secs(30);

/// Set when the subscription's stream is dropped: tells the blocking IDLE
/// wait to log out and stop.
struct StopOnDrop(Arc<AtomicBool>);

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Waits (up to `IDLE_TIMEOUT`) for a change in `folder`, in `IDLE_STEP`
/// steps. `None`: the watch was dropped (the session is logged out).
fn idle_until_change(
    session: &mut mail::ImapSession,
    folder: &str,
    stop: &AtomicBool,
) -> anyhow::Result<Option<bool>> {
    let started = Instant::now();
    // Mail arriving between two steps (while IDLE is re-issued) would not
    // be reported by the next one: a UID at or past the first unused one
    // tells it.
    let uid_next = session.select(folder)?.uid_next;
    loop {
        if stop.load(Ordering::SeqCst) {
            let _ = session.logout();
            return Ok(None);
        }
        if mail::idle_wait_selected(session, IDLE_STEP)? {
            return Ok(Some(true));
        }
        if let Some(next) = uid_next
            && session
                .uid_search(format!("UID {next}:*"))?
                .into_iter()
                .any(|u| u >= next)
        {
            return Ok(Some(true));
        }
        if started.elapsed() >= IDLE_TIMEOUT {
            return Ok(Some(false));
        }
    }
}

#[derive(Clone)]
pub struct Watch {
    pub account: Account,
    pub folder: String,
    pub generation: u64,
    /// An account other than the one in use: new mail is looked up here and
    /// reported with [`Message::OtherMail`] (for a desktop notification)
    /// instead of [`Message::Push`].
    pub other: bool,
}

impl Hash for Watch {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.account.username.hash(state);
        self.account.imap_host.hash(state);
        self.account.imap_port.hash(state);
        self.folder.hash(state);
        self.generation.hash(state);
        self.other.hash(state);
    }
}

pub fn subscription(watch: Watch) -> Subscription<Message> {
    Subscription::run_with(watch, stream)
}

fn stream(watch: &Watch) -> impl Stream<Item = Message> + use<> {
    let watch = watch.clone();
    iced::stream::channel(4, async move |mut out| {
        let stop = Arc::new(AtomicBool::new(false));
        let _stop_on_drop = StopOnDrop(stop.clone());
        let mut backoff = 15;
        loop {
            let account = watch.account.clone();
            let connected =
                tokio::task::spawn_blocking(move || -> anyhow::Result<Option<mail::ImapSession>> {
                    let mut session = mail::connect(&account)?;
                    if !mail::supports_idle(&mut session)? {
                        let _ = session.logout();
                        return Ok(None);
                    }
                    Ok(Some(session))
                })
                .await;
            let mut session = match connected {
                Ok(Ok(Some(s))) => s,
                // No IDLE on this server: nothing to do, the periodic refresh covers it.
                Ok(Ok(None)) => return,
                _ => {
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    backoff = (backoff * 2).min(300);
                    continue;
                }
            };
            // Other accounts: the highest UID already seen, so only mail
            // arrived after this point is reported.
            let mut seen_max = 0;
            // Other accounts: their unread count, for the dock icon.
            let mut unread = None;
            if watch.other {
                let folder = watch.folder.clone();
                match tokio::task::spawn_blocking(move || {
                    let r = session
                        .select(&folder)
                        .and_then(|m| match m.uid_next {
                            Some(next) => Ok(next.saturating_sub(1)),
                            None if m.exists == 0 => Ok(0),
                            // No UIDNEXT from the server: the last message's
                            // UID, or all the unread mail would look new.
                            None => Ok(session.uid_search("*")?.into_iter().max().unwrap_or(0)),
                        })
                        .and_then(|max| Ok((max, session.uid_search("UNSEEN")?.len() as u32)));
                    (session, r)
                })
                .await
                {
                    Ok((s, Ok((max, n)))) => {
                        session = s;
                        seen_max = max;
                        unread = Some(n);
                        if out
                            .send(Message::OtherUnread(watch.account.email.clone(), n))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    // Wait before logging in again, as for a failed connect.
                    _ => {
                        tokio::time::sleep(Duration::from_secs(backoff)).await;
                        backoff = (backoff * 2).min(300);
                        continue;
                    }
                }
            }
            backoff = 15;
            loop {
                let folder = watch.folder.clone();
                let other = watch.other;
                let stop = stop.clone();
                let waited = tokio::task::spawn_blocking(move || {
                    let r = idle_until_change(&mut session, &folder, &stop).and_then(|changed| {
                        // Dropped: nothing more to do.
                        let Some(changed) = changed else {
                            return Ok(None);
                        };
                        if !(changed && other) {
                            return Ok(Some((changed, Vec::new(), None)));
                        }
                        let new = match mail::fetch_delta(&mut session, &folder, seen_max, seen_max + 1)? {
                            Some(d) => d.new,
                            // More new mail than a page: the newest page.
                            None => mail::fetch_headers(&mut session, &folder)?.headers,
                        };
                        // Also changes when mail is read elsewhere.
                        let n = session.uid_search("UNSEEN")?.len() as u32;
                        Ok(Some((changed, new, Some(n))))
                    });
                    (session, r)
                })
                .await;
                match waited {
                    Ok((_, Ok(None))) => return,
                    Ok((s, Ok(Some((changed, new, n))))) => {
                        session = s;
                        if !changed {
                            continue;
                        }
                        if n.is_some() && n != unread {
                            unread = n;
                            if out
                                .send(Message::OtherUnread(watch.account.email.clone(), n.unwrap_or(0)))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        let msg = if watch.other {
                            // Mail a rule keeps quiet is not notified.
                            let quiet = crate::backend::Backend::silenced(&watch.account, &new);
                            let fresh: Vec<(String, String)> = new
                                .iter()
                                .filter(|h| h.uid > seen_max && !h.seen && !quiet.contains(&h.uid))
                                .map(|h| (h.from.clone(), h.subject.clone()))
                                .collect();
                            seen_max = new.iter().map(|h| h.uid).max().unwrap_or(0).max(seen_max);
                            if fresh.is_empty() {
                                continue;
                            }
                            Message::OtherMail(watch.account.email.clone(), fresh)
                        } else {
                            Message::Push(watch.generation)
                        };
                        if out.send(msg).await.is_err() {
                            return;
                        }
                    }
                    // Connection lost: reconnect.
                    _ => break,
                }
            }
        }
    })
}

/// The session bus connection used for notifications and the dock badge,
/// kept for the app's lifetime: GNOME Shell destroys an application's
/// notifications as soon as the connection that sent them closes, and a
/// connection per notification (what notify-rust does) closes right after
/// sending, so they never showed. Opened again once if it broke (session
/// bus restarted).
#[cfg(all(unix, not(target_os = "macos")))]
fn on_bus<T>(f: impl Fn(&zbus::blocking::Connection) -> zbus::Result<T>) -> Option<T> {
    use std::sync::Mutex;
    static BUS: Mutex<Option<zbus::blocking::Connection>> = Mutex::new(None);
    let mut bus = BUS.lock().unwrap_or_else(|p| p.into_inner());
    for _ in 0..2 {
        if bus.is_none() {
            *bus = zbus::blocking::Connection::session().ok();
        }
        match f(bus.as_ref()?) {
            Ok(v) => return Some(v),
            Err(_) => *bus = None,
        }
    }
    None
}

/// Ids of the notifications shown and not closed yet.
#[cfg(all(unix, not(target_os = "macos")))]
static SHOWN: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

/// Shows a desktop notification (blocking: run off the UI thread).
#[cfg(all(unix, not(target_os = "macos")))]
pub fn notify(summary: String, body: String) {
    use std::collections::HashMap;
    let id = on_bus(|conn| {
        // Names the app (ferrowl.desktop, as installed by the package and
        // by desktop_entry), whatever process happened to launch it.
        let hints: HashMap<&str, zbus::zvariant::Value> =
            HashMap::from([("desktop-entry", zbus::zvariant::Value::from("ferrowl"))]);
        conn.call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Ferrowl",
                0u32,
                "",
                summary.as_str(),
                body.as_str(),
                Vec::<&str>::new(),
                hints,
                -1i32,
            ),
        )?
        .body()
        .deserialize::<u32>()
    });
    if let Some(id) = id {
        SHOWN.lock().unwrap_or_else(|p| p.into_inner()).push(id);
    }
}

/// Shows `count` (unread mail) on the dock icon, Unity launcher API (Ubuntu
/// Dock, Dash to Dock, KDE). The dock otherwise counts the notifications
/// still open; when nothing is unread they are closed, so the icon is clean.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn set_badge(count: u32) {
    use std::collections::HashMap;
    on_bus(|conn| {
        let props: HashMap<&str, zbus::zvariant::Value> = HashMap::from([
            ("count", zbus::zvariant::Value::from(i64::from(count))),
            ("count-visible", zbus::zvariant::Value::from(count > 0)),
        ]);
        conn.emit_signal(
            None::<&str>,
            "/com/ferrowl/Ferrowl/LauncherEntry",
            "com.canonical.Unity.LauncherEntry",
            "Update",
            &("application://ferrowl.desktop", props),
        )
    });
    if count == 0 {
        let ids = std::mem::take(&mut *SHOWN.lock().unwrap_or_else(|p| p.into_inner()));
        for id in ids {
            on_bus(|conn| {
                conn.call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "CloseNotification",
                    &(id),
                )
                .map(|_| ())
            });
        }
    }
}

/// The unread count on the dock icon (Linux only).
#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn set_badge(_count: u32) {}

/// Shows a desktop notification (blocking: run off the UI thread).
#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn notify(summary: String, body: String) {
    let _ = notify_rust::Notification::new()
        .appname("Ferrowl")
        .summary(&summary)
        .body(&body)
        .show();
}
