//! The user's rules: "when a message arrives that … then …", the same on
//! every platform. A condition is written as a search ([crate::query]:
//! words, `from:`, `subject:`, `has:attachment`, `is:…`); actions mark,
//! star, move or archive the message, or keep it from being notified.
//!
//! Rules apply to the inbox's new messages once each: the newest message
//! they saw is remembered per account (the first run only remembers where
//! the inbox is, so existing mail is left alone).

use serde::{Deserialize, Serialize};

use crate::mail::Header;
use crate::query::Query;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    MarkRead,
    Star,
    Archive,
    /// Move to a folder (its id, the raw IMAP name).
    MoveTo(String),
    /// No new-mail notification for it.
    Silence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    /// The user's name for it (may be empty: the condition is shown).
    pub name: String,
    /// A search, e.g. `from:newsletter@ has:attachment`.
    pub condition: String,
    pub actions: Vec<Action>,
    pub enabled: bool,
}

impl Rule {
    /// Whether a message meets the condition (an empty one never does).
    pub fn matches(&self, h: &Header) -> bool {
        let q = Query::parse(&self.condition);
        !q.is_empty() && q.matches(h) && q.matches_words(h)
    }
}

/// What the rules do to a batch of messages.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    pub mark_read: Vec<u32>,
    pub star: Vec<u32>,
    pub archive: Vec<u32>,
    /// By target folder.
    pub moves: Vec<(String, Vec<u32>)>,
    /// Set by `Backend::apply_rules` when an action failed: its messages
    /// are left out above, the other actions were still done.
    pub error: Option<String>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.mark_read.is_empty() && self.star.is_empty() && self.archive.is_empty() && self.moves.is_empty()
    }

    /// The messages that leave the folder.
    pub fn removed(&self) -> Vec<u32> {
        let mut out = self.archive.clone();
        for (_, uids) in &self.moves {
            out.extend(uids);
        }
        out
    }
}

/// What enabled rules do to `headers`, in order: every matching rule
/// marks and stars; the first one that moves or archives a message
/// decides where it goes.
pub fn plan(rules: &[Rule], headers: &[Header]) -> Plan {
    let mut plan = Plan::default();
    for h in headers {
        let mut placed = false;
        for rule in rules.iter().filter(|r| r.enabled && r.matches(h)) {
            for action in &rule.actions {
                match action {
                    Action::MarkRead if !h.seen && !plan.mark_read.contains(&h.uid) => {
                        plan.mark_read.push(h.uid)
                    }
                    Action::Star if !h.flagged && !plan.star.contains(&h.uid) => plan.star.push(h.uid),
                    Action::Archive if !placed => {
                        plan.archive.push(h.uid);
                        placed = true;
                    }
                    Action::MoveTo(folder) if !placed => {
                        match plan.moves.iter_mut().find(|(f, _)| f == folder) {
                            Some((_, uids)) => uids.push(h.uid),
                            None => plan.moves.push((folder.clone(), vec![h.uid])),
                        }
                        placed = true;
                    }
                    _ => {}
                }
            }
        }
    }
    plan
}

/// Whether an enabled rule keeps this message from being notified.
pub fn silenced(rules: &[Rule], h: &Header) -> bool {
    rules
        .iter()
        .any(|r| r.enabled && r.actions.contains(&Action::Silence) && r.matches(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(uid: u32, from: &str, subject: &str) -> Header {
        Header {
            uid,
            from: from.into(),
            from_address: format!("{}@x.it", from.to_lowercase()),
            subject: subject.into(),
            ..Default::default()
        }
    }

    fn rule(condition: &str, actions: Vec<Action>) -> Rule {
        Rule {
            id: condition.into(),
            name: String::new(),
            condition: condition.into(),
            actions,
            enabled: true,
        }
    }

    #[test]
    fn plans_marks_and_the_first_move() {
        let rules = vec![
            rule(
                "from:shop",
                vec![Action::MarkRead, Action::MoveTo("Promo".into())],
            ),
            rule("subject:offerta", vec![Action::Archive, Action::Star]),
        ];
        let mail = vec![
            h(1, "Shop", "Offerta"),
            h(2, "Anna", "Offerta"),
            h(3, "Anna", "Ciao"),
        ];
        let p = plan(&rules, &mail);
        assert_eq!(p.mark_read, vec![1]);
        assert_eq!(p.star, vec![1, 2]);
        // Message 1 was moved by the first rule: not archived too.
        assert_eq!(p.moves, vec![("Promo".to_string(), vec![1])]);
        assert_eq!(p.archive, vec![2]);
        assert_eq!(p.removed(), vec![2, 1]);
    }

    #[test]
    fn empty_disabled_and_silence() {
        let mut off = rule("from:anna", vec![Action::Archive]);
        off.enabled = false;
        let rules = vec![
            rule("", vec![Action::Archive]),
            off,
            rule("from:news", vec![Action::Silence]),
        ];
        assert!(plan(&rules, &[h(1, "Anna", "x"), h(2, "News", "y")]).is_empty());
        assert!(silenced(&rules, &h(2, "News", "y")));
        assert!(!silenced(&rules, &h(1, "Anna", "x")));
    }

    #[test]
    fn words_match_subject_and_sender() {
        let r = rule("fattura", vec![Action::Star]);
        assert!(r.matches(&h(1, "Anna", "La fattura di marzo")));
        assert!(!r.matches(&h(2, "Anna", "Ciao")));
    }
}
