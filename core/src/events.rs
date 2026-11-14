//! Creating and editing calendar events.
//!
//! A new event is written in the user's time zone (TZID plus a VTIMEZONE
//! built from the time zone database), so recurring events keep their
//! wall-clock time across daylight-saving changes. Editing changes only
//! the fields the form shows; everything else in the stored object
//! (attendees, alarms, changed occurrences, unknown properties) is kept.

use crate::ical::{self, Freq, Zone};
use chrono::{
    DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, Offset, TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;

/// How often the event repeats (the choices offered by the form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Never,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    /// A rule the form cannot show (e.g. "every 2nd Tuesday"): kept as is.
    Custom,
}

/// The event form's content. Times are wall-clock times in the user's
/// time zone; for all-day events only the dates count and `end` is the
/// last day (inclusive).
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub summary: String,
    pub location: String,
    pub description: String,
    pub all_day: bool,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub repeat: Repeat,
}

impl Draft {
    /// A one-hour event starting at the next full hour of `day` (or of
    /// now, for today).
    pub fn new_on(day: NaiveDate) -> Draft {
        let now = Local::now().naive_local();
        let hour = if day == now.date() {
            (now.hour() + 1).min(23)
        } else {
            9
        };
        let start = day.and_hms_opt(hour, 0, 0).unwrap_or_default();
        Draft {
            summary: String::new(),
            location: String::new(),
            description: String::new(),
            all_day: false,
            start,
            end: start + Duration::hours(1),
            repeat: Repeat::Never,
        }
    }

    /// Checks the form: a title, and an end not before the start.
    pub fn is_valid(&self) -> bool {
        !self.summary.trim().is_empty()
            && if self.all_day {
                self.end.date() >= self.start.date()
            } else {
                self.end >= self.start
            }
    }

    /// The form for a stored event (its series, for recurring ones).
    pub fn from_ics(ics: &str) -> Option<Draft> {
        let cal = ical::parse(ics);
        let ev = cal.events.iter().find(|e| e.recurrence_id.is_none())?;
        let local = |ts: i64| {
            DateTime::<Utc>::from_timestamp(ts, 0)
                .unwrap_or_default()
                .with_timezone(&Local)
                .naive_local()
        };
        let (start, end) = if ev.all_day() {
            // DTEND is exclusive: the form shows the last day.
            let last = ev
                .end
                .local
                .checked_sub_days(chrono::Days::new(1))
                .map_or(ev.start.local, |l| l.max(ev.start.local));
            (ev.start.local, last)
        } else {
            (local(ev.start.timestamp()), local(ev.end.timestamp()))
        };
        let repeat = match &ev.rule {
            None => Repeat::Never,
            Some(r) if is_plain(r, ev.start.local.date()) => match r.freq {
                Freq::Daily => Repeat::Daily,
                Freq::Weekly => Repeat::Weekly,
                Freq::Monthly => Repeat::Monthly,
                Freq::Yearly => Repeat::Yearly,
                Freq::Other => Repeat::Custom,
            },
            Some(_) => Repeat::Custom,
        };
        Some(Draft {
            summary: ev.summary.clone(),
            location: ev.location.clone(),
            description: ev.description.clone(),
            all_day: ev.all_day(),
            start,
            end,
            repeat,
        })
    }
}

/// A rule that is just "every day/week/month/year" from the start.
fn is_plain(r: &ical::Rule, start: NaiveDate) -> bool {
    r.interval == 1
        && r.count.is_none()
        && r.until.is_none()
        && r.by_month_day.is_empty()
        && r.by_month.is_empty()
        && r.by_set_pos.is_empty()
        && (r.by_day.is_empty() || (r.freq == Freq::Weekly && r.by_day == [(0, start.weekday())]))
}

