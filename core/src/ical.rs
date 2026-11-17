//! iCalendar (RFC 5545) events: reading VEVENTs and expanding recurring
//! ones into occurrences for a time range.
//!
//! Supported recurrence rules: FREQ=DAILY/WEEKLY/MONTHLY/YEARLY with
//! INTERVAL, COUNT, UNTIL, BYDAY (with ordinals such as 2MO or -1FR),
//! BYMONTHDAY, BYMONTH and BYSETPOS; EXDATE and RECURRENCE-ID overrides.
//! Rules finer than a day (HOURLY, MINUTELY) show the first occurrence only.
//!
//! Times are kept in the event's own time zone while recurring, so a weekly
//! 9:00 meeting stays at 9:00 across daylight-saving changes.

use jiff::civil::{Date, DateTime, Time, Weekday};
use jiff::tz::{AmbiguousOffset, Offset, TimeZone};
use jiff::{SignedDuration, Span, Timestamp, Zoned};

/// A calendar object: the method (REQUEST/REPLY/CANCEL for invitations,
/// empty for stored events) and its events.
#[derive(Debug, Clone, Default)]
pub struct Calendar {
    pub method: String,
    pub events: Vec<Event>,
}

/// Where a time is anchored.
#[derive(Debug, Clone, PartialEq)]
pub enum Zone {
    Utc,
    Named(TimeZone),
    /// No zone: the same wall-clock time everywhere (also all-day dates).
    Floating,
}

/// A start or end time: wall-clock time in a zone.
#[derive(Debug, Clone, PartialEq)]
pub struct When {
    pub local: DateTime,
    pub zone: Zone,
    /// A date without time (all-day events).
    pub date_only: bool,
}

impl When {
    /// Seconds since the epoch. Wall-clock times skipped by a
    /// daylight-saving change move one hour later.
    pub fn timestamp(&self) -> i64 {
        fn resolve(tz: &TimeZone, t: DateTime) -> i64 {
            // The earlier of two repeated times; none for skipped ones.
            let earliest = |t: DateTime| {
                let offset = match tz.to_ambiguous_timestamp(t).offset() {
                    AmbiguousOffset::Unambiguous { offset } => offset,
                    AmbiguousOffset::Fold { before, .. } => before,
                    AmbiguousOffset::Gap { .. } => return None,
                };
                offset.to_timestamp(t).ok()
            };
            earliest(t)
                .or_else(|| earliest(t.checked_add(SignedDuration::from_hours(1)).ok()?))
                .map_or_else(|| utc_seconds(t), |ts| ts.as_second())
        }
        match &self.zone {
            Zone::Utc => utc_seconds(self.local),
            Zone::Named(tz) => resolve(tz, self.local),
            Zone::Floating => resolve(&local_zone(), self.local),
        }
    }

    fn with_local(&self, local: DateTime) -> When {
        When {
            local,
            zone: self.zone.clone(),
            date_only: self.date_only,
        }
    }
}

/// Seconds since the epoch of a wall-clock time read as UTC (clamped to
/// the instants jiff can represent).
fn utc_seconds(t: DateTime) -> i64 {
    Offset::UTC
        .to_timestamp(t)
        .unwrap_or(if t.year() < 0 {
            Timestamp::MIN
        } else {
            Timestamp::MAX
        })
        .as_second()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attendee {
    pub email: String,
    pub name: String,
    /// NEEDS-ACTION, ACCEPTED, DECLINED, TENTATIVE…
    pub partstat: String,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    pub start: When,
    /// End (exclusive), from DTEND or DTSTART + DURATION.
    pub end: When,
    pub rule: Option<Rule>,
    /// Start times (as timestamps) removed from the series.
    pub exdates: Vec<i64>,
    /// For a changed occurrence of a series: the original start.
    pub recurrence_id: Option<i64>,
    /// CONFIRMED, TENTATIVE, CANCELLED (empty if not stated).
    pub status: String,
    /// Organizer's address (without "mailto:").
    pub organizer: String,
    pub organizer_name: String,
    pub attendees: Vec<Attendee>,
    pub sequence: u32,
}

impl Event {
    pub fn all_day(&self) -> bool {
        self.start.date_only
    }
}

/// One occurrence of an event, ready to show.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    /// Start and end as timestamps (seconds since the epoch, UTC).
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    /// All-day events: first and last day (inclusive).
    pub first_day: Option<Date>,
    pub last_day: Option<Date>,
    /// Part of a recurring series.
    pub recurring: bool,
    pub organizer: String,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
    /// HOURLY and finer: not expanded.
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    /// Last allowed start (timestamp).
    pub until: Option<i64>,
    /// (ordinal, weekday); ordinal 0 = every such weekday.
    pub by_day: Vec<(i32, Weekday)>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u32>,
    pub by_set_pos: Vec<i32>,
}

