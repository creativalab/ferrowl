//! The folder list as a tree that folds, as webmails show long lists: a
//! folder with subfolders has an arrow and starts closed; the apps remember
//! which ones the user opened.
//!
//! The standard folders (Inbox, Sent, Drafts, Archive, Spam, Trash) come
//! first in that order, always at the top level (also when the server keeps
//! them under a container such as Gmail's `[Gmail]`); the user's folders
//! follow, sorted by name, each under its parent. Levels the server does not
//! list (`INBOX.Sent.2002.11-nov` without `INBOX.Sent.2002`) are added as
//! folders that only open and close.

use std::collections::{HashMap, HashSet};

/// A folder as the tree needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// Path segments (`["Archivio", "2024"]`).
    pub path: Vec<String>,
    /// Position of a standard folder (Inbox 0, …); `None` for the user's.
    pub rank: Option<u8>,
    pub selectable: bool,
}

/// A row to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Index into the entries given; past their end for a level the
    /// server does not list (then see `entry`).
    pub index: usize,
    /// The folder (for a missing level: its path, an id of its own, not selectable).
    pub entry: Entry,
    pub depth: u32,
    pub has_children: bool,
    pub expanded: bool,
    /// Every folder under it (for the unread count of a closed one).
    pub descendants: Vec<String>,
}

