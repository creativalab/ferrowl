//! The search box, the same in every app: free words (matched by the
//! full-text index) and operators, in English as in Gmail whatever the
//! interface language:
//!
//! - `from:mario` — sender's name or address contains "mario"
//! - `subject:fattura` — the subject contains "fattura"
//! - `has:attachment` (or `has:attach…`) — with attachments
//! - `is:unread`, `is:read`, `is:starred`
//!
//! Other `word:value` tokens are ignored (as the desktop always did), so a
//! pasted "Re:" or a link does not empty the results.

use crate::mail::Header;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// Free words, for the full-text index.
    pub words: Vec<String>,
    /// `from:` values (lowercase); all must match.
    pub from: Vec<String>,
    /// `subject:` values (lowercase); all must match.
    pub subject: Vec<String>,
    pub attachment: bool,
    /// `is:unread` (Some(false)) / `is:read` (Some(true)).
    pub seen: Option<bool>,
    pub starred: bool,
}

impl Query {
    pub fn parse(text: &str) -> Query {
        let mut q = Query::default();
        for token in text.split_whitespace() {
            let lower = token.to_lowercase();
            if let Some(v) = lower.strip_prefix("from:") {
                if !v.is_empty() {
                    q.from.push(v.to_string());
                }
            } else if let Some(v) = lower.strip_prefix("subject:") {
                if !v.is_empty() {
                    q.subject.push(v.to_string());
                }
            } else if lower.starts_with("has:attach") {
                q.attachment = true;
            } else if lower == "is:unread" {
                q.seen = Some(false);
            } else if lower == "is:read" {
                q.seen = Some(true);
            } else if lower == "is:starred" {
                q.starred = true;
            } else if !token.contains(':') {
                q.words.push(token.to_string());
            }
        }
        q
    }

    /// Whether there is any operator (a search without free words then
    /// filters the messages already on the device).
    pub fn has_filters(&self) -> bool {
        !self.from.is_empty()
            || !self.subject.is_empty()
            || self.attachment
            || self.seen.is_some()
            || self.starred
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && !self.has_filters()
    }

    /// Whether a message passes the operators (not the free words).
    pub fn matches(&self, h: &Header) -> bool {
        let sender = format!("{} {}", h.from, h.from_address).to_lowercase();
        let subject = h.subject.to_lowercase();
        self.from.iter().all(|f| sender.contains(f.as_str()))
            && self.subject.iter().all(|s| subject.contains(s.as_str()))
            && (!self.attachment || h.has_attachment)
            && self.seen.is_none_or(|seen| h.seen == seen)
            && (!self.starred || h.flagged)
    }

    /// The free words matched in memory (sender, subject, preview), for
    /// lists without the full-text index.
    pub fn matches_words(&self, h: &Header) -> bool {
        let text = format!("{} {} {}", h.from, h.subject, h.preview).to_lowercase();
        self.words.iter().all(|w| text.contains(&w.to_lowercase()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(from: &str, addr: &str, seen: bool, attach: bool, flagged: bool) -> Header {
        Header {
            from: from.into(),
            from_address: addr.into(),
            subject: "Fattura marzo".into(),
            seen,
            has_attachment: attach,
            flagged,
            ..Default::default()
        }
    }

    #[test]
    fn operators_and_words() {
        let q = Query::parse("fattura from:Mario has:attachment is:unread Re: https://x.it");
        assert_eq!(q.words, vec!["fattura".to_string()]);
        assert_eq!(q.from, vec!["mario".to_string()]);
        assert!(q.attachment && q.seen == Some(false) && !q.starred);
        assert!(q.matches(&h("Mario Rossi", "m@x.it", false, true, false)));
        // From matches the address too.
        assert!(Query::parse("from:rossi@").matches(&h("Mario", "rossi@x.it", true, false, false)));
        assert!(!q.matches(&h("Mario Rossi", "m@x.it", true, true, false)));
        assert!(!q.matches(&h("Luca", "l@x.it", false, true, false)));
        assert!(Query::parse("is:starred").matches(&h("A", "", true, false, true)));
        assert!(q.matches_words(&h("A", "", true, false, false)));
        assert!(Query::parse("Re:").is_empty());
        let s = Query::parse("subject:FATTURA");
        assert!(s.has_filters() && s.matches(&h("A", "", true, false, false)));
        assert!(!Query::parse("subject:ordine").matches(&h("A", "", true, false, false)));
    }
}