fn rrule(repeat: Repeat) -> Option<&'static str> {
    match repeat {
        Repeat::Daily => Some("FREQ=DAILY"),
        Repeat::Weekly => Some("FREQ=WEEKLY"),
        Repeat::Monthly => Some("FREQ=MONTHLY"),
        Repeat::Yearly => Some("FREQ=YEARLY"),
        Repeat::Never | Repeat::Custom => None,
    }
}

/// The user's time zone, if the system names one the database knows.
fn local_zone() -> Option<Tz> {
    iana_time_zone::get_timezone().ok()?.parse().ok()
}

/// DTSTART/DTEND lines for the draft's times.
fn time_lines(d: &Draft, tz: Option<Tz>) -> (String, String) {
    if d.all_day {
        let first = d.start.date();
        let last = d.end.date().max(first);
        let after = last.checked_add_days(chrono::Days::new(1)).unwrap_or(last);
        return (
            format!("DTSTART;VALUE=DATE:{}", first.format("%Y%m%d")),
            format!("DTEND;VALUE=DATE:{}", after.format("%Y%m%d")),
        );
    }
    let end = d.end.max(d.start);
    match tz {
        Some(tz) => (
            format!("DTSTART;TZID={}:{}", tz.name(), d.start.format("%Y%m%dT%H%M%S")),
            format!("DTEND;TZID={}:{}", tz.name(), end.format("%Y%m%dT%H%M%S")),
        ),
        None => {
            // Unknown zone name: exact instants in UTC.
            let utc = |t: NaiveDateTime| {
                ical::When {
                    local: t,
                    zone: Zone::Floating,
                    date_only: false,
                }
                .timestamp()
            };
            let fmt = |ts: i64| {
                DateTime::<Utc>::from_timestamp(ts, 0)
                    .unwrap_or_default()
                    .format("%Y%m%dT%H%M%SZ")
                    .to_string()
            };
            (
                format!("DTSTART:{}", fmt(utc(d.start))),
                format!("DTEND:{}", fmt(utc(end))),
            )
        }
    }
}

/// A VTIMEZONE for `tz` around `year`: standard and daylight time with
/// their yearly change rules (or just the offset, without DST).
fn vtimezone(tz: Tz, year: i32) -> Vec<String> {
    let offset_at = |t: NaiveDateTime| tz.offset_from_utc_datetime(&t).fix().local_minus_utc();
    let fmt_off = |secs: i32| {
        let sign = if secs < 0 { '-' } else { '+' };
        let m = secs.abs() / 60;
        format!("{sign}{:02}{:02}", m / 60, m % 60)
    };
    // Find the offset changes in the year, hour by hour (UTC).
    let mut changes: Vec<(NaiveDateTime, i32, i32)> = Vec::new();
    let Some(mut t) = NaiveDate::from_ymd_opt(year, 1, 1).and_then(|d| d.and_hms_opt(0, 0, 0)) else {
        return Vec::new();
    };
    let end = t + Duration::days(366);
    let mut prev = offset_at(t);
    while t < end {
        t += Duration::hours(1);
        let now = offset_at(t);
        if now != prev {
            // Local wall-clock time of the change, in the old offset.
            changes.push((t + Duration::seconds(prev as i64), prev, now));
            prev = now;
        }
    }
    let mut out = vec!["BEGIN:VTIMEZONE".to_owned(), format!("TZID:{}", tz.name())];
    if changes.is_empty() {
        let off = fmt_off(prev);
        out.extend([
            "BEGIN:STANDARD".to_owned(),
            "DTSTART:19700101T000000".to_owned(),
            format!("TZOFFSETFROM:{off}"),
            format!("TZOFFSETTO:{off}"),
            "END:STANDARD".to_owned(),
        ]);
    } else {
        for (local, from, to) in changes.into_iter().take(2) {
            let kind = if to > from { "DAYLIGHT" } else { "STANDARD" };
            let day = local.date();
            let last_week = day.day() + 7 > crate::ical::days_in_month(day.year(), day.month());
            let nth = if last_week {
                -1
            } else {
                ((day.day() - 1) / 7 + 1) as i32
            };
            let wd =
                ["MO", "TU", "WE", "TH", "FR", "SA", "SU"][day.weekday().num_days_from_monday() as usize];
            out.extend([
                format!("BEGIN:{kind}"),
                format!("DTSTART:{}", local.format("%Y%m%dT%H%M%S")),
                format!("RRULE:FREQ=YEARLY;BYMONTH={};BYDAY={nth}{wd}", day.month()),
                format!("TZOFFSETFROM:{}", fmt_off(from)),
                format!("TZOFFSETTO:{}", fmt_off(to)),
                format!("END:{kind}"),
            ]);
        }
    }
    out.push("END:VTIMEZONE".to_owned());
    out
}

