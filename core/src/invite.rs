//! Meeting invitations by email (iMIP, RFC 6047; iTIP, RFC 5546).
//!
//! An invitation is a message with a text/calendar part whose METHOD is
//! REQUEST (or CANCEL when the organizer calls the event off). Answering
//! sends the organizer a METHOD:REPLY with our participation status, and,
//! when the account has a calendar server, stores the event there (or
//! removes it, when declined or cancelled).

use crate::config::Account;
use crate::dav::{self, Client, Kind};
use crate::i18n::tf;
use crate::ical::{self, Attendee, Occurrence};
use crate::mail::Outgoing;
use anyhow::{Result, bail};

/// The answers offered to an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    Accepted,
    Tentative,
    Declined,
}

impl Reply {
    /// The iCalendar PARTSTAT value.
    pub fn partstat(self) -> &'static str {
        match self {
            Reply::Accepted => "ACCEPTED",
            Reply::Tentative => "TENTATIVE",
            Reply::Declined => "DECLINED",
        }
    }

    pub fn from_partstat(s: &str) -> Option<Reply> {
        match s.trim().to_ascii_uppercase().as_str() {
            "ACCEPTED" => Some(Reply::Accepted),
            "TENTATIVE" => Some(Reply::Tentative),
            "DECLINED" => Some(Reply::Declined),
            _ => None,
        }
    }
}

/// What a message's calendar part says, for the invitation card.
#[derive(Debug, Clone, PartialEq)]
pub struct Invitation {
    /// REQUEST (an invitation), CANCEL (called off) or REPLY (an answer
    /// to an invitation we sent).
    pub method: String,
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    /// When (the first occurrence for a series).
    pub when: Occurrence,
    pub organizer: String,
    pub organizer_name: String,
    /// REPLY: who answered, with their status.
    pub replies: Vec<Attendee>,
}

/// Reads the calendar part of a message. `None` if it is not an
/// invitation, a cancellation or a reply.
pub fn read(ics: &str) -> Option<Invitation> {
    ical::no_panic(|| read_calendar(ics)).flatten()
}

fn read_calendar(ics: &str) -> Option<Invitation> {
    let cal = ical::parse(ics);
    if !matches!(cal.method.as_str(), "REQUEST" | "CANCEL" | "REPLY") {
        return None;
    }
    // The series itself, else the first changed occurrence.
    let ev = cal
        .events
        .iter()
        .find(|e| e.recurrence_id.is_none())
        .or_else(|| cal.events.first())?;
    let single = ical::Calendar {
        method: String::new(),
        events: vec![ical::Event {
            rule: None,
            recurrence_id: None,
            status: String::new(),
            ..ev.clone()
        }],
    };
    let mut when = ical::occurrences(&single, i64::MIN / 4, i64::MAX / 4)
        .into_iter()
        .next()?;
    when.recurring = ev.rule.is_some();
    Some(Invitation {
        method: cal.method.clone(),
        uid: ev.uid.clone(),
        summary: ev.summary.clone(),
        location: ev.location.clone(),
        description: ev.description.clone(),
        when,
        organizer: ev.organizer.clone(),
        organizer_name: ev.organizer_name.clone(),
        replies: if cal.method == "REPLY" {
            ev.attendees.clone()
        } else {
            Vec::new()
        },
    })
}

