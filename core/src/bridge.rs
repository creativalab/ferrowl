//! Ferrowl Bridge: what other software may ask of Ferrowl, the same on every
//! platform. The desktop's command line and MCP server, the phone's
//! Shortcuts/Siri actions and Android intents all go through here, so that
//! links, permissions and the free/Pro split never differ between them.
//!
//! See `docs/integrazioni-desktop.md` for the whole plan.

use std::collections::HashMap;

use url::Url;

use crate::backend::Backend;
use crate::config::Account;
use crate::mail::Header;

/// The scheme of Ferrowl's own links.
pub const SCHEME: &str = "ferrowl";

/// What a `ferrowl://` (or `mailto:`) link asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// Open a message: `ferrowl://open?account=<email>&uid=<uid>[&folder=<id>]`
    /// (the inbox when no folder is given).
    Open {
        account: String,
        folder: String,
        uid: u32,
    },
    /// Show an account's inbox: `ferrowl://inbox[?account=<email>]`.
    Inbox { account: Option<String> },
    /// Write a message: `ferrowl://compose?to=…&cc=…&subject=…&body=…`, or
    /// a `mailto:` link. Nothing is sent: the user sees the draft first.
    Compose {
        to: String,
        cc: String,
        bcc: String,
        subject: String,
        body: String,
    },
}

