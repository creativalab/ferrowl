use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    /// Name shown as sender, e.g. "Fabio Carli"
    pub display_name: String,
    /// Email address, e.g. "name@domain.com"
    pub email: String,
    pub username: String,
    pub password: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    /// 465 = implicit TLS, 587 = STARTTLS
    pub smtp_port: u16,
    /// Passphrase of this account's OpenPGP key (empty = none).
    #[serde(default)]
    pub pgp_passphrase: String,
    /// How incoming mail is read. With POP3, `imap_host`/`imap_port` are the
    /// POP3 server and messages are kept in a local mailbox.
    #[serde(default)]
    pub protocol: Protocol,
    /// POP3: days a downloaded message stays on the server
    /// (`None` = never deleted, `Some(0)` = deleted right after download).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pop3_keep_days: Option<u32>,
    /// Password and passphrase are in the system keychain, not in this file.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keyring: bool,
    /// Contacts server (CardDAV); empty = found from the address's domain.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub carddav_url: String,
    /// Calendar server (CalDAV); empty = found from the address's domain.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caldav_url: String,
    /// Username for the contacts and calendar servers when they do not take
    /// the mail one (a calendar elsewhere); empty = the mail username and
    /// password.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dav_username: String,
    /// Its password (in the keychain, like the mail password).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub dav_password: String,
    /// Signed in with Google or Microsoft (OAuth2) instead of a password;
    /// the tokens live in the keychain, not here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<crate::oauth::Provider>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    #[default]
    Imap,
    Pop3,
}

impl Default for Account {
    fn default() -> Self {
        Self {
            display_name: String::new(),
            email: String::new(),
            username: String::new(),
            password: String::new(),
            imap_host: String::new(),
            imap_port: 993,
            smtp_host: String::new(),
            smtp_port: 465,
            pgp_passphrase: String::new(),
            protocol: Protocol::Imap,
            pop3_keep_days: None,
            keyring: false,
            carddav_url: String::new(),
            caldav_url: String::new(),
            dav_username: String::new(),
            dav_password: String::new(),
            oauth: None,
        }
    }
}

impl Account {
    pub fn is_pop3(&self) -> bool {
        self.protocol == Protocol::Pop3
    }

    pub fn is_complete(&self) -> bool {
        !self.email.is_empty()
            && !self.username.is_empty()
            && (!self.password.is_empty() || self.oauth.is_some())
            && !self.imap_host.is_empty()
            && !self.smtp_host.is_empty()
    }

    /// Initials for the avatar, e.g. "Fabio Carli" -> "FC".
    pub fn initials(&self) -> String {
        let source = if self.display_name.trim().is_empty() {
            &self.email
        } else {
            &self.display_name
        };
        crate::initials(source)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    Compact,
    #[default]
    Comfortable,
    Spacious,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Ui {
    /// Language code ("it", "en", …) or "auto" to follow the system.
    pub language: String,
    pub theme: String,
    pub density: Density,
    pub show_preview: bool,
    /// Compose with Markdown formatting (sent as HTML + plain text).
    pub rich_compose: bool,
    /// Show the HTML version of messages (converted, images blocked) when present.
    pub prefer_html: bool,
    /// Desktop notification for new mail in the inbox.
    pub notifications: bool,
    /// Load remote images/styles in HTML mail (senders can see when it is opened).
    pub load_remote_images: bool,
    /// Message lists grouped into conversations (false: one row per message).
    pub threaded: bool,
    /// Order of message lists.
    pub sort: crate::sort::SortOrder,
    /// Rows per page of the message list (20, 50 or 100).
    pub page_size: u32,
    /// Width of the message list beside the reader, in pixels (the user
    /// drags its edge; kept between LIST_MIN_WIDTH and LIST_MAX_WIDTH).
    pub list_width: f32,
}

pub const LIST_MIN_WIDTH: f32 = 410.0;
pub const LIST_MAX_WIDTH: f32 = 760.0;

impl Default for Ui {
    fn default() -> Self {
        Self {
            language: "auto".into(),
            theme: "ruggine".into(),
            density: Density::Comfortable,
            show_preview: true,
            rich_compose: true,
            prefer_html: true,
            notifications: true,
            load_remote_images: true,
            threaded: true,
            sort: crate::sort::SortOrder::Newest,
            page_size: 20,
            list_width: LIST_MIN_WIDTH,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub accounts: Vec<Account>,
    /// Index into `accounts` of the account shown in the main window.
    pub active: usize,
    /// Single-account format of earlier versions: read, moved into `accounts`.
    #[serde(rename = "account", skip_serializing)]
    legacy_account: Option<Account>,
    pub ui: Ui,
}

fn config_dir() -> Result<PathBuf> {
    crate::paths::config_dir().context("configuration directory not found")
}

fn config_path() -> Result<PathBuf> {
    Ok(config_dir()?.join("config.toml"))
}

impl Config {
    pub fn load() -> Config {
        let Ok(path) = config_path() else {
            return Config::default();
        };
        // Migration from the first version ("client-mail").
        let legacy = path
            .parent()
            .and_then(|p| p.parent())
            .map(|p| p.join("client-mail").join("config.toml"));
        let text = std::fs::read_to_string(&path)
            .ok()
            .or_else(|| legacy.as_ref().and_then(|l| std::fs::read_to_string(l).ok()));
        let config: Config = text.and_then(|s| toml::from_str(&s).ok()).unwrap_or_default();
        let mut config = config.migrated();
        crate::secrets::load(&crate::secrets::SystemKeychain, &mut config.accounts);
        // Passwords still in the file (older versions): move them to the keychain.
        if config
            .accounts
            .iter()
            .any(|a| !a.keyring && !a.password.is_empty())
        {
            let _ = config.save();
        }
        // Once the current file exists, the first version's file (which held
        // the password in plain text) is obsolete.
        if let Some(legacy) = legacy
            && legacy.exists()
            && path.exists()
        {
            let _ = std::fs::remove_file(&legacy);
            let _ = legacy.parent().map(std::fs::remove_dir);
        }
        config
    }

    fn migrated(mut self) -> Self {
        if let Some(acc) = self.legacy_account.take()
            && self.accounts.is_empty()
        {
            self.accounts.push(acc);
        }
        if self.active >= self.accounts.len() {
            self.active = 0;
        }
        self
    }

    /// The account shown in the main window.
    pub fn account(&self) -> Option<&Account> {
        self.accounts.get(self.active)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        std::fs::create_dir_all(config_dir()?)?;
        let file = Config {
            accounts: crate::secrets::store(&crate::secrets::SystemKeychain, &self.accounts),
            ..self.clone()
        };
        std::fs::write(&path, toml::to_string_pretty(&file)?)?;
        // Without a keychain the file holds passwords: owner only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_single_account_is_migrated() {
        let text = r#"
            [account]
            display_name = "A"
            email = "a@x.it"
            username = "a@x.it"
            password = "p"
            imap_host = "imap.x.it"
            imap_port = 993
            smtp_host = "smtp.x.it"
            smtp_port = 465
        "#;
        let c = toml::from_str::<Config>(text).unwrap().migrated();
        assert_eq!(c.accounts.len(), 1);
        assert_eq!(c.account().unwrap().email, "a@x.it");
        let out = toml::to_string_pretty(&c).unwrap();
        assert!(out.contains("[[accounts]]") && !out.contains("[account]"));
    }
}
