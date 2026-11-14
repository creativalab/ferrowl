//! One IMAP IDLE connection per account, each on its own thread.
//!
//! When the inbox gets new unread messages (an unread message above the
//! previous UIDNEXT) the phone is notified. When unread messages are read
//! or removed elsewhere, or read ones are marked unread again, the phone is
//! told silently, a few seconds later (several in a row make one message),
//! so it can take back their notifications and update its unread count. Mail already read when it arrives wakes nobody. Connection errors
//! are retried with a growing pause.

use crate::fcm::{Delivery, Event, Notifier};
use crate::store::Store;
use ferrowl_core::config::Account;
use ferrowl_core::mail;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zeroize::Zeroize;

/// IDLE is renewed well within the 29 minutes allowed by RFC 2177. It also
/// bounds how long a stopped watcher keeps its connection open.
const IDLE_TIMEOUT: Duration = Duration::from_secs(9 * 60);
const RETRY_MIN: Duration = Duration::from_secs(30);
const RETRY_MAX: Duration = Duration::from_secs(15 * 60);
/// A session that watched this long worked: the next pause is short again.
const SESSION_OK: Duration = Duration::from_secs(60);
/// Mail read elsewhere is reported once nothing more was read for this long
/// (reading several messages in a row makes one message to the phone)...
const READ_QUIET: Duration = Duration::from_secs(4);
/// ...or at the latest this long after the first one.
const READ_MAX_DELAY: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct Watchers {
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl Watchers {
    pub fn is_running(&self, account_id: &str) -> bool {
        self.running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(account_id)
    }

    /// Starts watching (no-op if already running).
    pub fn start(
        self: &Arc<Self>,
        account_id: String,
        device_id: String,
        account: Account,
        store: Arc<Store>,
        notifier: Arc<dyn Notifier>,
    ) {
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
            if running.contains_key(&account_id) {
                return;
            }
            running.insert(account_id.clone(), stop.clone());
        }
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name(format!("watch-{account_id}"))
            .spawn(move || {
                let mut account = account;
                watch(
                    &account_id,
                    &device_id,
                    &account,
                    &stop,
                    &store,
                    notifier.as_ref(),
                );
                account.password.zeroize();
                // Only its own entry: after stop() a new watcher for the
                // same account may already be running.
                let mut running = me.running.lock().unwrap_or_else(|p| p.into_inner());
                if running
                    .get(&account_id)
                    .is_some_and(|flag| Arc::ptr_eq(flag, &stop))
                {
                    running.remove(&account_id);
                }
            });
        if spawned.is_err() {
            eprintln!("cannot start a watcher thread");
        }
    }

    /// Asks a watcher to stop (it exits at its next wake-up).
    pub fn stop(&self, account_id: &str) {
        if let Some(flag) = self
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(account_id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }
}

fn watch(
    account_id: &str,
    device_id: &str,
    account: &Account,
    stop: &AtomicBool,
    store: &Store,
    notifier: &dyn Notifier,
) {
    let mut retry = RETRY_MIN;
    // Kept across reconnections, so a drop does not lose the report.
    let mut read_pending = None;
    while !stop.load(Ordering::Relaxed) {
        let mut watching_since = None;
        let ended = idle_session(
            account_id,
            account,
            stop,
            &mut read_pending,
            &mut watching_since,
            |event| tell_device(account_id, device_id, store, notifier, event),
        );
        // A session that watched for a while starts the pauses over.
        if watching_since.is_some_and(|t: Instant| t.elapsed() >= SESSION_OK) {
            retry = RETRY_MIN;
        }
        match ended {
            Ok(Ended::Stopped) => return,
            Ok(Ended::NoIdle) => {
                eprintln!("account {account_id}: server has no IDLE, not watched");
                return;
            }
            Ok(Ended::Gone) => return,
            // A refused login is not retried: repeated failures get the
            // server's IP blocked by mail providers' firewalls, which would
            // hurt every account watched from here. The phone registers
            // the account again after the user fixes the password.
            Err(e) if login_refused(&e) => {
                eprintln!("account {account_id}: login refused, watching stopped");
                return;
            }
            Err(e) => {
                // Only the error kind: messages may contain the user name.
                eprintln!("account {account_id}: connection lost ({}), retrying", short(&e));
                sleep_unless_stopped(retry, stop);
                retry = (retry * 2).min(RETRY_MAX);
            }
        }
    }
}

enum Ended {
    Stopped,
    NoIdle,
    /// The device was unregistered.
    Gone,
}

/// `read_pending`: mail read elsewhere not reported yet, (first, latest)
/// change. `watching_since`: set once the inbox is being watched.
fn idle_session(
    account_id: &str,
    account: &Account,
    stop: &AtomicBool,
    read_pending: &mut Option<(Instant, Instant)>,
    watching_since: &mut Option<Instant>,
    mut tell: impl FnMut(Event) -> bool,
) -> anyhow::Result<Ended> {
    let mut session = crate::addr::connect(account)?;
    if !mail::supports_idle(&mut session)? {
        let _ = session.logout();
        return Ok(Ended::NoIdle);
    }
    let first = snapshot(&mut session)?;
    let mut exists = first.exists;
    let mut uid_next = first.uid_next;
    // UIDs only, kept in memory: which inbox messages are unread.
    let (mut unseen, early) = without_early(first.unseen, uid_next);
    // Something changed while looking (or mail arrived between the SELECT
    // and the search): look again before waiting.
    let mut look_now = first.stale || early;
    let mut looks_in_a_row = 0;
    *watching_since = Some(Instant::now());
    eprintln!("account {account_id}: watching the inbox");
    while !stop.load(Ordering::Relaxed) {
        let changed = if std::mem::take(&mut look_now) {
            true
        } else {
            let wait = match *read_pending {
                Some((first, last)) => (last + READ_QUIET)
                    .min(first + READ_MAX_DELAY)
                    .saturating_duration_since(Instant::now()),
                None => IDLE_TIMEOUT,
            };
            // Not selected again: the server then reports every change
            // since the last look, even one made before IDLE began.
            mail::idle_wait_selected(&mut session, wait.max(Duration::from_secs(1)))?
        };
        // Removed while waiting (the wait ends only with a change or a
        // timeout): nothing more for an account that is gone.
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if changed {
            let now = snapshot(&mut session)?;
            let now_unseen = now.unseen;
            let change = compare(
                unseen.as_ref(),
                now_unseen.as_ref(),
                uid_next,
                now.exists > exists,
            );
            // A few looks in a row at most, should a server always report
            // something during the search.
            looks_in_a_row = if now.stale { looks_in_a_row + 1 } else { 0 };
            look_now = now.stale && looks_in_a_row < 3;
            if change.arrived {
                eprintln!("account {account_id}: new mail, notifying");
                if !tell(Event::NewMail) {
                    let _ = session.logout();
                    return Ok(Ended::Gone);
                }
            }
            if change.read {
                let t = Instant::now();
                *read_pending = Some((read_pending.map_or(t, |(first, _)| first), t));
            }
            exists = now.exists;
            uid_next = now.uid_next.or(uid_next);
            unseen = now_unseen.or(unseen);
        }
        if let Some((first, last)) = *read_pending {
            let t = Instant::now();
            if t >= last + READ_QUIET || t >= first + READ_MAX_DELAY {
                *read_pending = None;
                eprintln!("account {account_id}: mail read elsewhere, telling the phone");
                if !tell(Event::Read) {
                    let _ = session.logout();
                    return Ok(Ended::Gone);
                }
            }
        }
    }
    let _ = session.logout();
    Ok(Ended::Stopped)
}

/// The inbox as seen at one look.
struct Snapshot {
    exists: u32,
    uid_next: Option<u32>,
    /// Unread UIDs (`None` if the server refused the search).
    unseen: Option<HashSet<u32>>,
    /// The server reported a change during the search, which the result
    /// may miss and IDLE will not report again.
    stale: bool,
}

/// Selects the inbox and takes its unread UIDs.
fn snapshot(session: &mut mail::ImapSession) -> anyhow::Result<Snapshot> {
    let inbox = session.select("INBOX")?;
    // Changes from before the SELECT are in its answer.
    while session.unsolicited_responses.try_recv().is_ok() {}
    let unseen = mail::unseen_from(session, 1).ok().map(HashSet::from_iter);
    let mut stale = false;
    while session.unsolicited_responses.try_recv().is_ok() {
        stale = true;
    }
    Ok(Snapshot {
        exists: inbox.exists,
        uid_next: inbox.uid_next,
        unseen,
        stale,
    })
}

/// The first unread UIDs, without mail that arrived between the SELECT and
/// the search (at or above UIDNEXT): left out, it counts as new at the next
/// look. True if there was any.
fn without_early(unseen: Option<HashSet<u32>>, uid_next: Option<u32>) -> (Option<HashSet<u32>>, bool) {
    match (unseen, uid_next) {
        (Some(mut unseen), Some(next)) => {
            let before = unseen.len();
            unseen.retain(|u| *u < next);
            let early = unseen.len() < before;
            (Some(unseen), early)
        }
        (unseen, _) => (unseen, false),
    }
}

/// What a change of the inbox means for the phone.
#[derive(Debug, PartialEq, Eq)]
struct Change {
    /// A new message arrived unread: notify.
    arrived: bool,
    /// An unread message was read or removed, or a read one was marked
    /// unread again: take back notifications, update the counts (sent as
    /// the "read" push, which the apps answer by counting again).
    read: bool,
}

/// Compares the unread UIDs before and after a change. Only messages above
/// the previous UIDNEXT are new (one marked unread again is not); mail
/// already read when it arrives is not news. Without the unread UIDs
/// (search refused) or UIDNEXT, a growing inbox is news, as it used to be.
fn compare(
    before: Option<&HashSet<u32>>,
    after: Option<&HashSet<u32>>,
    uid_next: Option<u32>,
    grew: bool,
) -> Change {
    let (Some(before), Some(after)) = (before, after) else {
        return Change {
            arrived: grew,
            read: false,
        };
    };
    let arrived = match uid_next {
        Some(next) => after.iter().any(|u| *u >= next && !before.contains(u)),
        None => grew,
    };
    let removed = before.iter().any(|u| !after.contains(u));
    // Unread again: not new (or the inbox did not grow without UIDNEXT).
    let unread_again = after.iter().any(|u| {
        !before.contains(u)
            && match uid_next {
                Some(next) => *u < next,
                None => !grew,
            }
    });
    Change {
        arrived,
        read: removed || unread_again,
    }
}

/// Tells the device. Returns false when the device no longer exists.
fn tell_device(
    account_id: &str,
    device_id: &str,
    store: &Store,
    notifier: &dyn Notifier,
    event: Event,
) -> bool {
    let target = match store.target(device_id) {
        Ok(Some(t)) => t,
        _ => return false,
    };
    match notifier.notify(&target, account_id, event) {
        Ok(Delivery::Sent) => {
            eprintln!(
                "account {account_id}: {} sent ({})",
                event.name(),
                target.platform.name()
            );
            true
        }
        Ok(Delivery::Unregistered) => {
            eprintln!("device {device_id}: push token no longer valid, removing the device");
            let _ = store.delete_device(device_id);
            false
        }
        Err(e) => {
            eprintln!("account {account_id}: notification failed ({})", short(&e));
            true
        }
    }
}

fn sleep_unless_stopped(total: Duration, stop: &AtomicBool) {
    let step = Duration::from_secs(1);
    let mut waited = Duration::ZERO;
    while waited < total && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(step);
        waited += step;
    }
}