/// Most occurrences produced for one series (and periods scanned): bounds
/// the work for rules that never end.
const MAX_OCCURRENCES: usize = 5000;
const MAX_PERIODS: u32 = 20000;
/// Larger INTERVALs make a rule invalid (see `parse_rule`).
const MAX_INTERVAL: u32 = 10000;
/// Longer DURATIONs are ignored (see `parse_duration`).
const MAX_DURATION_DAYS: i64 = 366 * 100;

/// Runs `f`, turning a panic into `None`. Calendar data comes from other
/// people's mail and servers; a bug in expanding it must never crash the
/// apps (on iOS a panic crossing a synchronous export aborts the app).
pub(crate) fn no_panic<T>(f: impl FnOnce() -> T) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()
}

// ---------------------------------------------------------------- reading

/// A content line: NAME;PARAM=VALUE;...:value
pub(crate) struct Line {
    pub(crate) name: String,
    pub(crate) params: Vec<(String, String)>,
    pub(crate) value: String,
}

impl Line {
    pub(crate) fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Splits a content line, honouring quoted parameter values (which may
/// contain ':' and ';').
pub(crate) fn split_line(line: &str) -> Option<Line> {
    let mut in_quotes = false;
    let mut colon = None;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ':' if !in_quotes => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = Vec::new();
    let mut cur = String::new();
    in_quotes = false;
    for ch in head.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            ';' if !in_quotes => parts.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    parts.push(cur);
    let mut parts = parts.into_iter();
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.trim().to_ascii_uppercase(), v.trim().to_owned()))
        })
        .collect();
    Some(Line {
        name,
        params,
        value: value.to_owned(),
    })
}

/// Reads a calendar object. Components other than VEVENT (time zones,
/// tasks, alarms inside events) are skipped.
pub fn parse(text: &str) -> Calendar {
    let mut cal = Calendar::default();
    // Nesting of BEGIN/END, to read only the event's own properties.
    let mut stack: Vec<String> = Vec::new();
    let mut lines: Vec<Line> = Vec::new();
    for raw in crate::vcard::unfold(text) {
        let Some(line) = split_line(&raw) else { continue };
        match line.name.as_str() {
            "BEGIN" => {
                let what = line.value.trim().to_ascii_uppercase();
                if what == "VEVENT" && !stack.iter().any(|s| s == "VEVENT") {
                    lines.clear();
                }
                stack.push(what);
            }
            "END" => {
                let what = line.value.trim().to_ascii_uppercase();
                if stack.last() == Some(&what) {
                    stack.pop();
                }
                if what == "VEVENT" && !stack.iter().any(|s| s == "VEVENT") {
                    if let Some(ev) = event_from(&lines) {
                        cal.events.push(ev);
                    }
                    lines.clear();
                }
            }
            "METHOD" if stack.len() == 1 => cal.method = line.value.trim().to_ascii_uppercase(),
            _ if stack.last().map(String::as_str) == Some("VEVENT") => lines.push(line),
            _ => {}
        }
    }
    cal
}

fn event_from(lines: &[Line]) -> Option<Event> {
    let mut start = None;
    let mut end = None;
    let mut duration = None;
    let mut ev = Event {
        uid: String::new(),
        summary: String::new(),
        location: String::new(),
        description: String::new(),
        start: When {
            local: DateTime::default(),
            zone: Zone::Floating,
            date_only: false,
        },
        end: When {
            local: DateTime::default(),
            zone: Zone::Floating,
            date_only: false,
        },
        rule: None,
        exdates: Vec::new(),
        recurrence_id: None,
        status: String::new(),
        organizer: String::new(),
        organizer_name: String::new(),
        attendees: Vec::new(),
        sequence: 0,
    };
    let text = |l: &Line| crate::vcard::unescape(&l.value).trim().to_owned();
    for l in lines {
        match l.name.as_str() {
            "UID" => ev.uid = l.value.trim().to_owned(),
            "SUMMARY" => ev.summary = text(l),
            "LOCATION" => ev.location = text(l),
            "DESCRIPTION" => ev.description = text(l),
            "STATUS" => ev.status = l.value.trim().to_ascii_uppercase(),
            "SEQUENCE" => ev.sequence = l.value.trim().parse().unwrap_or(0),
            "DTSTART" => start = parse_when(l),
            "DTEND" => end = parse_when(l),
            "DURATION" => duration = parse_duration(&l.value),
            "RRULE" => ev.rule = parse_rule(&l.value),
            "EXDATE" => {
                for v in l.value.split(',') {
                    let one = Line {
                        name: String::new(),
                        params: l.params.clone(),
                        value: v.to_owned(),
                    };
                    if let Some(w) = parse_when(&one) {
                        ev.exdates.push(w.timestamp());
                    }
                }
            }
            "RECURRENCE-ID" => ev.recurrence_id = parse_when(l).map(|w| w.timestamp()),
            "ORGANIZER" => {
                ev.organizer = mailto(&l.value);
                ev.organizer_name = l.param("CN").unwrap_or_default().trim_matches('"').to_owned();
            }
            "ATTENDEE" => ev.attendees.push(Attendee {
                email: mailto(&l.value),
                name: l.param("CN").unwrap_or_default().trim_matches('"').to_owned(),
                partstat: l.param("PARTSTAT").unwrap_or("NEEDS-ACTION").to_ascii_uppercase(),
            }),
            _ => {}
        }
    }
    ev.start = start?;
    let by_duration = duration.and_then(|d| ev.start.local.checked_add(d).ok());
    ev.end = match (end, by_duration) {
        (Some(e), _) if e.timestamp() >= ev.start.timestamp() => e,
        (_, Some(l)) => ev.start.with_local(l),
        // No end: all-day events last the day, others are instants.
        _ if ev.start.date_only => ev
            .start
            .local
            .add_days(1)
            .map_or_else(|| ev.start.clone(), |l| ev.start.with_local(l)),
        _ => ev.start.clone(),
    };
    if ev.uid.is_empty() {
        ev.uid = format!("{}-{}", ev.start.timestamp(), ev.summary);
    }
    Some(ev)
}