/// The rows to show: the folders whose parents are all open, in order.
pub fn rows(given: &[Entry], expanded: &HashSet<String>) -> Vec<Row> {
    // Levels the server does not list, added under the folders that have them.
    let mut entries = given.to_vec();
    let mut known: HashSet<Vec<String>> = given.iter().map(|e| e.path.clone()).collect();
    for e in given.iter().filter(|e| e.rank.is_none()) {
        for k in 1..e.path.len() {
            let prefix = e.path[..k].to_vec();
            if known.insert(prefix.clone()) {
                entries.push(Entry {
                    id: format!("\u{1}{}", prefix.join("\u{1}")),
                    path: prefix,
                    rank: None,
                    selectable: false,
                });
            }
        }
    }
    let entries = entries.as_slice();
    // Servers like Aruba keep every folder of the user under "INBOX."
    // (a prefix, not a choice): then those are top-level folders, not
    // hidden under a closed Inbox.
    let is_inbox = |p: &str| p.eq_ignore_ascii_case("INBOX");
    let users: Vec<&Entry> = given.iter().filter(|e| e.rank.is_none()).collect();
    let inbox_prefix = !users.is_empty() && users.iter().all(|e| e.path.len() >= 2 && is_inbox(&e.path[0]));
    let by_path: HashMap<&[String], usize> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| (e.path.as_slice(), i))
        .collect();
    // Parent: the folder one level up, unless this is a standard folder
    // (always at the top).
    let parent = |i: usize| -> Option<usize> {
        let e = &entries[i];
        if e.rank.is_some() || e.path.len() < 2 || (inbox_prefix && e.path.len() == 2 && is_inbox(&e.path[0]))
        {
            return None;
        }
        by_path.get(&e.path[..e.path.len() - 1]).copied()
    };
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut roots = Vec::new();
    for i in 0..entries.len() {
        match parent(i) {
            Some(p) => children.entry(p).or_default().push(i),
            None => roots.push(i),
        }
    }
    let name = |i: &usize| entries[*i].path.join("/").to_lowercase();
    for list in children.values_mut() {
        list.sort_by_key(name);
    }
    roots.sort_by_key(|i| (entries[*i].rank.unwrap_or(u8::MAX), name(i)));

    fn under(i: usize, children: &HashMap<usize, Vec<usize>>, out: &mut Vec<usize>) {
        for &c in children.get(&i).map(Vec::as_slice).unwrap_or_default() {
            out.push(c);
            under(c, children, out);
        }
    }

    let mut out = Vec::new();
    // Depth-first, skipping what is under a closed folder.
    let mut stack: Vec<(usize, u32)> = roots.iter().rev().map(|&i| (i, 0)).collect();
    while let Some((i, depth)) = stack.pop() {
        let mut all = Vec::new();
        under(i, &children, &mut all);
        // A container that cannot be opened and holds nothing to show.
        if !entries[i].selectable && all.is_empty() {
            continue;
        }
        let open = expanded.contains(&entries[i].id);
        out.push(Row {
            index: i,
            entry: entries[i].clone(),
            depth,
            has_children: !all.is_empty(),
            expanded: open,
            descendants: all.iter().map(|&c| entries[c].id.clone()).collect(),
        });
        if open && let Some(kids) = children.get(&i) {
            for &c in kids.iter().rev() {
                stack.push((c, depth + 1));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(id: &str, rank: Option<u8>) -> Entry {
        Entry {
            id: id.into(),
            path: id.split('/').map(String::from).collect(),
            rank,
            selectable: true,
        }
    }

    fn shown(entries: &[Entry], open: &[&str]) -> Vec<(String, u32)> {
        let open: HashSet<String> = open.iter().map(|s| s.to_string()).collect();
        rows(entries, &open)
            .into_iter()
            .map(|r| (entries[r.index].id.clone(), r.depth))
            .collect()
    }

    #[test]
    fn closed_by_default_open_on_request() {
        let entries = vec![
            e("Archivio/2024/03-mar", None),
            e("Sent", Some(2)),
            e("Archivio", None),
            e("INBOX", Some(0)),
            e("Archivio/2024", None),
            e("Archivio/2023", None),
            e("Templates", None),
        ];
        assert_eq!(
            shown(&entries, &[]),
            [
                ("INBOX".into(), 0),
                ("Sent".into(), 0),
                ("Archivio".into(), 0),
                ("Templates".into(), 0)
            ]
        );
        assert_eq!(
            shown(&entries, &["Archivio"]),
            [
                ("INBOX".into(), 0),
                ("Sent".into(), 0),
                ("Archivio".into(), 0),
                ("Archivio/2023".into(), 1),
                ("Archivio/2024".into(), 1),
                ("Templates".into(), 0),
            ]
        );
        assert_eq!(
            shown(&entries, &["Archivio", "Archivio/2024"])[5],
            ("Archivio/2024/03-mar".into(), 2)
        );
        // A closed folder knows everything under it (for its unread count).
        let rows = rows(&entries, &HashSet::new());
        let archivio = rows.iter().find(|r| entries[r.index].id == "Archivio").unwrap();
        assert!(archivio.has_children && !archivio.expanded);
        assert_eq!(archivio.descendants.len(), 3);
    }

    #[test]
    fn missing_levels_are_added() {
        let entries = vec![
            e("INBOX", Some(0)),
            e("INBOX/Sent", Some(2)),
            e("INBOX/Sent/2002/11-nov", None),
            e("INBOX/Sent/2002/12-dic", None),
        ];
        let shown = |open: &[&str]| -> Vec<(String, u32, bool)> {
            let open: HashSet<String> = open.iter().map(|s| s.to_string()).collect();
            rows(&entries, &open)
                .into_iter()
                .map(|r| (r.entry.path.join("/"), r.depth, r.entry.selectable))
                .collect()
        };
        // Sent holds a "2002" the server does not list: it is added, closed.
        assert_eq!(
            shown(&[]),
            [("INBOX".into(), 0, true), ("INBOX/Sent".into(), 0, true)]
        );
        let sent = rows(&entries, &HashSet::new())[1].clone();
        assert!(sent.has_children);
        let year = "\u{1}INBOX\u{1}Sent\u{1}2002";
        assert_eq!(
            shown(&["INBOX/Sent", year]),
            [
                ("INBOX".into(), 0, true),
                ("INBOX/Sent".into(), 0, true),
                ("INBOX/Sent/2002".into(), 1, false),
                ("INBOX/Sent/2002/11-nov".into(), 2, true),
                ("INBOX/Sent/2002/12-dic".into(), 2, true),
            ]
        );
    }

    #[test]
    fn an_inbox_prefix_is_not_a_parent() {
        // Aruba: every folder of the user is INBOX.something.
        let entries = vec![
            e("INBOX", Some(0)),
            e("INBOX/Sent", Some(2)),
            e("INBOX/Banche", None),
            e("INBOX/Lavoro", None),
            e("INBOX/Lavoro/2024", None),
        ];
        assert_eq!(
            shown(&entries, &[]),
            [
                ("INBOX".into(), 0),
                ("INBOX/Sent".into(), 0),
                ("INBOX/Banche".into(), 0),
                ("INBOX/Lavoro".into(), 0)
            ]
        );
        let inbox = rows(&entries, &HashSet::new())[0].clone();
        assert!(!inbox.has_children);
        // A real subfolder of the inbox (other folders elsewhere) stays under it.
        let mixed = vec![e("INBOX", Some(0)), e("INBOX/Clienti", None), e("Progetti", None)];
        assert_eq!(shown(&mixed, &[]), [("INBOX".into(), 0), ("Progetti".into(), 0)]);
    }

    #[test]
    fn standard_folders_stay_on_top() {
        let mut gmail = e("[Gmail]", None);
        gmail.selectable = false;
        let entries = vec![
            gmail,
            e("[Gmail]/Sent Mail", Some(2)),
            e("[Gmail]/Trash", Some(5)),
            e("INBOX/Lavoro", None),
            e("INBOX", Some(0)),
        ];
        // [Gmail] holds only standard folders: not shown; Inbox has a subfolder.
        assert_eq!(
            shown(&entries, &[]),
            [
                ("INBOX".into(), 0),
                ("[Gmail]/Sent Mail".into(), 0),
                ("[Gmail]/Trash".into(), 0)
            ]
        );
        assert_eq!(shown(&entries, &["INBOX"])[1], ("INBOX/Lavoro".into(), 1));
    }
}