/// The server rejected the credentials (the core's "login failed" errors;
/// the server runs the core in English).
fn login_refused(e: &anyhow::Error) -> bool {
    format!("{e:#}").to_lowercase().contains("login failed")
}

/// First line of an error, without the chain (kept short for logs).
fn short(e: &anyhow::Error) -> String {
    e.to_string()
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .take(120)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(uids: &[u32]) -> HashSet<u32> {
        uids.iter().copied().collect()
    }

    #[test]
    fn new_unread_mail_is_news() {
        let c = compare(Some(&set(&[5])), Some(&set(&[5, 10])), Some(10), true);
        assert_eq!(
            c,
            Change {
                arrived: true,
                read: false
            }
        );
    }

    #[test]
    fn mail_already_read_is_not_news() {
        let c = compare(Some(&set(&[5])), Some(&set(&[5])), Some(10), true);
        assert_eq!(
            c,
            Change {
                arrived: false,
                read: false
            }
        );
    }

    #[test]
    fn marked_unread_again_is_not_news_but_updates_counts() {
        let c = compare(Some(&set(&[5])), Some(&set(&[3, 5])), Some(10), false);
        assert_eq!(
            c,
            Change {
                arrived: false,
                read: true
            }
        );
        // Without UIDNEXT: an inbox that did not grow.
        let c = compare(Some(&set(&[5])), Some(&set(&[3, 5])), None, false);
        assert_eq!(
            c,
            Change {
                arrived: false,
                read: true
            }
        );
    }

    #[test]
    fn read_or_removed_elsewhere_is_reported() {
        let c = compare(Some(&set(&[5, 7])), Some(&set(&[7])), Some(10), false);
        assert_eq!(
            c,
            Change {
                arrived: false,
                read: true
            }
        );
        // Read one, another arrived: both.
        let c = compare(Some(&set(&[5])), Some(&set(&[11])), Some(10), true);
        assert_eq!(
            c,
            Change {
                arrived: true,
                read: true
            }
        );
    }

    #[test]
    fn without_search_a_growing_inbox_is_news() {
        let c = compare(None, Some(&set(&[11])), Some(10), true);
        assert_eq!(
            c,
            Change {
                arrived: true,
                read: false
            }
        );
        let c = compare(Some(&set(&[5])), Some(&set(&[5, 11])), None, true);
        assert_eq!(
            c,
            Change {
                arrived: true,
                read: false
            }
        );
    }

    #[test]
    fn mail_arrived_while_starting_counts_as_new() {
        // UIDNEXT 10 at the SELECT, 10 arrived before the search.
        let (first, early) = without_early(Some(set(&[5, 10])), Some(10));
        assert!(early);
        assert_eq!(first, Some(set(&[5])));
        let c = compare(first.as_ref(), Some(&set(&[5, 10])), Some(10), false);
        assert!(c.arrived);

        assert_eq!(without_early(Some(set(&[5])), Some(10)), (Some(set(&[5])), false));
        assert_eq!(without_early(Some(set(&[5])), None), (Some(set(&[5])), false));
        assert_eq!(without_early(None, Some(10)), (None, false));
    }

    #[test]
    fn an_old_watcher_leaves_a_new_one_running() {
        let watchers = Arc::new(Watchers::default());
        let store = Arc::new(Store::in_memory().unwrap());
        let notifier: Arc<dyn Notifier> = Arc::new(crate::fcm::LogNotifier);
        // Refused address: each watcher fails at once and waits to retry.
        let account = Account {
            imap_host: "127.0.0.1".into(),
            imap_port: 993,
            ..Default::default()
        };
        let start = || {
            watchers.start(
                "acc".into(),
                "dev".into(),
                account.clone(),
                store.clone(),
                notifier.clone(),
            )
        };
        start();
        watchers.stop("acc");
        start();
        // The first one notices its stop flag within a second or so.
        std::thread::sleep(Duration::from_millis(2500));
        assert!(watchers.is_running("acc"));
        watchers.stop("acc");
        assert!(!watchers.is_running("acc"));
    }

    #[test]
    fn recognizes_refused_logins() {
        ferrowl_core::i18n::set(Some("en"));
        let auth = anyhow::anyhow!(ferrowl_core::i18n::tf("err_auth", &[("msg", "NO")]));
        let login = anyhow::anyhow!(ferrowl_core::i18n::tf("err_login", &[("msg", "NO")]));
        assert!(login_refused(&auth) && login_refused(&login));
        assert!(!login_refused(&anyhow::anyhow!("Connection reset by peer")));
    }
}