fn mailto(v: &str) -> String {
    let v = v.trim();
    let v = v
        .get(..7)
        .filter(|p| p.eq_ignore_ascii_case("mailto:"))
        .map_or(v, |_| &v[7..]);
    v.trim().to_owned()
}

/// DATE or DATE-TIME, with TZID, UTC ("Z") or floating.
fn parse_when(l: &Line) -> Option<When> {
    let v = l.value.trim();
    let date_only = l.param("VALUE").is_some_and(|x| x.eq_ignore_ascii_case("DATE")) || v.len() == 8;
    if date_only {
        let d = Date::strptime("%Y%m%d", v.get(..8)?).ok()?;
        return Some(When {
            local: d.to_datetime(Time::midnight()),
            zone: Zone::Floating,
            date_only: true,
        });
    }
    let (v, utc) = match v.strip_suffix(['Z', 'z']) {
        Some(rest) => (rest, true),
        None => (v, false),
    };
    let local = DateTime::strptime("%Y%m%dT%H%M%S", v).ok()?;
    let zone = if utc {
        Zone::Utc
    } else {
        l.param("TZID").map_or(Zone::Floating, zone_named)
    };
    Some(When {
        local,
        zone,
        date_only: false,
    })
}

/// IANA names ("Europe/Rome"), names with a vendor prefix
/// ("/mozilla.org/.../Europe/Rome") and the common Windows names used by
/// Outlook. Unknown zones are read as local time.
fn zone_named(tzid: &str) -> Zone {
    let tzid = tzid.trim().trim_matches('"');
    let db = jiff::tz::db();
    if let Ok(tz) = db.get(tzid) {
        return Zone::Named(tz);
    }
    // Vendor prefix: try the last two, then the last three path segments.
    let segs: Vec<&str> = tzid.split('/').filter(|s| !s.is_empty()).collect();
    for n in [2, 3] {
        if segs.len() >= n
            && let Ok(tz) = db.get(&segs[segs.len() - n..].join("/"))
        {
            return Zone::Named(tz);
        }
    }
    const WINDOWS: &[(&str, &str)] = &[
        ("W. Europe Standard Time", "Europe/Berlin"),
        ("Central Europe Standard Time", "Europe/Budapest"),
        ("Romance Standard Time", "Europe/Paris"),
        ("Central European Standard Time", "Europe/Warsaw"),
        ("GMT Standard Time", "Europe/London"),
        ("Greenwich Standard Time", "Atlantic/Reykjavik"),
        ("E. Europe Standard Time", "Europe/Chisinau"),
        ("FLE Standard Time", "Europe/Kiev"),
        ("GTB Standard Time", "Europe/Bucharest"),
        ("Russian Standard Time", "Europe/Moscow"),
        ("Eastern Standard Time", "America/New_York"),
        ("Central Standard Time", "America/Chicago"),
        ("Mountain Standard Time", "America/Denver"),
        ("Pacific Standard Time", "America/Los_Angeles"),
        ("Tokyo Standard Time", "Asia/Tokyo"),
        ("China Standard Time", "Asia/Shanghai"),
        ("Korea Standard Time", "Asia/Seoul"),
        ("India Standard Time", "Asia/Kolkata"),
        ("Arab Standard Time", "Asia/Riyadh"),
        ("Arabian Standard Time", "Asia/Dubai"),
        ("E. South America Standard Time", "America/Sao_Paulo"),
        ("UTC", "UTC"),
    ];
    WINDOWS
        .iter()
        .find(|(w, _)| w.eq_ignore_ascii_case(tzid))
        .and_then(|(_, iana)| db.get(iana).ok())
        .map_or(Zone::Floating, Zone::Named)
}