fn escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace("\r\n", "\\n")
        .replace('\n', "\\n")
}

fn stamp() -> String {
    Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// A new event, as a calendar object. Returns (UID, text).
pub fn create(d: &Draft) -> (String, String) {
    let uid = format!("{}@ferrowl", uuid_like());
    let tz = if d.all_day { None } else { local_zone() };
    let (start, end) = time_lines(d, tz);
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        "PRODID:-//Ferrowl//Ferrowl//EN".to_owned(),
    ];
    if let Some(tz) = tz {
        lines.extend(vtimezone(tz, d.start.year()));
    }
    lines.extend([
        "BEGIN:VEVENT".to_owned(),
        format!("UID:{uid}"),
        format!("DTSTAMP:{}", stamp()),
        format!("CREATED:{}", stamp()),
        start,
        end,
        format!("SUMMARY:{}", escape(d.summary.trim())),
    ]);
    if !d.location.trim().is_empty() {
        lines.push(format!("LOCATION:{}", escape(d.location.trim())));
    }
    if !d.description.trim().is_empty() {
        lines.push(format!("DESCRIPTION:{}", escape(d.description.trim())));
    }
    if let Some(rule) = rrule(d.repeat) {
        lines.push(format!("RRULE:{rule}"));
    }
    lines.extend(["END:VEVENT".to_owned(), "END:VCALENDAR".to_owned()]);
    (uid, finish(&lines))
}

