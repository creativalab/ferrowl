//! vCard (RFC 6350, and the older 3.0) reading and writing: only what the
//! contacts list needs: name, addresses, phones, organization and note.
//!
//! Unknown properties are ignored; the original text is kept by the caller,
//! so nothing is lost if the contact is written back later.

/// One contact, as shown in the contacts list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Contact {
    /// Where it is stored: the card's URL on the CardDAV server, or
    /// `local:…` on this device (empty for one not saved yet).
    pub url: String,
    /// The vCard UID (may be empty on old cards).
    pub uid: String,
    /// Display name: FN, or built from N, or the first address.
    pub name: String,
    /// Email addresses, preferred first.
    pub emails: Vec<String>,
    pub phones: Vec<String>,
    pub organization: String,
    pub note: String,
}

/// A minimal vCard 3.0 (the version every server accepts) for a new
/// contact: its name and one address.
pub fn build(uid: &str, name: &str, email: &str) -> String {
    let esc = |v: &str| {
        v.replace('\\', "\\\\")
            .replace(',', "\\,")
            .replace(';', "\\;")
            .replace('\n', "\\n")
            .replace('\r', "")
    };
    format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{}\r\nFN:{}\r\nN:{};;;;\r\nEMAIL;TYPE=INTERNET:{}\r\nEND:VCARD\r\n",
        esc(uid),
        esc(name),
        esc(name),
        esc(email)
    )
}

/// A whole vCard 3.0 for a contact written in the app: every address and
/// phone, the organization and the note.
pub fn write(c: &Contact) -> String {
    let esc = |v: &str| {
        v.replace('\\', "\\\\")
            .replace(',', "\\,")
            .replace(';', "\\;")
            .replace('\n', "\\n")
            .replace('\r', "")
    };
    let mut out = format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{}\r\nFN:{}\r\nN:{};;;;\r\n",
        esc(&c.uid),
        esc(&c.name),
        esc(&c.name)
    );
    for (i, email) in c.emails.iter().filter(|e| !e.trim().is_empty()).enumerate() {
        let pref = if i == 0 { ",PREF" } else { "" };
        out += &format!("EMAIL;TYPE=INTERNET{pref}:{}\r\n", esc(email.trim()));
    }
    for phone in c.phones.iter().filter(|p| !p.trim().is_empty()) {
        out += &format!("TEL;TYPE=VOICE:{}\r\n", esc(phone.trim()));
    }
    if !c.organization.trim().is_empty() {
        out += &format!("ORG:{}\r\n", esc(c.organization.trim()));
    }
    if !c.note.trim().is_empty() {
        out += &format!("NOTE:{}\r\n", esc(c.note.trim()));
    }
    out + "END:VCARD\r\n"
}