/// "P1D", "PT1H30M", "P2W", "-PT15M" (negative durations are ignored,
/// and so are absurd ones: they come from other people's mail and would
/// overflow date arithmetic).
fn parse_duration(v: &str) -> Option<SignedDuration> {
    let v = v.trim();
    let v = v.strip_prefix('+').unwrap_or(v);
    if v.starts_with('-') {
        return None;
    }
    let v = v.strip_prefix('P')?;
    let mut total = SignedDuration::ZERO;
    let mut num = String::new();
    let mut time = false;
    for ch in v.chars() {
        match ch {
            '0'..='9' => num.push(ch),
            'T' => time = true,
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                let secs = match (ch, time) {
                    ('W', _) => n.checked_mul(7 * 86400),
                    ('D', _) => n.checked_mul(86400),
                    ('H', true) => n.checked_mul(3600),
                    ('M', true) => n.checked_mul(60),
                    ('S', true) => Some(n),
                    _ => None,
                }?;
                total = total.checked_add(SignedDuration::from_secs(secs))?;
            }
        }
    }
    (total <= SignedDuration::from_hours(MAX_DURATION_DAYS * 24)).then_some(total)
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        "SU" => Weekday::Sunday,
        _ => return None,
    })
}

fn parse_rule(v: &str) -> Option<Rule> {
    let mut r = Rule {
        freq: Freq::Other,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        by_set_pos: Vec::new(),
    };
    let mut has_freq = false;
    let ints = |s: &str| -> Vec<i32> { s.split(',').filter_map(|x| x.trim().parse().ok()).collect() };
    for part in v.trim().split(';') {
        let Some((k, val)) = part.split_once('=') else {
            continue;
        };
        let val = val.trim().to_ascii_uppercase();
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                has_freq = true;
                r.freq = match val.as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => Freq::Other,
                }
            }
            "INTERVAL" => {
                r.interval = val
                    .parse::<u64>()
                    .map_or(1, |n| n.clamp(1, u64::from(MAX_INTERVAL) + 1) as u32)
            }
            "COUNT" => r.count = val.parse().ok(),
            "UNTIL" => {
                let l = Line {
                    name: String::new(),
                    params: Vec::new(),
                    value: val.clone(),
                };
                // A date-only UNTIL includes that whole day.
                r.until = parse_when(&l).map(|w| match w.local.add_days(1) {
                    Some(next) if w.date_only => w.with_local(next).timestamp() - 1,
                    _ => w.timestamp(),
                });
            }
            "BYDAY" => {
                for d in val.split(',') {
                    let d = d.trim();
                    if d.len() < 2 {
                        continue;
                    }
                    let (n, wd) = d.split_at(d.len() - 2);
                    if let Some(wd) = weekday(wd) {
                        r.by_day.push((n.parse().unwrap_or(0), wd));
                    }
                }
            }
            "BYMONTHDAY" => r.by_month_day = ints(&val),
            "BYMONTH" => {
                r.by_month = ints(&val)
                    .into_iter()
                    .filter_map(|m| u32::try_from(m).ok())
                    .collect()
            }
            "BYSETPOS" => r.by_set_pos = ints(&val),
            _ => {}
        }
    }
    // A huge INTERVAL (from someone else's invitation) only overflows the
    // date arithmetic; no real series repeats that rarely.
    (has_freq && r.interval <= MAX_INTERVAL).then_some(r)
}

// ---------------------------------------------------------------- expanding

pub(crate) fn days_in_month(year: i32, month: u32) -> u32 {
    ymd(year, month, 1).map_or(28, |d| d.days_in_month() as u32)
}

/// A date, if it exists.
pub(crate) fn ymd(year: i32, month: u32, day: u32) -> Option<Date> {
    Date::new(
        i16::try_from(year).ok()?,
        i8::try_from(month).ok()?,
        i8::try_from(day).ok()?,
    )
    .ok()
}

/// Moving dates and wall-clock times by whole days.
pub trait AddDays: Sized {
    /// `n` days later (earlier when negative), if still a valid date.
    fn add_days(self, n: i64) -> Option<Self>;
}

impl AddDays for Date {
    fn add_days(self, n: i64) -> Option<Date> {
        self.checked_add(Span::new().try_days(n).ok()?).ok()
    }
}

impl AddDays for DateTime {
    fn add_days(self, n: i64) -> Option<DateTime> {
        self.checked_add(Span::new().try_days(n).ok()?).ok()
    }
}

/// Days of a month (or of a year, with `month` = None) matching BYDAY
/// entries, honouring ordinals.
fn by_day_in(year: i32, month: Option<u32>, by_day: &[(i32, Weekday)]) -> Vec<Date> {
    let days: Vec<Date> = match month {
        Some(m) => (1..=days_in_month(year, m))
            .filter_map(|d| ymd(year, m, d))
            .collect(),
        None => (0..366)
            .filter_map(|n| ymd(year, 1, 1)?.add_days(n))
            .filter(|d| i32::from(d.year()) == year)
            .collect(),
    };
    let mut out = Vec::new();
    for &(n, wd) in by_day {
        let same: Vec<Date> = days.iter().copied().filter(|d| d.weekday() == wd).collect();
        match n {
            0 => out.extend(same),
            n if n > 0 => out.extend(same.get(n as usize - 1)),
            n => out.extend(
                same.len()
                    .checked_sub(n.unsigned_abs() as usize)
                    .and_then(|i| same.get(i)),
            ),
        }
    }
    out
}