/// The stored event with the form's changes. Only the series itself is
/// changed; SEQUENCE goes up, as calendar programs expect.
pub fn update(ics: &str, d: &Draft) -> String {
    const REPLACED: &[&str] = &[
        "SUMMARY",
        "LOCATION",
        "DESCRIPTION",
        "DTSTART",
        "DTEND",
        "DURATION",
        "DTSTAMP",
        "LAST-MODIFIED",
        "SEQUENCE",
    ];
    let tz = if d.all_day { None } else { local_zone() };
    let (start, end) = time_lines(d, tz);
    let mut out: Vec<String> = Vec::new();
    let mut depth: Vec<String> = Vec::new();
    let mut sequence = 0u32;
    let mut has_tz = false;
    // Whether the event being read is the series (not a changed occurrence).
    let mut master = false;
    let mut event: Vec<String> = Vec::new();
    for raw in crate::vcard::unfold(ics) {
        let Some(line) = ical::split_line(&raw) else {
            out.push(raw);
            continue;
        };
        let what = line.value.trim().to_ascii_uppercase();
        let in_event = depth.len() >= 2 && depth[1] == "VEVENT";
        match line.name.as_str() {
            "BEGIN" if what == "VEVENT" && depth.len() == 1 => {
                depth.push(what);
                event = vec![raw];
                master = true;
            }
            "BEGIN" => {
                if what == "VTIMEZONE" && tz.is_some_and(|z| ics.contains(&format!("TZID:{}", z.name()))) {
                    has_tz = true;
                }
                depth.push(what);
                if in_event {
                    event.push(raw);
                } else {
                    out.push(raw);
                }
            }
            "END" if what == "VEVENT" && depth.len() == 2 => {
                depth.pop();
                if master {
                    // The form's fields, right after BEGIN:VEVENT's own
                    // identifying lines.
                    let mut fields = vec![
                        format!("DTSTAMP:{}", stamp()),
                        format!("LAST-MODIFIED:{}", stamp()),
                        format!("SEQUENCE:{}", sequence + 1),
                        start.clone(),
                        end.clone(),
                        format!("SUMMARY:{}", escape(d.summary.trim())),
                    ];
                    if !d.location.trim().is_empty() {
                        fields.push(format!("LOCATION:{}", escape(d.location.trim())));
                    }
                    if !d.description.trim().is_empty() {
                        fields.push(format!("DESCRIPTION:{}", escape(d.description.trim())));
                    }
                    if d.repeat != Repeat::Custom {
                        event.retain(|l| !l.to_ascii_uppercase().starts_with("RRULE"));
                        if let Some(rule) = rrule(d.repeat) {
                            fields.push(format!("RRULE:{rule}"));
                        }
                    }
                    event.splice(1..1, fields);
                }
                out.append(&mut event);
                out.push(raw);
            }
            "END" => {
                depth.pop();
                if in_event && depth.len() >= 2 {
                    event.push(raw);
                } else {
                    out.push(raw);
                }
            }
            "RECURRENCE-ID" if in_event && depth.len() == 2 => {
                master = false;
                event.push(raw);
            }
            "SEQUENCE" if in_event && depth.len() == 2 => {
                sequence = sequence.max(line.value.trim().parse().unwrap_or(0));
                // Kept for changed occurrences, rewritten for the series.
                event.push(raw);
            }
            name if in_event && depth.len() == 2 && REPLACED.contains(&name) => event.push(raw),
            _ if in_event => event.push(raw),
            _ => out.push(raw),
        }
    }
    // Drop the series' old copies of the replaced fields (the new ones
    // were inserted right after BEGIN:VEVENT, so the old ones come later).
    let out = drop_old_fields(out, REPLACED);
    let mut lines = out;
    if let Some(tz) = tz.filter(|_| !has_tz) {
        // Before the first VEVENT.
        let at = lines
            .iter()
            .position(|l| l == "BEGIN:VEVENT")
            .unwrap_or(lines.len());
        lines.splice(at..at, vtimezone(tz, d.start.year()));
    }
    finish(&lines)
}

/// In each VEVENT that has the new fields twice, keeps the first copy.
fn drop_old_fields(lines: Vec<String>, names: &[&str]) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut seen: Vec<String> = Vec::new();
    let mut depth = 0usize;
    for l in lines {
        let upper = l.to_ascii_uppercase();
        if upper == "BEGIN:VEVENT" {
            seen.clear();
            depth = 1;
            out.push(l);
            continue;
        }
        if depth >= 1 {
            if upper.starts_with("BEGIN:") {
                depth += 1;
            } else if upper.starts_with("END:") {
                depth -= 1;
            } else if depth == 1 {
                let name = upper.split([':', ';']).next().unwrap_or_default().to_owned();
                if names.contains(&name.as_str()) {
                    // A changed occurrence keeps its own copy (it has only one).
                    if seen.contains(&name) {
                        continue;
                    }
                    seen.push(name);
                }
            }
        }
        out.push(l);
    }
    out
}

fn finish(lines: &[String]) -> String {
    lines
        .iter()
        .map(|l| crate::invite::fold(l))
        .collect::<Vec<_>>()
        .join("\r\n")
        + "\r\n"
}

