//! Account passwords and OpenPGP passphrases in the system keychain
//! (macOS Keychain, Windows Credential Manager, Secret Service on Linux:
//! GNOME Keyring or KWallet).
//!
//! The config file then holds empty strings and `keyring = true`. Where no
//! keychain is available (e.g. a Linux system without Secret Service) the
//! secrets stay in the config file, which is readable by the owner only.

use crate::config::Account;
use std::collections::HashMap;
use std::sync::Mutex;

#[cfg(feature = "keychain")]
const SERVICE: &str = "ferrowl";

/// Storage behind the secrets: the system keychain, or memory in tests.
pub trait Store: Send + Sync {
    fn get(&self, user: &str) -> Option<String>;
    fn set(&self, user: &str, secret: &str) -> bool;
    fn delete(&self, user: &str);
}

/// The platform keychain. Without the `keychain` feature (mobile builds,
/// which keep secrets through the platform) every write fails, so secrets
/// stay in the config file the app provides.
pub struct SystemKeychain;

/// Development and test profiles (`FERROWL_NO_KEYCHAIN` set) keep their
/// secrets out of the user's real keychain.
#[cfg(feature = "keychain")]
fn keychain_off() -> bool {
    std::env::var_os("FERROWL_NO_KEYCHAIN").is_some()
}

#[cfg(feature = "keychain")]
impl Store for SystemKeychain {
    fn get(&self, user: &str) -> Option<String> {
        if keychain_off() {
            return None;
        }
        keyring::Entry::new(SERVICE, user).ok()?.get_password().ok()
    }

    fn set(&self, user: &str, secret: &str) -> bool {
        if keychain_off() {
            return false;
        }
        match keyring::Entry::new(SERVICE, user).and_then(|e| e.set_password(secret)) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("keychain unavailable, keeping secrets in the config file: {e}");
                false
            }
        }
    }

    fn delete(&self, user: &str) {
        if keychain_off() {
            return;
        }
        if let Ok(e) = keyring::Entry::new(SERVICE, user) {
            let _ = e.delete_credential();
        }
    }
}

#[cfg(not(feature = "keychain"))]
impl Store for SystemKeychain {
    fn get(&self, _user: &str) -> Option<String> {
        None
    }
    fn set(&self, _user: &str, _secret: &str) -> bool {
        false
    }
    fn delete(&self, _user: &str) {}
}

/// Keychain entries of an account: (entry name, secret).
fn entries(a: &Account) -> [(String, &String); 3] {
    let id = format!("{}@{}", a.username.trim(), a.imap_host.trim());
    [
        (format!("password:{id}"), &a.password),
        (format!("pgp-passphrase:{id}"), &a.pgp_passphrase),
        (format!("dav-password:{id}"), &a.dav_password),
    ]
}

/// What this process last wrote to (or read from) the keychain, so unchanged
/// secrets are not written again on every save and removed accounts can be
/// cleaned up.
static KNOWN: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn known() -> std::sync::MutexGuard<'static, Option<HashMap<String, String>>> {
    KNOWN.lock().unwrap_or_else(|p| p.into_inner())
}

/// After loading: fills in the secrets of accounts stored in the keychain.
pub fn load(store: &dyn Store, accounts: &mut [Account]) {
    let mut known = known();
    let known = known.get_or_insert_with(HashMap::new);
    for a in accounts.iter_mut().filter(|a| a.keyring) {
        let [(pw_key, _), (pp_key, _), (dav_key, _)] = entries(a);
        a.password = store.get(&pw_key).unwrap_or_default();
        a.pgp_passphrase = store.get(&pp_key).unwrap_or_default();
        a.dav_password = store.get(&dav_key).unwrap_or_default();
        known.insert(pw_key, a.password.clone());
        known.insert(pp_key, a.pgp_passphrase.clone());
        known.insert(dav_key, a.dav_password.clone());
    }
}

/// Before saving: moves each account's secrets to the keychain and returns
/// the accounts as they go in the file (secrets blanked where stored).
pub fn store(store: &dyn Store, accounts: &[Account]) -> Vec<Account> {
    let mut known = known();
    let known = known.get_or_insert_with(HashMap::new);
    let mut in_use = std::collections::HashSet::new();

    let out = accounts
        .iter()
        .map(|a| {
            let mut stored = true;
            for (key, secret) in entries(a) {
                in_use.insert(key.clone());
                if known.get(&key) == Some(secret) {
                    continue;
                }
                if secret.is_empty() {
                    store.delete(&key);
                    known.insert(key, String::new());
                } else if store.set(&key, secret) {
                    known.insert(key, secret.clone());
                } else {
                    stored = false;
                }
            }
            let mut file = a.clone();
            file.keyring = stored;
            if stored {
                file.password.clear();
                file.pgp_passphrase.clear();
                file.dav_password.clear();
            }
            file
        })
        .collect();

    // Accounts removed or renamed: their old entries go too.
    let stale: Vec<String> = known.keys().filter(|k| !in_use.contains(*k)).cloned().collect();
    for key in stale {
        store.delete(&key);
        known.remove(&key);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    pub struct Memory {
        pub items: Mutex<HashMap<String, String>>,
        pub fail: bool,
    }

    impl Store for Memory {
        fn get(&self, user: &str) -> Option<String> {
            self.items.lock().unwrap().get(user).cloned()
        }
        fn set(&self, user: &str, secret: &str) -> bool {
            if self.fail {
                return false;
            }
            self.items.lock().unwrap().insert(user.into(), secret.into());
            true
        }
        fn delete(&self, user: &str) {
            self.items.lock().unwrap().remove(user);
        }
    }

    fn account(user: &str, password: &str) -> Account {
        Account {
            username: user.into(),
            password: password.into(),
            imap_host: "imap.example.org".into(),
            ..Default::default()
        }
    }

    // One test only: the `KNOWN` cache is process-wide.
    #[test]
    fn round_trip_fallback_and_cleanup() {
        let keychain = Memory::default();
        let mut accounts = vec![account("ada", "secret-1"), account("bob", "secret-2")];
        accounts[0].pgp_passphrase = "pp".into();

        // Saved: the file has no secrets, the keychain has them.
        let file = store(&keychain, &accounts);
        assert!(
            file.iter()
                .all(|a| a.keyring && a.password.is_empty() && a.pgp_passphrase.is_empty())
        );
        assert_eq!(keychain.items.lock().unwrap().len(), 3);

        // Loaded back: secrets restored.
        let mut loaded = file.clone();
        load(&keychain, &mut loaded);
        assert_eq!(loaded[0].password, "secret-1");
        assert_eq!(loaded[0].pgp_passphrase, "pp");
        assert_eq!(loaded[1].password, "secret-2");

        // Account removed: its entry is deleted.
        store(&keychain, &loaded[..1]);
        assert!(!keychain.items.lock().unwrap().keys().any(|k| k.contains("bob")));

        // No keychain: secrets stay in the file.
        let broken = Memory {
            fail: true,
            ..Default::default()
        };
        let file = store(&broken, &[account("eve", "secret-3")]);
        assert!(!file[0].keyring);
        assert_eq!(file[0].password, "secret-3");
    }
}