fn month_days(year: i32, month: u32, by_month_day: &[i32]) -> Vec<Date> {
    let len = days_in_month(year, month) as i32;
    by_month_day
        .iter()
        .filter_map(|&d| {
            let d = if d < 0 { len + d + 1 } else { d };
            (1..=len)
                .contains(&d)
                .then(|| ymd(year, month, d as u32))
                .flatten()
        })
        .collect()
}

fn add_months(year: i32, month: u32, n: u32) -> Option<(i32, u32)> {
    let total = i64::from(year) * 12 + (i64::from(month) - 1) + i64::from(n);
    Some((
        i32::try_from(total.div_euclid(12)).ok()?,
        (total.rem_euclid(12) + 1) as u32,
    ))
}

/// The dates of the series in period `k` (k-th interval after the start),
/// sorted, before COUNT/UNTIL. Empty when the period falls outside the
/// dates jiff can represent.
fn period_dates(rule: &Rule, first: Date, k: u32) -> Vec<Date> {
    let step = k.saturating_mul(rule.interval);
    let (year, month, day) = (i32::from(first.year()), first.month() as u32, first.day() as u32);
    let in_month = |d: &Date| rule.by_month.is_empty() || rule.by_month.contains(&(d.month() as u32));
    let mut dates: Vec<Date> = match rule.freq {
        Freq::Daily => first
            .add_days(i64::from(step))
            .into_iter()
            .filter(|d| rule.by_day.is_empty() || rule.by_day.iter().any(|(_, w)| *w == d.weekday()))
            .filter(|d| {
                rule.by_month_day.is_empty()
                    || month_days(i32::from(d.year()), d.month() as u32, &rule.by_month_day).contains(d)
            })
            .collect(),
        Freq::Weekly => {
            let Some(monday) = first
                .add_days(-i64::from(first.weekday().to_monday_zero_offset()))
                .and_then(|m| m.add_days(i64::from(step) * 7))
            else {
                return Vec::new();
            };
            let wanted: Vec<Weekday> = if rule.by_day.is_empty() {
                vec![first.weekday()]
            } else {
                rule.by_day.iter().map(|(_, w)| *w).collect()
            };
            (0..7)
                .filter_map(|i| monday.add_days(i))
                .filter(|d| wanted.contains(&d.weekday()))
                .collect()
        }
        Freq::Monthly => {
            let Some((y, m)) = add_months(year, month, step) else {
                return Vec::new();
            };
            if !rule.by_month_day.is_empty() {
                let mut v = month_days(y, m, &rule.by_month_day);
                if !rule.by_day.is_empty() {
                    v.retain(|d| rule.by_day.iter().any(|(_, w)| *w == d.weekday()));
                }
                v
            } else if !rule.by_day.is_empty() {
                by_day_in(y, Some(m), &rule.by_day)
            } else {
                ymd(y, m, day).into_iter().collect()
            }
        }
        Freq::Yearly => {
            let Some(y) = i32::try_from(step).ok().and_then(|s| year.checked_add(s)) else {
                return Vec::new();
            };
            let months: Vec<u32> = if rule.by_month.is_empty() {
                vec![month]
            } else {
                rule.by_month.clone()
            };
            if !rule.by_month_day.is_empty() {
                months
                    .iter()
                    .flat_map(|&m| month_days(y, m, &rule.by_month_day))
                    .collect()
            } else if !rule.by_day.is_empty() {
                if rule.by_month.is_empty() {
                    by_day_in(y, None, &rule.by_day)
                } else {
                    months
                        .iter()
                        .flat_map(|&m| by_day_in(y, Some(m), &rule.by_day))
                        .collect()
                }
            } else {
                months.iter().filter_map(|&m| ymd(y, m, day)).collect()
            }
        }
        Freq::Other => Vec::new(),
    };
    dates.retain(in_month);
    dates.sort();
    dates.dedup();
    if !rule.by_set_pos.is_empty() {
        let n = dates.len() as i32;
        let mut picked: Vec<Date> = rule
            .by_set_pos
            .iter()
            .filter_map(|&p| {
                let i = if p > 0 { p - 1 } else { n + p };
                (0..n).contains(&i).then(|| dates[i as usize])
            })
            .collect();
        picked.sort();
        picked.dedup();
        dates = picked;
    }
    dates
}

/// Start times (in the event's zone) of a series, from the first one,
/// up to `to` (timestamp) or the rule's end.
fn series_starts(ev: &Event, rule: &Rule, to: i64) -> Vec<When> {
    let first = ev.start.local.date();
    let time = ev.start.local.time();
    let start_ts = ev.start.timestamp();
    let mut out = Vec::new();
    // The first occurrence is always DTSTART, even if the rule would not
    // produce it (RFC 5545 3.8.5.3).
    out.push(ev.start.clone());
    if rule.freq == Freq::Other {
        return out;
    }
    let mut count = 1u32;
    'periods: for k in 0..MAX_PERIODS {
        let dates = period_dates(rule, first, k);
        for d in dates {
            let w = ev.start.with_local(d.to_datetime(time));
            let ts = w.timestamp();
            if ts <= start_ts {
                continue;
            }
            if rule.until.is_some_and(|u| ts > u) || ts > to {
                break 'periods;
            }
            if rule.count.is_some_and(|c| count >= c) || out.len() >= MAX_OCCURRENCES {
                break 'periods;
            }
            count += 1;
            out.push(w);
        }
        // Periods past the range: stop even if the rule produced nothing
        // there (e.g. BYMONTHDAY=31 in short months).
        if let Some(last) = period_dates(
            &Rule {
                by_set_pos: Vec::new(),
                ..rule.clone()
            },
            first,
            k,
        )
        .last()
            && ev.start.with_local(last.to_datetime(time)).timestamp() > to
        {
            break;
        }
    }
    out
}