/// A random identifier for UIDs and file names.
pub(crate) fn uuid_like() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!("{:016x}{:016x}", rng.r#gen::<u64>(), rng.r#gen::<u64>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(start: &str, end: &str) -> Draft {
        let p = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap();
        Draft {
            summary: "Cena, amici".into(),
            location: "Alba".into(),
            description: "Riga 1\nRiga 2".into(),
            all_day: false,
            start: p(start),
            end: p(end),
            repeat: Repeat::Never,
        }
    }

    #[test]
    fn new_event_reads_back() {
        let d = draft("2026-11-20 20:00", "2026-11-20 22:30");
        let (uid, ics) = create(&d);
        let back = Draft::from_ics(&ics).unwrap();
        assert_eq!(back, d);
        let cal = ical::parse(&ics);
        assert_eq!(cal.events[0].uid, uid);
        assert!(ics.contains("SUMMARY:Cena\\, amici"));
    }

    #[test]
    fn all_day_and_weekly() {
        let mut d = draft("2026-11-21 00:00", "2026-11-22 00:00");
        d.all_day = true;
        d.repeat = Repeat::Weekly;
        let (_, ics) = create(&d);
        assert!(ics.contains("DTSTART;VALUE=DATE:20261121") && ics.contains("DTEND;VALUE=DATE:20261123"));
        let back = Draft::from_ics(&ics).unwrap();
        assert!(back.all_day && back.repeat == Repeat::Weekly);
        assert_eq!(back.end.date(), d.end.date(), "last day shown inclusive");
    }

    #[test]
    fn rome_vtimezone_has_both_rules() {
        let v = vtimezone(chrono_tz::Europe::Rome, 2026).join("\n");
        assert!(
            v.contains("BEGIN:DAYLIGHT") && v.contains("BYMONTH=3;BYDAY=-1SU"),
            "{v}"
        );
        assert!(v.contains("BEGIN:STANDARD") && v.contains("BYMONTH=10;BYDAY=-1SU"));
        assert!(v.contains("TZOFFSETTO:+0200") && v.contains("TZOFFSETTO:+0100"));
        let tokyo = vtimezone(chrono_tz::Asia::Tokyo, 2026).join("\n");
        assert!(tokyo.contains("TZOFFSETTO:+0900") && !tokyo.contains("DAYLIGHT"));
    }

    #[test]
    fn update_keeps_what_the_form_does_not_show() {
        let stored = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Other//EN\r\nBEGIN:VEVENT\r\nUID:keep@x\r\n\
SEQUENCE:3\r\nSUMMARY:Old\r\nDTSTART:20261110T090000Z\r\nDTEND:20261110T100000Z\r\n\
RRULE:FREQ=WEEKLY;INTERVAL=2\r\nATTENDEE;PARTSTAT=ACCEPTED:mailto:a@x.it\r\nX-CUSTOM:yes\r\n\
BEGIN:VALARM\r\nTRIGGER:-PT10M\r\nACTION:DISPLAY\r\nDESCRIPTION:Alarm\r\nEND:VALARM\r\nEND:VEVENT\r\n\
BEGIN:VEVENT\r\nUID:keep@x\r\nRECURRENCE-ID:20261124T090000Z\r\nSUMMARY:Moved\r\n\
DTSTART:20261125T090000Z\r\nDTEND:20261125T100000Z\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let mut d = Draft::from_ics(stored).unwrap();
        assert_eq!(d.repeat, Repeat::Custom, "every 2 weeks is not a form choice");
        d.summary = "New".into();
        let out = update(stored, &d);
        let cal = ical::parse(&out);
        assert_eq!(cal.events.len(), 2);
        let series = &cal.events[0];
        assert_eq!(series.summary, "New");
        assert_eq!(series.sequence, 4);
        assert_eq!(series.rule.as_ref().unwrap().interval, 2, "custom rule kept");
        assert_eq!(series.attendees.len(), 1);
        assert_eq!(series.description, "", "alarm text not taken");
        assert!(out.contains("X-CUSTOM:yes") && out.contains("TRIGGER:-PT10M"));
        assert_eq!(cal.events[1].summary, "Moved", "changed occurrence untouched");
        assert_eq!(out.matches("SUMMARY:").count(), 2);
        assert_eq!(out.matches("\r\nSEQUENCE:").count(), 1);

        // Choosing "never" drops the rule.
        d.repeat = Repeat::Never;
        assert!(ical::parse(&update(stored, &d)).events[0].rule.is_none());
    }
}