/// The stored vCard `original` changed to the contact `c` edited in the
/// app: only what the form shows is touched, so everything else (address,
/// birthday, photo, title, a phone's type...) stays as it was on the
/// server. Unchanged fields keep their lines as they are; for addresses
/// and phones, the ones kept keep their lines, removed ones are dropped
/// and new ones added. Falls back to [`write`] without a card to merge.
pub fn merge(original: &str, c: &Contact) -> String {
    let Some(old) = parse(original) else {
        return write(c);
    };
    let esc = |v: &str| {
        v.replace('\\', "\\\\")
            .replace(',', "\\,")
            .replace(';', "\\;")
            .replace('\n', "\\n")
            .replace('\r', "")
    };
    let emails: Vec<&str> = c
        .emails
        .iter()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
        .collect();
    let phones: Vec<&str> = c
        .phones
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    let name_changed = old.name != c.name.trim();
    let org_changed = old.organization != c.organization.trim();
    let note_changed = old.note.trim() != c.note.trim();
    // Each property with its continuation lines, as written.
    let mut props: Vec<Vec<&str>> = Vec::new();
    let mut inside = false;
    for raw in original.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if raw.starts_with([' ', '\t']) {
            if inside && let Some(last) = props.last_mut() {
                last.push(raw);
            }
            continue;
        }
        if raw.is_empty() {
            continue;
        }
        let begin = raw.trim().eq_ignore_ascii_case("BEGIN:VCARD");
        if !inside && !begin {
            continue;
        }
        inside = true;
        props.push(vec![raw]);
        if raw.trim().eq_ignore_ascii_case("END:VCARD") {
            break;
        }
    }
    let mut seen_emails: Vec<String> = Vec::new();
    let mut seen_phones: Vec<String> = Vec::new();
    let mut has_uid = false;
    // Groups ("item1.") whose property was dropped: their labels go too.
    let mut dropped_groups: Vec<String> = Vec::new();
    let mut kept: Vec<(Option<String>, Vec<&str>)> = Vec::new();
    for lines in props {
        let logical: String = lines
            .iter()
            .enumerate()
            .map(|(i, l)| if i == 0 { *l } else { &l[1..] })
            .collect();
        let Some((head, value)) = logical.split_once(':') else {
            continue;
        };
        let full = head.split(';').next().unwrap_or_default();
        let (group, name) = match full.rsplit_once('.') {
            Some((g, n)) => (Some(g.to_ascii_lowercase()), n.to_ascii_uppercase()),
            None => (None, full.to_ascii_uppercase()),
        };
        let keep = match name.as_str() {
            "END" => break,
            "UID" => {
                has_uid = true;
                true
            }
            "FN" | "N" => !name_changed,
            "ORG" => !org_changed,
            "NOTE" => !note_changed,
            "EMAIL" => {
                let addr = unescape(value).trim().to_owned();
                let keep = emails.iter().any(|e| e.eq_ignore_ascii_case(&addr))
                    && !seen_emails.iter().any(|s| s.eq_ignore_ascii_case(&addr));
                if keep {
                    seen_emails.push(addr);
                }
                keep
            }
            "TEL" => {
                let tel = unescape(value).trim().trim_start_matches("tel:").to_owned();
                let keep = phones.contains(&tel.as_str()) && !seen_phones.contains(&tel);
                if keep {
                    seen_phones.push(tel);
                }
                keep
            }
            _ => true,
        };
        if keep {
            kept.push((group, lines));
        } else if let Some(g) = group {
            dropped_groups.push(g);
        }
    }
    let mut out = String::new();
    for (group, lines) in kept {
        if group.is_some_and(|g| dropped_groups.contains(&g)) {
            continue;
        }
        for line in lines {
            out += line;
            out += "\r\n";
        }
    }
    if !has_uid && !c.uid.is_empty() {
        out += &format!("UID:{}\r\n", esc(&c.uid));
    }
    if name_changed {
        out += &format!("FN:{}\r\nN:{};;;;\r\n", esc(c.name.trim()), esc(c.name.trim()));
    }
    // Plain properties: valid in vCard 3.0 and 4.0 alike.
    for email in emails {
        if !seen_emails.iter().any(|s| s.eq_ignore_ascii_case(email)) {
            out += &format!("EMAIL:{}\r\n", esc(email));
        }
    }
    for phone in phones {
        if !seen_phones.iter().any(|s| s == phone) {
            out += &format!("TEL:{}\r\n", esc(phone));
        }
    }
    if org_changed && !c.organization.trim().is_empty() {
        out += &format!("ORG:{}\r\n", esc(c.organization.trim()));
    }
    if note_changed && !c.note.trim().is_empty() {
        out += &format!("NOTE:{}\r\n", esc(c.note.trim()));
    }
    out + "END:VCARD\r\n"
}