/// Every occurrence of the calendar's events that overlaps `[from, to)`
/// (timestamps), sorted by start. Changed occurrences replace the
/// original ones; cancelled ones and EXDATEs are left out.
pub fn occurrences(cal: &Calendar, from: i64, to: i64) -> Vec<Occurrence> {
    let mut out = Vec::new();
    for ev in &cal.events {
        if ev.recurrence_id.is_some() {
            // An override: shown on its own (unless cancelled).
            if ev.status != "CANCELLED" {
                push_if_overlaps(&mut out, ev, ev.start.clone(), true, from, to);
            }
            continue;
        }
        let Some(rule) = &ev.rule else {
            if ev.status != "CANCELLED" {
                push_if_overlaps(&mut out, ev, ev.start.clone(), false, from, to);
            }
            continue;
        };
        let overridden: Vec<i64> = cal
            .events
            .iter()
            .filter(|o| o.uid == ev.uid)
            .filter_map(|o| o.recurrence_id)
            .collect();
        for start in series_starts(ev, rule, to) {
            let ts = start.timestamp();
            if ev.exdates.contains(&ts) || overridden.contains(&ts) {
                continue;
            }
            push_if_overlaps(&mut out, ev, start, true, from, to);
        }
    }
    out.sort_by_key(|o| (o.start, o.end));
    out
}

fn push_if_overlaps(out: &mut Vec<Occurrence>, ev: &Event, start: When, recurring: bool, from: i64, to: i64) {
    let length = ev.end.local.duration_since(ev.start.local);
    let end = start.with_local(start.local.checked_add(length).unwrap_or(start.local));
    let (s, e) = (start.timestamp(), end.timestamp().max(start.timestamp()));
    // Instants (no length) count when they fall inside the range.
    if s >= to || (e <= from && !(s == e && s >= from)) {
        return;
    }
    let (first_day, last_day) = if ev.all_day() {
        let last = end
            .local
            .add_days(-1)
            .map_or(start.local.date(), |l| l.date().max(start.local.date()));
        (Some(start.local.date()), Some(last))
    } else {
        (None, None)
    };
    out.push(Occurrence {
        uid: ev.uid.clone(),
        summary: ev.summary.clone(),
        location: ev.location.clone(),
        description: ev.description.clone(),
        start: s,
        end: e,
        all_day: ev.all_day(),
        first_day,
        last_day,
        recurring: recurring && (ev.rule.is_some() || ev.recurrence_id.is_some()),
        organizer: ev.organizer.clone(),
        status: ev.status.clone(),
    });
}

/// First start and last end of everything in the calendar (timestamps),
/// to find stored events quickly; `None` as end for series without end.
pub fn span(cal: &Calendar) -> Option<(i64, Option<i64>)> {
    let mut first: Option<i64> = None;
    let mut last: Option<i64> = Some(i64::MIN);
    for ev in &cal.events {
        let s = ev.start.timestamp();
        first = Some(first.map_or(s, |f| f.min(s)));
        let length = ev.end.timestamp() - s;
        let end = match &ev.rule {
            Some(r) if r.freq != Freq::Other && r.count.is_none() && r.until.is_none() => None,
            Some(r) => series_starts(ev, r, i64::MAX)
                .last()
                .map(|w| w.timestamp() + length),
            None => Some(ev.end.timestamp()),
        };
        last = match (last, end) {
            (Some(a), Some(b)) => Some(a.max(b)),
            _ => None,
        };
    }
    first.map(|f| (f, last))
}

/// The month's name in the interface language: "novembre".
fn month_name(d: Date) -> &'static str {
    crate::i18n::t(&format!("month_{}", d.month()))
}

/// The weekday's name in the interface language: "martedì" ("mar" short).
fn weekday_name(d: Date, short: bool) -> &'static str {
    let n = d.weekday().to_monday_one_offset();
    crate::i18n::t(&format!("weekday_{}{n}", if short { "short_" } else { "" }))
}

/// A year as calendars write it: at least four digits.
fn year_text(d: Date) -> String {
    let y = i32::from(d.year());
    if y < 0 {
        format!("-{:04}", -y)
    } else {
        format!("{y:04}")
    }
}