/// Reads a `ferrowl://` or `mailto:` link; `None` when it is neither or is
/// incomplete.
pub fn parse_link(text: &str) -> Option<Link> {
    let url = Url::parse(text.trim()).ok()?;
    let query: HashMap<String, String> = url
        .query_pairs()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.into_owned()))
        .collect();
    let get = |k: &str| query.get(k).cloned().unwrap_or_default();
    match url.scheme().to_ascii_lowercase().as_str() {
        "mailto" => {
            // mailto:a@x.it,b@y.it?cc=…&subject=…; "to" may also be a field.
            let path = percent_decode(url.path());
            let to = [path, get("to")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            Some(Link::Compose {
                to,
                cc: get("cc"),
                bcc: get("bcc"),
                subject: get("subject"),
                body: get("body"),
            })
        }
        SCHEME => {
            // ferrowl://open?… has "open" as its host; ferrowl:open?… as its path.
            let action = url
                .host_str()
                .map(str::to_string)
                .unwrap_or_else(|| url.path().trim_matches('/').to_string())
                .to_ascii_lowercase();
            match action.as_str() {
                "open" => {
                    let account = get("account");
                    let uid = get("uid").parse().ok()?;
                    if account.is_empty() || uid == 0 {
                        return None;
                    }
                    let folder = Some(get("folder"))
                        .filter(|f| !f.is_empty())
                        .unwrap_or_else(|| "INBOX".into());
                    Some(Link::Open { account, folder, uid })
                }
                "inbox" => Some(Link::Inbox {
                    account: Some(get("account")).filter(|a| !a.is_empty()),
                }),
                "compose" => Some(Link::Compose {
                    to: get("to"),
                    cc: get("cc"),
                    bcc: get("bcc"),
                    subject: get("subject"),
                    body: get("body"),
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

fn percent_decode(s: &str) -> String {
    url::form_urlencoded::parse(format!("x={}", s.replace('+', "%2B")).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default()
}

/// The link that opens a message, for other software to keep and use later.
pub fn open_link(account: &str, folder: &str, uid: u32) -> String {
    let mut url = Url::parse("ferrowl://open").expect("valid link");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("account", account);
        if !folder.eq_ignore_ascii_case("INBOX") {
            q.append_pair("folder", folder);
        }
        q.append_pair("uid", &uid.to_string());
    }
    url.into()
}

/// What another program may do, granted one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Capability {
    /// Search and read mail, list unread messages.
    Read,
    /// Change mail: move, archive, trash, mark, label, drafts.
    Write,
    /// Send mail.
    Send,
    /// Read the address book.
    Contacts,
    /// Read and add calendar events.
    Calendar,
    /// Open Ferrowl on a message or a draft (the user stays in charge).
    Open,
}

/// Whether a capability needs Ferrowl Pro. Reading, opening and drafting by
/// hand are free; acting on the user's behalf (sending, changing mail) is
/// Pro. Agreed in `docs/integrazioni-desktop.md`.
pub fn needs_pro(capability: Capability) -> bool {
    matches!(capability, Capability::Send | Capability::Write)
}

/// Whether a capability changes or sends something, so the user confirms
/// it unless they chose to always allow that program. Mail is untrusted
/// input: an assistant may have been told by a message to act.
pub fn needs_confirmation(capability: Capability) -> bool {
    matches!(capability, Capability::Send | Capability::Write)
}

/// Search in what is stored on the device for any account (not only the
/// connected one), newest first, at most `limit` results.
pub fn search(account: &Account, folder: &str, query: &str, limit: usize) -> Vec<Header> {
    let mut found = Backend::search_query(account, folder, query);
    found.sort_by_key(|h| std::cmp::Reverse(h.timestamp));
    found.truncate(limit);
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_links_round_trip() {
        let link = open_link("anna@example.com", "INBOX", 42);
        assert_eq!(link, "ferrowl://open?account=anna%40example.com&uid=42");
        assert_eq!(
            parse_link(&link),
            Some(Link::Open {
                account: "anna@example.com".into(),
                folder: "INBOX".into(),
                uid: 42
            })
        );
        let other = open_link("a@x.it", "[Gmail]/Sent Mail", 7);
        assert_eq!(
            parse_link(&other),
            Some(Link::Open {
                account: "a@x.it".into(),
                folder: "[Gmail]/Sent Mail".into(),
                uid: 7
            })
        );
    }

    #[test]
    fn incomplete_or_foreign_links_are_refused() {
        assert_eq!(parse_link("ferrowl://open?account=a@x.it"), None);
        assert_eq!(parse_link("ferrowl://open?uid=3"), None);
        assert_eq!(parse_link("ferrowl://delete?uid=3"), None);
        assert_eq!(parse_link("https://example.com/open?uid=3"), None);
        assert_eq!(parse_link("not a link"), None);
    }

    #[test]
    fn compose_and_mailto() {
        assert_eq!(
            parse_link("ferrowl://compose?to=b%40y.it&subject=Ciao%20a%20te&body=Riga+1"),
            Some(Link::Compose {
                to: "b@y.it".into(),
                cc: String::new(),
                bcc: String::new(),
                subject: "Ciao a te".into(),
                body: "Riga 1".into(),
            })
        );
        assert_eq!(
            parse_link("mailto:a@x.it,b@y.it?cc=c@z.it&subject=Fattura%20ottobre"),
            Some(Link::Compose {
                to: "a@x.it,b@y.it".into(),
                cc: "c@z.it".into(),
                bcc: String::new(),
                subject: "Fattura ottobre".into(),
                body: String::new(),
            })
        );
        assert_eq!(
            parse_link("MAILTO:a+tag@x.it"),
            Some(Link::Compose {
                to: "a+tag@x.it".into(),
                cc: String::new(),
                bcc: String::new(),
                subject: String::new(),
                body: String::new(),
            })
        );
    }

    #[test]
    fn inbox_links() {
        assert_eq!(parse_link("ferrowl://inbox"), Some(Link::Inbox { account: None }));
        assert_eq!(
            parse_link("ferrowl:inbox?account=a@x.it"),
            Some(Link::Inbox {
                account: Some("a@x.it".into())
            })
        );
    }

    #[test]
    fn pro_and_confirmation() {
        assert!(!needs_pro(Capability::Read));
        assert!(!needs_pro(Capability::Open));
        assert!(needs_pro(Capability::Send));
        assert!(needs_confirmation(Capability::Send));
        assert!(!needs_confirmation(Capability::Read));
    }
}