/// Reads the first vCard in `text`. Returns `None` if there is no
/// BEGIN:VCARD ... END:VCARD block.
pub fn parse(text: &str) -> Option<Contact> {
    let mut c = Contact::default();
    let mut inside = false;
    let mut structured_name = String::new();
    // (address, preferred) so PREF addresses can be moved first.
    let mut emails: Vec<(String, bool)> = Vec::new();
    for line in unfold(text) {
        let Some((head, value)) = line.split_once(':') else {
            continue;
        };
        let mut parts = head.split(';');
        // Grouped properties ("item1.EMAIL") are read like plain ones.
        let name = parts.next().unwrap_or_default();
        let name = name.rsplit('.').next().unwrap_or(name).to_ascii_uppercase();
        let params: Vec<String> = parts.map(|p| p.to_ascii_uppercase()).collect();
        match name.as_str() {
            "BEGIN" if value.trim().eq_ignore_ascii_case("VCARD") => inside = true,
            "END" if value.trim().eq_ignore_ascii_case("VCARD") => {
                if inside {
                    break;
                }
            }
            _ if !inside => {}
            "UID" => c.uid = value.trim().to_owned(),
            "FN" => c.name = unescape(value),
            "N" => structured_name = name_from_n(value),
            "EMAIL" => {
                let addr = unescape(value).trim().to_owned();
                if addr.contains('@') && !emails.iter().any(|(a, _)| a.eq_ignore_ascii_case(&addr)) {
                    emails.push((addr, is_preferred(&params)));
                }
            }
            "TEL" => {
                let tel = unescape(value).trim().trim_start_matches("tel:").to_owned();
                if !tel.is_empty() && !c.phones.contains(&tel) {
                    c.phones.push(tel);
                }
            }
            "ORG" => {
                c.organization = split_unescaped(value, ';')
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
            "NOTE" => c.note = unescape(value),
            _ => {}
        }
    }
    if !inside {
        return None;
    }
    // Stable sort: preferred addresses first, otherwise file order.
    emails.sort_by_key(|(_, pref)| !pref);
    c.emails = emails.into_iter().map(|(a, _)| a).collect();
    c.name = c.name.trim().to_owned();
    if c.name.is_empty() {
        c.name = if !structured_name.is_empty() {
            structured_name
        } else if !c.organization.is_empty() {
            c.organization.clone()
        } else {
            c.emails.first().cloned().unwrap_or_default()
        };
    }
    Some(c)
}

/// Joins folded lines (a line starting with a space or tab continues the
/// previous one) and drops empty lines.
pub(crate) fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = raw.strip_prefix([' ', '\t'])
            && let Some(last) = lines.last_mut()
        {
            last.push_str(rest);
            continue;
        }
        if !raw.is_empty() {
            lines.push(raw.to_owned());
        }
    }
    lines
}

/// TYPE=pref (3.0) or PREF=1 (4.0).
fn is_preferred(params: &[String]) -> bool {
    params.iter().any(|p| {
        p.starts_with("PREF")
            || p.strip_prefix("TYPE=")
                .is_some_and(|t| t.split(',').any(|t| t.trim_matches('"') == "PREF"))
    })
}

/// "Carli;Fabio;;Dott.;" -> "Dott. Fabio Carli" (prefix, given, additional,
/// family, suffix).
fn name_from_n(value: &str) -> String {
    let f = split_unescaped(value, ';');
    let get = |i: usize| f.get(i).map(String::as_str).unwrap_or_default();
    [get(3), get(1), get(2), get(0), get(4)]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Splits on `sep` except where it is escaped with a backslash, then
/// unescapes each piece.
fn split_unescaped(value: &str, sep: char) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            out.last_mut().unwrap().push(ch);
            if let Some(next) = chars.next() {
                out.last_mut().unwrap().push(next);
            }
        } else if ch == sep {
            out.push(String::new());
        } else {
            out.last_mut().unwrap().push(ch);
        }
    }
    out.iter().map(|s| unescape(s).trim().to_owned()).collect()
}