/// The message to the organizer answering an invitation: a short text and
/// a METHOD:REPLY calendar object with our status.
pub fn reply_mail(account: &Account, ics: &str, reply: Reply) -> Result<Outgoing> {
    let Some(inv) = read(ics).filter(|i| i.method == "REQUEST") else {
        bail!("not an invitation");
    };
    if inv.organizer.is_empty() {
        bail!("{}", crate::i18n::t("err_invite_no_organizer"));
    }
    let who = if account.display_name.trim().is_empty() {
        account.email.clone()
    } else {
        account.display_name.trim().to_owned()
    };
    let (subject, body) = match reply {
        Reply::Accepted => ("invite_subject_accepted", "invite_reply_accepted"),
        Reply::Tentative => ("invite_subject_tentative", "invite_reply_tentative"),
        Reply::Declined => ("invite_subject_declined", "invite_reply_declined"),
    };
    Ok(Outgoing {
        to: if inv.organizer_name.is_empty() {
            inv.organizer.clone()
        } else {
            // Separators would split the address list when sending.
            format!(
                "\"{}\" <{}>",
                inv.organizer_name.replace(['"', ',', ';'], ""),
                inv.organizer
            )
        },
        subject: tf(subject, &[("summary", &inv.summary)]),
        body: format!("{}\n", tf(body, &[("who", &who)])),
        calendar: Some(reply_ics(ics, &account.email, &who, reply)),
        ..Default::default()
    })
}

