//! One IMAP IDLE connection per account, each on its own thread.
//!
//! When the inbox gets new unread messages (the EXISTS count grows and a
//! message above the previous UIDNEXT is unread) the phone is notified.
//! Flag changes, deletions and mail already read do not wake it. Connection errors
//! are retried with a growing pause.

use crate::fcm::{Delivery, Notifier};
use crate::store::Store;
use ferrowl_core::config::Account;
use ferrowl_core::mail;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zeroize::Zeroize;

/// IDLE is renewed well within the 29 minutes allowed by RFC 2177. It also
/// bounds how long a stopped watcher keeps its connection open.
const IDLE_TIMEOUT: Duration = Duration::from_secs(9 * 60);
const RETRY_MIN: Duration = Duration::from_secs(30);
const RETRY_MAX: Duration = Duration::from_secs(15 * 60);

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
                me.running
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(&account_id);
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
    while !stop.load(Ordering::Relaxed) {
        match idle_session(account_id, account, stop, || {
            on_new_mail(account_id, device_id, store, notifier)
        }) {
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

fn idle_session(
    account_id: &str,
    account: &Account,
    stop: &AtomicBool,
    mut on_new: impl FnMut() -> bool,
) -> anyhow::Result<Ended> {
    let mut session = mail::connect(account)?;
    if !mail::supports_idle(&mut session)? {
        let _ = session.logout();
        return Ok(Ended::NoIdle);
    }
    let inbox = session.select("INBOX")?;
    let mut exists = inbox.exists;
    let mut uid_next = inbox.uid_next;
    eprintln!("account {account_id}: watching the inbox");
    while !stop.load(Ordering::Relaxed) {
        if mail::idle_wait(&mut session, "INBOX", IDLE_TIMEOUT)? {
            // Removed while waiting (the wait ends only with mail or a
            // timeout): no notification for an account that is gone.
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let now = session.select("INBOX")?;
            if now.exists > exists && worth_notifying(&mut session, uid_next) {
                eprintln!("account {account_id}: new mail, notifying");
                if !on_new() {
                    let _ = session.logout();
                    return Ok(Ended::Gone);
                }
            }
            exists = now.exists;
            uid_next = now.uid_next.or(uid_next);
        }
    }
    let _ = session.logout();
    Ok(Ended::Stopped)
}

/// Whether the messages that just arrived include an unread one: messages
/// already read elsewhere, sent to oneself or moved back into the inbox
/// after reading wake nobody. Without the server's UIDNEXT, or when the
/// search fails, the phone is notified as before.
fn worth_notifying(session: &mut mail::ImapSession, uid_next: Option<u32>) -> bool {
    match uid_next {
        Some(from) => mail::unseen_from(session, from)
            .map(|u| !u.is_empty())
            .unwrap_or(true),
        None => true,
    }
}

/// Notifies the device. Returns false when the device no longer exists.
fn on_new_mail(account_id: &str, device_id: &str, store: &Store, notifier: &dyn Notifier) -> bool {
    let target = match store.target(device_id) {
        Ok(Some(t)) => t,
        _ => return false,
    };
    match notifier.notify(&target, account_id) {
        Ok(Delivery::Sent) => {
            eprintln!(
                "account {account_id}: notification sent ({})",
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

    #[test]
    fn recognizes_refused_logins() {
        ferrowl_core::i18n::set(Some("en"));
        let auth = anyhow::anyhow!(ferrowl_core::i18n::tf("err_auth", &[("msg", "NO")]));
        let login = anyhow::anyhow!(ferrowl_core::i18n::tf("err_login", &[("msg", "NO")]));
        assert!(login_refused(&auth) && login_refused(&login));
        assert!(!login_refused(&anyhow::anyhow!("Connection reset by peer")));
    }
}
