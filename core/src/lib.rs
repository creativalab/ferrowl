//! Ferrowl's engine, shared by the desktop app and the mobile apps:
//! mail protocols (IMAP, POP3, SMTP), MIME and OpenPGP, the local cache and
//! search index, the POP3 mailbox, settings, secrets and translations.
//! Nothing here depends on a user interface toolkit.

pub mod backend;
pub mod bridge;
pub mod cache;
pub mod carddav;
pub mod config;
pub mod crypto;
pub mod dav;
pub mod errors;
pub mod events;
pub mod folder_tree;
pub mod html;
pub mod i18n;
pub mod ical;
pub mod invite;
pub mod local;
pub mod mail;
pub mod markdown;
pub mod oauth;
pub mod paths;
pub mod pop3;
pub mod providers;
pub mod query;
pub mod rules;
pub mod secrets;
pub mod sort;
pub mod threads;
pub mod vcard;

/// Initials for an avatar, e.g. "Fabio Carli" -> "FC", "giulia.ferri@x.it" -> "GF".
pub fn initials(s: &str) -> String {
    let s = s.split('@').next().unwrap_or(s);
    let words: Vec<&str> = s
        .split(|c: char| c.is_whitespace() || c == '.' || c == '_' || c == '-')
        .filter(|w| w.chars().next().is_some_and(char::is_alphanumeric))
        .collect();
    let mut out: String = words
        .iter()
        .take(2)
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .collect();
    if out.is_empty() {
        out.push('?');
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn initials_work() {
        use super::initials;
        assert_eq!(initials("Fabio Carli"), "FC");
        assert_eq!(initials("giulia.ferri@studio.it"), "GF");
        assert_eq!(initials("Amministrazione"), "A");
        assert_eq!(initials(""), "?");
    }
}