/// A day as a heading in the interface language: "martedì 3 novembre".
pub fn day_label(d: Date) -> String {
    let (weekday, day, month) = (weekday_name(d, false), d.day(), month_name(d));
    let m = d.month();
    let label = match crate::i18n::current().code {
        "en" => format!("{weekday}, {month} {day}"),
        "de" => format!("{weekday}, {day}. {month}"),
        "zh" | "ja" => format!("{m}月{day}日 {weekday}"),
        "ko" => format!("{m}월 {day}일 {weekday}"),
        _ => format!("{weekday} {day} {month}"),
    };
    capitalize(&label)
}

/// A short weekday name for a month grid: "lun", "Mon".
pub fn weekday_short(d: Date) -> String {
    capitalize(weekday_name(d, true))
}

/// The first day of the week where the UI language is mostly spoken.
pub fn week_start() -> Weekday {
    match crate::i18n::current().code {
        "en" | "ja" | "ko" | "pt" => Weekday::Sunday,
        _ => Weekday::Monday,
    }
}

/// A month heading: "novembre 2026".
pub fn month_label(d: Date) -> String {
    let (y, m) = (year_text(d), d.month());
    let label = match crate::i18n::current().code {
        "zh" | "ja" => format!("{y}年{m}月"),
        "ko" => format!("{y}년 {m}월"),
        _ => format!("{} {y}", month_name(d)),
    };
    capitalize(&label)
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// The user's time zone (UTC when the system names none).
pub fn local_zone() -> TimeZone {
    TimeZone::try_system()
        .ok()
        .or_else(|| jiff::tz::db().get(&iana_time_zone::get_timezone().ok()?).ok())
        .unwrap_or(TimeZone::UTC)
}

/// Today's date in the user's time zone.
pub fn today() -> Date {
    Timestamp::now().to_zoned(local_zone()).date()
}

/// Local midnight of a date, as a timestamp.
pub fn day_start(d: Date) -> i64 {
    When {
        local: d.to_datetime(Time::midnight()),
        zone: Zone::Floating,
        date_only: true,
    }
    .timestamp()
}

/// Local date and time of a timestamp.
pub fn local(ts: i64) -> Zoned {
    Timestamp::from_second(ts)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(local_zone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> i64 {
        s.parse::<Timestamp>().unwrap().as_second()
    }

    fn cal(body: &str) -> Calendar {
        parse(&format!(
            "BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{body}END:VCALENDAR\r\n"
        ))
    }

    #[test]
    fn reads_a_simple_event() {
        let c = cal("BEGIN:VEVENT\r\nUID:a@x\r\nSUMMARY:Riunione\\, budget\r\n\
                     DTSTART;TZID=Europe/Rome:20261103T090000\r\nDTEND;TZID=Europe/Rome:20261103T100000\r\n\
                     LOCATION:Sala 2\r\nORGANIZER;CN=\"Rossi: Mario\":mailto:mario@x.it\r\n\
                     ATTENDEE;PARTSTAT=ACCEPTED;CN=Anna:mailto:anna@x.it\r\n\
                     BEGIN:VALARM\r\nDESCRIPTION:Promemoria\r\nTRIGGER:-PT15M\r\nEND:VALARM\r\nEND:VEVENT\r\n");
        assert_eq!(c.events.len(), 1);
        let e = &c.events[0];
        assert_eq!(e.summary, "Riunione, budget");
        assert_eq!(e.description, "", "alarm text is not the event's");
        assert_eq!(e.organizer, "mario@x.it");
        assert_eq!(e.organizer_name, "Rossi: Mario");
        assert_eq!(e.attendees[0].partstat, "ACCEPTED");
        assert_eq!(e.start.timestamp(), ts("2026-11-03T08:00:00Z"));
        let o = occurrences(&c, ts("2026-11-01T00:00:00Z"), ts("2026-11-10T00:00:00Z"));
        assert_eq!(o.len(), 1);
        assert_eq!(o[0].end - o[0].start, 3600);
    }

    #[test]
    fn weekly_series_keeps_wall_clock_time_across_dst() {
        // Every Monday 9:00 Rome; DST ends 2026-10-25.
        let c = cal(
            "BEGIN:VEVENT\r\nUID:w\r\nDTSTART;TZID=Europe/Rome:20261019T090000\r\n\
                     DURATION:PT30M\r\nRRULE:FREQ=WEEKLY;BYDAY=MO;COUNT=3\r\nEND:VEVENT\r\n",
        );
        let o = occurrences(&c, ts("2026-10-01T00:00:00Z"), ts("2027-01-01T00:00:00Z"));
        let starts: Vec<i64> = o.iter().map(|x| x.start).collect();
        assert_eq!(
            starts,
            [
                ts("2026-10-19T07:00:00Z"),
                ts("2026-10-26T08:00:00Z"),
                ts("2026-11-02T08:00:00Z")
            ]
        );
    }

    #[test]
    fn monthly_by_day_exdate_and_override() {
        // Last Friday of each month, one skipped, one moved.
        let c = cal(
            "BEGIN:VEVENT\r\nUID:m\r\nDTSTART:20260130T150000Z\r\nDTEND:20260130T160000Z\r\n\
                     RRULE:FREQ=MONTHLY;BYDAY=-1FR;UNTIL=20260531T000000Z\r\n\
                     EXDATE:20260327T150000Z\r\nEND:VEVENT\r\n\
                     BEGIN:VEVENT\r\nUID:m\r\nRECURRENCE-ID:20260424T150000Z\r\n\
                     DTSTART:20260423T100000Z\r\nDTEND:20260423T110000Z\r\nSUMMARY:Moved\r\nEND:VEVENT\r\n",
        );
        let o = occurrences(&c, ts("2026-01-01T00:00:00Z"), ts("2026-12-31T00:00:00Z"));
        let starts: Vec<i64> = o.iter().map(|x| x.start).collect();
        assert_eq!(
            starts,
            [
                ts("2026-01-30T15:00:00Z"),
                ts("2026-02-27T15:00:00Z"),
                ts("2026-04-23T10:00:00Z"),
                ts("2026-05-29T15:00:00Z"),
            ]
        );
        assert_eq!(o[2].summary, "Moved");
    }

    #[test]
    fn yearly_birthday_all_day_and_endless_span() {
        let c = cal(
            "BEGIN:VEVENT\r\nUID:b\r\nDTSTART;VALUE=DATE:19920229\r\nRRULE:FREQ=YEARLY\r\n\
                     SUMMARY:Compleanno\r\nEND:VEVENT\r\n",
        );
        // 29 February only exists in leap years.
        let o = occurrences(&c, ts("2027-01-01T00:00:00Z"), ts("2029-01-01T00:00:00Z"));
        assert_eq!(o.len(), 1);
        assert!(o[0].all_day);
        assert_eq!(o[0].first_day, ymd(2028, 2, 29));
        assert_eq!(o[0].last_day, o[0].first_day);
        assert!(span(&c).unwrap().1.is_none(), "no end");
    }

    #[test]
    fn monthly_set_pos_and_windows_zone() {
        // Last weekday of the month, Outlook zone name.
        let c = cal(
            "BEGIN:VEVENT\r\nUID:s\r\nDTSTART;TZID=W. Europe Standard Time:20260130T080000\r\n\
                     RRULE:FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=3\r\nEND:VEVENT\r\n",
        );
        assert!(matches!(c.events[0].start.zone, Zone::Named(_)));
        let o = occurrences(&c, ts("2026-01-01T00:00:00Z"), ts("2026-12-31T00:00:00Z"));
        let days: Vec<i8> = o
            .iter()
            .map(|x| local(x.start).in_tz("Europe/Berlin").unwrap().day())
            .collect();
        // 30 Jan (Fri), 27 Feb (Fri), 31 Mar (Tue).
        assert_eq!(days, [30, 27, 31]);
        assert_eq!(span(&c).unwrap().1, Some(o[2].end));
    }

    #[test]
    fn absurd_values_do_not_panic() {
        // A duration past the end of the calendar is ignored.
        let c = cal(
            "BEGIN:VEVENT\r\nUID:d\r\nDTSTART:20261110T100000Z\r\nDURATION:P100000000D\r\nEND:VEVENT\r\n",
        );
        assert_eq!(c.events[0].end, c.events[0].start);
        assert!(parse_duration("P9999999999999999W").is_none());
        // A series repeating every 4 billion weeks is not a series.
        let c = cal(
            "BEGIN:VEVENT\r\nUID:w\r\nDTSTART:20261110T100000Z\r\nRRULE:FREQ=WEEKLY;INTERVAL=4000000000\r\nEND:VEVENT\r\n",
        );
        assert!(c.events[0].rule.is_none());
        assert_eq!(occurrences(&c, ts("2026-01-01T00:00:00Z"), i64::MAX / 4).len(), 1);
        // The largest accepted interval runs off the calendar without panicking.
        for freq in ["DAILY", "WEEKLY", "MONTHLY", "YEARLY"] {
            let c = cal(&format!(
                "BEGIN:VEVENT\r\nUID:m\r\nDTSTART:20261110T100000Z\r\nRRULE:FREQ={freq};INTERVAL=10000\r\nEND:VEVENT\r\n"
            ));
            assert!(!occurrences(&c, ts("2026-01-01T00:00:00Z"), i64::MAX / 4).is_empty());
            let _ = span(&c);
            let rule = c.events[0].rule.clone().unwrap();
            assert!(period_dates(&rule, Date::MAX, 3).is_empty());
            let _ = period_dates(&rule, Date::MIN, u32::MAX);
        }
    }

    #[test]
    fn invitation_method() {
        let c = parse(
            "BEGIN:VCALENDAR\nMETHOD:REQUEST\nBEGIN:VEVENT\nUID:i\nDTSTART:20261110T100000Z\nEND:VEVENT\nEND:VCALENDAR\n",
        );
        assert_eq!(c.method, "REQUEST");
        assert_eq!(
            parse_duration("P1W2DT3H"),
            Some(SignedDuration::from_hours(9 * 24 + 3))
        );
    }
}