/// METHOD:REPLY for the events of an invitation: what identifies each
/// event, plus one ATTENDEE (us) with our status. Time zone definitions
/// are copied so the times keep their meaning.
pub fn reply_ics(ics: &str, own: &str, own_name: &str, reply: Reply) -> String {
    const KEEP: &[&str] = &[
        "UID",
        "SEQUENCE",
        "RECURRENCE-ID",
        "DTSTART",
        "DTEND",
        "DURATION",
        "SUMMARY",
        "ORGANIZER",
    ];
    let mut out = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        "PRODID:-//Ferrowl//Ferrowl//EN".to_owned(),
        "METHOD:REPLY".to_owned(),
    ];
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let mut depth: Vec<String> = Vec::new();
    let mut event: Vec<String> = Vec::new();
    for raw in crate::vcard::unfold(ics) {
        let Some(line) = ical::split_line(&raw) else {
            continue;
        };
        let what = line.value.trim().to_ascii_uppercase();
        let in_tz = depth.iter().any(|d| d == "VTIMEZONE");
        match line.name.as_str() {
            "BEGIN" => {
                depth.push(what.clone());
                if what == "VTIMEZONE" || in_tz {
                    out.push(raw);
                } else if what == "VEVENT" {
                    event.clear();
                }
            }
            "END" => {
                if in_tz {
                    out.push(raw);
                } else if what == "VEVENT" && depth.len() == 2 {
                    out.push("BEGIN:VEVENT".to_owned());
                    out.append(&mut event);
                    out.push(format!("DTSTAMP:{stamp}"));
                    out.push(attendee_line(own, own_name, reply));
                    out.push("END:VEVENT".to_owned());
                }
                depth.pop();
            }
            _ if in_tz => out.push(raw),
            // Only the event's own properties (not those of its alarms).
            name if depth.len() == 2 && depth[1] == "VEVENT" && KEEP.contains(&name) => event.push(raw),
            _ => {}
        }
    }
    out.push("END:VCALENDAR".to_owned());
    out.iter().map(|l| fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

/// The invitation as stored in our calendar: without METHOD, and with our
/// own status in our ATTENDEE line.
pub fn calendar_copy(ics: &str, own: &str, reply: Reply) -> String {
    let mut out = Vec::new();
    for raw in crate::vcard::unfold(ics) {
        let Some(line) = ical::split_line(&raw) else {
            out.push(raw);
            continue;
        };
        match line.name.as_str() {
            "METHOD" => {}
            "ATTENDEE" if address_of(&line.value).eq_ignore_ascii_case(own) => {
                let mut params: Vec<(String, String)> = line
                    .params
                    .iter()
                    .filter(|(k, _)| k != "PARTSTAT" && k != "RSVP")
                    .cloned()
                    .collect();
                params.push(("PARTSTAT".into(), reply.partstat().into()));
                out.push(build_line("ATTENDEE", &params, &line.value));
            }
            _ => out.push(raw),
        }
    }
    out.iter().map(|l| fold(l)).collect::<Vec<_>>().join("\r\n") + "\r\n"
}

fn attendee_line(own: &str, name: &str, reply: Reply) -> String {
    let mut params = vec![("PARTSTAT".to_owned(), reply.partstat().to_owned())];
    if !name.is_empty() && name != own {
        params.push(("CN".into(), name.into()));
    }
    build_line("ATTENDEE", &params, &format!("mailto:{own}"))
}

fn build_line(name: &str, params: &[(String, String)], value: &str) -> String {
    let mut s = name.to_owned();
    for (k, v) in params {
        // Values with separators must be quoted (quotes themselves are not
        // allowed inside).
        let v = v.replace('"', "");
        if v.contains([':', ';', ',']) {
            s.push_str(&format!(";{k}=\"{v}\""));
        } else {
            s.push_str(&format!(";{k}={v}"));
        }
    }
    format!("{s}:{value}")
}

fn address_of(value: &str) -> String {
    let v = value.trim();
    v.get(..7)
        .filter(|p| p.eq_ignore_ascii_case("mailto:"))
        .map_or(v, |_| &v[7..])
        .trim()
        .to_owned()
}

/// Folds a content line at 75 octets (RFC 5545 3.1), on character
/// boundaries.
pub(crate) fn fold(line: &str) -> String {
    let mut out = String::with_capacity(line.len() + line.len() / 70 * 3);
    let mut width = 0;
    for ch in line.chars() {
        let len = ch.len_utf8();
        if width + len > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(ch);
        width += len;
    }
    out
}

/// Stores the answered invitation in the account's first calendar
/// (replacing the copy already there, if any), or removes it when
/// declined. `existing` = the stored copy's URL and ETag.
pub fn save_to_calendar(
    account: &Account,
    ics: &str,
    reply: Reply,
    existing: Option<(String, String)>,
) -> Result<()> {
    let client = Client::new(account, Kind::Calendar);
    if reply == Reply::Declined {
        if let Some((url, etag)) = existing {
            client.delete(&url, Some(&etag))?;
        }
        return Ok(());
    }
    let data = calendar_copy(ics, &account.email, reply);
    const ICS: &str = "text/calendar; charset=utf-8";
    match existing {
        Some((url, etag)) => client.put(&url, &data, ICS, Some(&etag)),
        None => {
            let calendars = client.discover(account)?;
            let Some(cal) = calendars.first() else {
                bail!("{}", crate::i18n::t("err_caldav_none"));
            };
            let uid = read(ics).map(|i| i.uid).unwrap_or_default();
            client.put(&item_url(&cal.url, &uid), &data, ICS, None)
        }
    }
}

/// Removes a cancelled event from the calendar.
pub fn remove_from_calendar(account: &Account, url: &str, etag: &str) -> Result<()> {
    Client::new(account, Kind::Calendar).delete(url, Some(etag))
}

/// URL for a new event in a calendar: the UID, reduced to safe characters.
pub(crate) fn item_url(calendar: &str, uid: &str) -> String {
    let mut name: String = uid
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_.".contains(c) {
                c
            } else {
                '-'
            }
        })
        .take(120)
        .collect();
    if name.trim_matches(['-', '.']).is_empty() {
        name = format!("ferrowl-{}", chrono::Utc::now().timestamp_millis());
    }
    let base = if calendar.ends_with('/') {
        calendar.to_owned()
    } else {
        format!("{calendar}/")
    };
    dav::join(&base, &format!("{name}.ics"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nMETHOD:REQUEST\r\n\
BEGIN:VTIMEZONE\r\nTZID:Europe/Rome\r\nBEGIN:STANDARD\r\nDTSTART:19701025T030000\r\nTZOFFSETFROM:+0200\r\nTZOFFSETTO:+0100\r\nEND:STANDARD\r\nEND:VTIMEZONE\r\n\
BEGIN:VEVENT\r\nUID:abc@example.com\r\nSEQUENCE:2\r\nSUMMARY:Revisione progetto\r\n\
DTSTART;TZID=Europe/Rome:20261110T100000\r\nDTEND;TZID=Europe/Rome:20261110T110000\r\n\
LOCATION:Sala 3\r\nORGANIZER;CN=\"Ferri, Giulia\":mailto:giulia@example.com\r\n\
ATTENDEE;CN=Fabio;RSVP=TRUE;PARTSTAT=NEEDS-ACTION:mailto:Me@Example.org\r\n\
ATTENDEE;PARTSTAT=ACCEPTED:mailto:marco@example.com\r\n\
BEGIN:VALARM\r\nTRIGGER:-PT15M\r\nDESCRIPTION:Promemoria\r\nEND:VALARM\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn reads_an_invitation() {
        let inv = read(REQUEST).unwrap();
        assert_eq!(inv.method, "REQUEST");
        assert_eq!(inv.summary, "Revisione progetto");
        assert_eq!(inv.organizer_name, "Ferri, Giulia");
        assert_eq!(inv.when.end - inv.when.start, 3600);
        assert!(
            read("BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:x\nDTSTART:20260101T000000Z\nEND:VEVENT\nEND:VCALENDAR")
                .is_none()
        );
    }

    #[test]
    fn absurd_values_do_not_panic() {
        let huge = REQUEST.replace(
            "DTEND;TZID=Europe/Rome:20261110T110000\r\n",
            "DURATION:P100000000D\r\n",
        );
        let inv = read(&huge).unwrap();
        // The duration is ignored: the invitation is an instant.
        assert_eq!(inv.when.end, inv.when.start);
        let rare = REQUEST.replace("SEQUENCE:2\r\n", "RRULE:FREQ=WEEKLY;INTERVAL=4000000000\r\n");
        assert!(read(&rare).is_some());
    }

    #[test]
    fn builds_the_reply() {
        let r = reply_ics(REQUEST, "me@example.org", "Fabio Carli", Reply::Accepted);
        assert!(r.contains("METHOD:REPLY\r\n"));
        assert!(r.contains("BEGIN:VTIMEZONE\r\nTZID:Europe/Rome"));
        assert!(r.contains("UID:abc@example.com\r\nSEQUENCE:2\r\n"));
        assert!(r.contains("ATTENDEE;PARTSTAT=ACCEPTED;CN=Fabio Carli:mailto:me@example.org"));
        assert!(!r.contains("marco@"), "only our own answer");
        assert!(!r.contains("VALARM") && !r.contains("Promemoria"));
        // The reply reads back as one.
        let back = read(&r).unwrap();
        assert_eq!(back.method, "REPLY");
        assert_eq!(back.replies[0].partstat, "ACCEPTED");
        assert_eq!(back.when.start, read(REQUEST).unwrap().when.start);
    }

    #[test]
    fn calendar_copy_records_our_answer() {
        let c = calendar_copy(REQUEST, "me@example.org", Reply::Tentative);
        assert!(!c.contains("METHOD:"));
        assert!(c.contains("ATTENDEE;CN=Fabio;PARTSTAT=TENTATIVE:mailto:Me@Example.org"));
        assert!(c.contains("ATTENDEE;PARTSTAT=ACCEPTED:mailto:marco@example.com"));
        assert!(c.contains("ORGANIZER;CN=\"Ferri, Giulia\":mailto:giulia@example.com"));
    }

    #[test]
    fn reply_mail_goes_to_the_organizer() {
        let account = Account {
            display_name: "Fabio Carli".into(),
            email: "me@example.org".into(),
            ..Default::default()
        };
        let m = reply_mail(&account, REQUEST, Reply::Declined).unwrap();
        assert_eq!(m.to, "\"Ferri Giulia\" <giulia@example.com>");
        assert!(m.calendar.unwrap().contains("PARTSTAT=DECLINED"));
        assert!(!m.subject.is_empty() && m.subject.contains("Revisione progetto"));
    }

    #[test]
    fn folds_long_lines_and_names_items() {
        let long = format!("DESCRIPTION:{}", "è".repeat(60));
        let f = fold(&long);
        assert!(f.split("\r\n").all(|l| l.len() <= 75));
        assert_eq!(f.replace("\r\n ", ""), long);
        assert_eq!(
            item_url("https://d/cal/work", "abc@example.com"),
            "https://d/cal/work/abc-example.com.ics"
        );
    }
}