/// Text value escapes: \n, \, \; \\ .
pub(crate) fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n' | 'N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {

    #[test]
    fn built_cards_read_back() {
        let card = build("1@ferrowl", "Rossi, Ada; Jr", "ada@x.org");
        let c = parse(&card).expect("vcard");
        assert_eq!(c.uid, "1@ferrowl");
        assert_eq!(c.name, "Rossi, Ada; Jr");
        assert_eq!(c.emails, vec!["ada@x.org".to_string()]);
    }

    use super::*;

    #[test]
    fn reads_a_vcard_4() {
        let text = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:urn:uuid:1\r\nFN:Giulia Ferri\r\n\
                    N:Ferri;Giulia;;;\r\nEMAIL;TYPE=work:giulia@lavoro.it\r\n\
                    EMAIL;PREF=1:giulia@casa.it\r\nTEL;TYPE=cell:+39 333 1234567\r\n\
                    ORG:Ferri & Figli\\, S.r.l.;Vendite\r\nNOTE:Prima riga\\nseconda\r\n\
                    END:VCARD\r\n";
        let c = parse(text).unwrap();
        assert_eq!(c.uid, "urn:uuid:1");
        assert_eq!(c.name, "Giulia Ferri");
        assert_eq!(c.emails, ["giulia@casa.it", "giulia@lavoro.it"]);
        assert_eq!(c.phones, ["+39 333 1234567"]);
        assert_eq!(c.organization, "Ferri & Figli, S.r.l., Vendite");
        assert_eq!(c.note, "Prima riga\nseconda");
    }

    #[test]
    fn reads_folded_grouped_3_0_cards() {
        let text = "BEGIN:VCARD\nVERSION:3.0\nN:Rossi;Mario;;Dott.;\n\
                    item1.EMAIL;type=INTERNET;type=pref:mario.rossi@exa\n mple.com\n\
                    item1.X-ABLabel:_$!<Other>!$_\nEND:VCARD\n";
        let c = parse(text).unwrap();
        assert_eq!(c.name, "Dott. Mario Rossi", "built from N without FN");
        assert_eq!(c.emails, ["mario.rossi@example.com"]);
    }

    #[test]
    fn falls_back_to_address_and_rejects_garbage() {
        let c = parse("BEGIN:VCARD\nEMAIL:solo@x.it\nEND:VCARD").unwrap();
        assert_eq!(c.name, "solo@x.it");
        assert!(parse("not a card").is_none());
    }

    #[test]
    fn merged_cards_keep_what_the_form_does_not_show() {
        let original = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:abc\r\nFN:Ada Rossi\r\n\
                        N:Rossi;Ada;;;\r\nTITLE:CTO\r\nBDAY:1990-01-02\r\n\
                        ADR;TYPE=HOME:;;Via Roma 1;Milano;;20100;Italia\r\n\
                        PHOTO;ENCODING=b;TYPE=JPEG:AAAA\r\n BBBB\r\n\
                        EMAIL;TYPE=WORK:ada@lavoro.it\r\n\
                        item1.EMAIL;TYPE=INTERNET:old@x.it\r\nitem1.X-ABLabel:Vecchio\r\n\
                        TEL;TYPE=CELL:+39 333 1\r\nORG:ACME;Vendite\r\nEND:VCARD\r\n";
        let mut c = parse(original).unwrap();
        // Untouched: the same card.
        assert_eq!(merge(original, &c), original);
        // One address removed, one added, a phone added, the note written.
        c.emails = vec!["ada@lavoro.it".into(), "ada@casa.it".into()];
        c.phones.push("+39 02 1".into());
        c.note = "Cliente".into();
        let merged = merge(original, &c);
        for kept in [
            "TITLE:CTO",
            "BDAY:1990-01-02",
            "ADR;TYPE=HOME:;;Via Roma 1;Milano;;20100;Italia",
            "PHOTO;ENCODING=b;TYPE=JPEG:AAAA\r\n BBBB",
            "EMAIL;TYPE=WORK:ada@lavoro.it",
            "TEL;TYPE=CELL:+39 333 1",
            "N:Rossi;Ada;;;",
            "ORG:ACME;Vendite",
        ] {
            assert!(merged.contains(kept), "{kept} lost in {merged}");
        }
        assert!(
            !merged.contains("old@x.it") && !merged.contains("Vecchio"),
            "{merged}"
        );
        let back = parse(&merged).unwrap();
        assert_eq!(back.emails, ["ada@lavoro.it", "ada@casa.it"]);
        assert_eq!(back.phones, ["+39 333 1", "+39 02 1"]);
        assert_eq!(back.note, "Cliente");
        assert_eq!(back.uid, "abc");
        // A new name replaces FN and N only.
        c.name = "Ada Bianchi".into();
        let merged = merge(original, &c);
        assert_eq!(parse(&merged).unwrap().name, "Ada Bianchi");
        assert!(
            !merged.contains("Rossi") && merged.contains("TITLE:CTO"),
            "{merged}"
        );
        assert!(merged.starts_with("BEGIN:VCARD\r\n") && merged.ends_with("END:VCARD\r\n"));
    }

    #[test]
    fn written_cards_read_back() {
        let c = Contact {
            url: String::new(),
            uid: "1@ferrowl".into(),
            name: "Rossi, Mario; srl".into(),
            emails: vec!["m@x.it".into(), "mario@y.it".into()],
            phones: vec!["+39 333 1".into()],
            organization: "ACME".into(),
            note: "riga 1\nriga 2".into(),
        };
        let back = parse(&write(&c)).unwrap();
        assert_eq!(back.name, c.name);
        assert_eq!(back.emails, c.emails);
        assert_eq!(back.phones, c.phones);
        assert_eq!(back.organization, "ACME");
        assert_eq!(back.note, "riga 1\nriga 2");
    }
}
