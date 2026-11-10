//! Calendar pane: the month's events as an agenda (grouped by day) in
//! place of the message list, the open event in place of the reader.
//! Events come from the account's CalDAV server (see `ferrowl_core::dav`).

use super::*;
use crate::app::{App, EventField, EventForm, SyncState};
use crate::backend::Backend;
use crate::i18n::t;
use chrono::{Datelike, NaiveDate};
use ferrowl_core::backend::AgendaEntry;
use ferrowl_core::events::Repeat;
use ferrowl_core::ical::{self, day_label, month_label};
use iced::widget::{Column, column, scrollable, space};
use iced::{Alignment, Length::Fill, Padding};

/// Calendar color from the server ("#RRGGBB" or "#RRGGBBAA"), else the
/// theme's accent.
fn color_of(e: &AgendaEntry, p: Palette) -> Color {
    let hex = e.color.trim();
    if hex.len() >= 7 && hex.starts_with('#') {
        crate::theme::hex(&hex[..7])
    } else {
        p.accent
    }
}

/// "09:00 – 10:30", or "All day".
fn time_range(e: &AgendaEntry) -> String {
    let o = &e.occurrence;
    if o.all_day {
        return t("all_day").to_string();
    }
    let (s, end) = (ical::local(o.start), ical::local(o.end));
    if o.end <= o.start {
        s.format("%H:%M").to_string()
    } else if s.date_naive() == end.date_naive() {
        format!("{} – {}", s.format("%H:%M"), end.format("%H:%M"))
    } else {
        format!(
            "{} – {} {}",
            s.format("%H:%M"),
            day_label(end.date_naive()),
            end.format("%H:%M")
        )
    }
}

/// The days of the shown month an occurrence appears on (multi-day events
/// are listed on each day).
fn days_of(e: &AgendaEntry, month: NaiveDate) -> Vec<NaiveDate> {
    let o = &e.occurrence;
    let (first, last) = match (o.first_day, o.last_day) {
        (Some(f), Some(l)) => (f, l),
        // Timed: from the start day to the day of the last second.
        _ => (
            ical::local(o.start).date_naive(),
            ical::local((o.end - 1).max(o.start)).date_naive(),
        ),
    };
    first
        .iter_days()
        .take_while(|d| *d <= last)
        .filter(|d| d.year() == month.year() && d.month() == month.month())
        .take(31)
        .collect()
}

pub fn list<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let syncing = app.calendar_sync == SyncState::Running;
    let nav = |ic: &'static str, tip: &'static str, delta: i32| {
        tooltip(
            button(icon(ic, 18.0, p.text2))
                .padding(8)
                .style(ghost(p, p.text2))
                .on_press(Message::CalendarMonth(delta)),
            tip,
            p,
        )
    };
    // Arrows point the reading direction's way (right-to-left in Arabic).
    let (back, forward) = if crate::i18n::is_rtl() {
        (icons::CHEVRON_RIGHT, icons::CHEVRON_LEFT)
    } else {
        (icons::CHEVRON_LEFT, icons::CHEVRON_RIGHT)
    };
    let header = dir_row(vec![
        text(month_label(app.cal_month))
            .size(20)
            .font(INTER_BOLD)
            .width(Fill)
            .into(),
        nav(back, t("previous_month"), -1),
        button(text(t("today")).size(13).font(medium()))
            .padding([6, 10])
            .style(outline_btn(p, p.text))
            .on_press(Message::CalendarMonth(0))
            .into(),
        nav(forward, t("next_month"), 1),
        tooltip(
            button(icon(icons::PLUS, 18.0, p.text2))
                .padding(8)
                .style(ghost(p, p.text2))
                .on_press(Message::NewEvent),
            t("new_event"),
            p,
        ),
        tooltip(
            button(icon(icons::SYNC, 18.0, p.text2))
                .padding(8)
                .style(ghost(p, p.text2))
                .on_press_maybe((!syncing).then_some(Message::SyncCalendar)),
            t("calendar_sync"),
            p,
        ),
    ])
    .spacing(4)
    .align_y(Alignment::Center)
    .padding(Padding {
        top: 16.0,
        right: 12.0,
        bottom: 8.0,
        left: 20.0,
    });

    let status: Option<Element<'a, Message>> = match &app.calendar_sync {
        SyncState::Running => Some(text(t("calendar_syncing")).size(12).color(p.muted).into()),
        // No calendar server: not an error, events stay on this computer.
        SyncState::Failed(e) if e == t("err_caldav_none") => {
            Some(text(t("calendar_local_note")).size(12).color(p.muted).into())
        }
        SyncState::Failed(e) => Some(text(e.as_str()).size(12).color(p.danger).into()),
        _ => None,
    };

    // Day headings with that day's events under them.
    let today = ical::today();
    let mut days: std::collections::BTreeMap<NaiveDate, Vec<usize>> = Default::default();
    for (i, e) in app.agenda.iter().enumerate() {
        for d in days_of(e, app.cal_month) {
            days.entry(d).or_default().push(i);
        }
    }
    let content: Element<'a, Message> = if days.is_empty() {
        container(text(t("calendar_empty")).size(14).color(p.muted))
            .padding(20)
            .into()
    } else {
        let mut col = Column::new().padding(Padding {
            bottom: 16.0,
            ..Padding::ZERO
        });
        for (day, items) in days {
            let is_today = day == today;
            let mut heading = vec![
                text(day_label(day))
                    .size(13)
                    .font(semibold())
                    .color(if is_today { p.accent_text } else { p.text2 })
                    .into(),
            ];
            if is_today {
                heading.push(
                    container(text(t("today")).size(11).font(semibold()).color(p.on_accent))
                        .padding([1, 8])
                        .style(chip_box(p.accent, p.on_accent, 8.0))
                        .into(),
                );
            }
            col = col.push(
                container(dir_row(heading).spacing(8).align_y(Alignment::Center)).padding(Padding {
                    top: 14.0,
                    right: 20.0,
                    bottom: 4.0,
                    left: 20.0,
                }),
            );
            for i in items {
                col = col.push(event_row(app, i, p));
            }
        }
        scrollable(col).height(Fill).style(scroll_style(p)).into()
    };

    let mut top = Column::new().push(header);
    if let Some(s) = status {
        top = top.push(container(s).padding([0, 20]));
    }
    container(column![top, space().height(4), hline(p.line_soft), content])
        .width(410)
        .height(Fill)
        .style(panel(p.bg))
        .into()
}

fn event_row<'a>(app: &'a App, i: usize, p: Palette) -> Element<'a, Message> {
    let e = &app.agenda[i];
    let active = app.agenda_selected == Some(i);
    let color = color_of(e, p);
    let mut lines = Column::new().spacing(2).width(Fill).push(
        text(trunc(&e.occurrence.summary, 40))
            .size(14)
            .font(semibold())
            .color(p.text),
    );
    let mut meta = time_range(e);
    if !e.occurrence.location.is_empty() {
        meta = format!("{meta} · {}", e.occurrence.location);
    }
    lines = lines.push(text(trunc(&meta, 52)).size(12).color(p.text2));
    button(
        dir_row(vec![
            container(space().width(4).height(34))
                .style(chip_box(color, color, 2.0))
                .into(),
            lines.into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([8, 20])
    .style(nav_item(p, active))
    .on_press(Message::AgendaSelect(i))
    .into()
}

pub fn detail<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    if let Some(f) = &app.event_form {
        return form(app, f, p);
    }
    let Some(e) = app.agenda_selected.and_then(|i| app.agenda.get(i)) else {
        let inner = column![
            icon(icons::CALENDAR, 48.0, p.field),
            text(t("calendar")).size(15).color(p.muted)
        ]
        .spacing(12)
        .align_x(Alignment::Center);
        return container(inner).center(Fill).style(panel(p.bg)).into();
    };
    let o = &e.occurrence;
    let color = color_of(e, p);
    let line = |ic: &'static str, value: String| -> Element<'a, Message> {
        dir_row(vec![
            container(icon(ic, 16.0, p.muted)).padding([2, 0]).into(),
            text(value).size(14).color(p.text).width(Fill).into(),
        ])
        .spacing(12)
        .into()
    };

    let day = match o.first_day {
        Some(d) => d,
        None => ical::local(o.start).date_naive(),
    };
    let mut when = day_label(day);
    if let Some(last) = o.last_day.filter(|l| *l != day) {
        when = format!("{when} – {}", day_label(last));
    }
    when = format!("{when}\n{}", time_range(e));

    let mut body = Column::new().spacing(14).push(
        dir_row(vec![
            container(space().width(6).height(30))
                .style(chip_box(color, color, 3.0))
                .into(),
            text(o.summary.as_str())
                .size(22)
                .font(INTER_BOLD)
                .color(p.text)
                .width(Fill)
                .into(),
        ])
        .spacing(14)
        .align_y(Alignment::Center),
    );
    body = body.push(space().height(4)).push(line(icons::CLOCK, when));
    if o.recurring {
        body = body.push(line(icons::REPEAT, t("event_repeats").to_string()));
    }
    if !o.location.is_empty() {
        body = body.push(line(icons::PIN, o.location.clone()));
    }
    if !e.calendar.is_empty() {
        body = body.push(line(icons::CALENDAR, calendar_name(&e.calendar_url, &e.calendar)));
    }
    if !o.organizer.is_empty() {
        body = body.push(line(icons::MAIL, format!("{}: {}", t("organizer"), o.organizer)));
    }
    if !o.description.trim().is_empty() {
        body = body.push(space().height(4)).push(
            container(text(o.description.as_str()).size(14).color(p.text2))
                .padding(14)
                .width(Fill)
                .style(chip_box(p.chip, p.text2, 10.0)),
        );
    }

    // Edit / delete (asks first).
    let actions: Element<'a, Message> = if app.confirm_event_delete {
        column![
            text(t("event_delete_confirm")).size(14).color(p.text),
            dir_row(vec![
                button(text(t("delete")).size(13).font(semibold()))
                    .padding([7, 14])
                    .style(move |_, st| button::Style {
                        background: Some(Background::Color(if matches!(st, button::Status::Hovered) {
                            mix(p.danger, Color::BLACK, 0.15)
                        } else {
                            p.danger
                        })),
                        text_color: Color::WHITE,
                        border: Border {
                            radius: radius(8.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .on_press(Message::DeleteEvent(true))
                    .into(),
                button(text(t("cancel")).size(13).font(medium()))
                    .padding([7, 14])
                    .style(outline_btn(p, p.text))
                    .on_press(Message::CancelEvent)
                    .into(),
            ])
            .spacing(8),
        ]
        .spacing(10)
        .into()
    } else {
        dir_row(vec![
            button(text(t("event_edit")).size(13).font(medium()))
                .padding([7, 14])
                .style(outline_btn(p, p.text))
                .on_press(Message::EditEvent)
                .into(),
            button(text(t("delete")).size(13).font(medium()))
                .padding([7, 14])
                .style(outline_btn(p, p.danger))
                .on_press(Message::DeleteEvent(false))
                .into(),
        ])
        .spacing(8)
        .into()
    };
    body = body.push(space().height(8)).push(actions);

    container(scrollable(container(body).padding(32).max_width(720)).style(scroll_style(p)))
        .width(Fill)
        .height(Fill)
        .style(panel(p.bg))
        .into()
}

/// A calendar's name; the one kept on the device is "On this computer"
/// here (the engine names it for the phones).
fn calendar_name(url: &str, name: &str) -> String {
    if url == ferrowl_core::cache::LOCAL_CALENDAR {
        t("calendar_local_desktop").to_string()
    } else {
        name.to_string()
    }
}

/// A repeat choice in the picker, with its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RepeatChoice(Repeat);

impl std::fmt::Display for RepeatChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            Repeat::Never => t("repeat_never"),
            Repeat::Daily => t("repeat_daily"),
            Repeat::Weekly => t("repeat_weekly"),
            Repeat::Monthly => t("repeat_monthly"),
            Repeat::Yearly => t("repeat_yearly"),
            Repeat::Custom => t("repeat_custom"),
        })
    }
}

/// A calendar in the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CalendarChoice {
    url: String,
    name: String,
}

impl std::fmt::Display for CalendarChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

/// "Martedì 15 dicembre 2026": the date on the event form's date buttons.
fn date_label(d: NaiveDate) -> String {
    format!("{} {}", day_label(d), d.year())
}

/// A month of days to pick one from: the event form's date picker.
fn date_picker<'a>(month: NaiveDate, chosen: Option<NaiveDate>, p: Palette) -> Element<'a, Message> {
    const CELL: f32 = 36.0;
    let (back, forward) = if crate::i18n::is_rtl() {
        (icons::CHEVRON_RIGHT, icons::CHEVRON_LEFT)
    } else {
        (icons::CHEVRON_LEFT, icons::CHEVRON_RIGHT)
    };
    let nav = |ic: &'static str, delta: i32| -> Element<'a, Message> {
        button(icon(ic, 16.0, p.text2))
            .padding(6)
            .style(ghost(p, p.text2))
            .on_press(Message::EventPickerMonth(delta))
            .into()
    };
    let header = dir_row(vec![
        text(month_label(month))
            .size(14)
            .font(semibold())
            .color(p.text)
            .width(Fill)
            .into(),
        nav(back, -1),
        nav(forward, 1),
    ])
    .width(7.0 * CELL + 6.0 * 2.0)
    .align_y(Alignment::Center);
    let first = month.with_day(1).unwrap_or(month);
    let lead = (first.weekday().num_days_from_monday() + 7 - ical::week_start().num_days_from_monday()) % 7;
    let grid_start = first - chrono::Duration::days(lead as i64);
    let names = dir_row(
        (0..7)
            .map(|i| {
                let d = grid_start + chrono::Duration::days(i);
                container(text(ical::weekday_short(d)).size(11).color(p.muted))
                    .center_x(CELL)
                    .into()
            })
            .collect(),
    )
    .spacing(2);
    let today = ical::today();
    let mut weeks = Column::new().spacing(2).push(names);
    for w in 0..6 {
        let week_first = grid_start + chrono::Duration::days(w * 7);
        if w > 0 && week_first.month() != first.month() {
            break;
        }
        let days: Vec<Element<'a, Message>> = (0..7)
            .map(|i| {
                let d = week_first + chrono::Duration::days(i);
                let picked = chosen == Some(d);
                let is_today = d == today;
                let color = if picked {
                    p.on_accent
                } else if d.month() != first.month() {
                    p.muted
                } else if is_today {
                    p.accent_text
                } else {
                    p.text
                };
                let label = text(d.day().to_string())
                    .size(13)
                    .font(if picked || is_today { semibold() } else { medium() })
                    .color(color);
                button(container(label).center_x(Fill).center_y(Fill))
                    .width(CELL)
                    .height(32)
                    .padding(0)
                    .style(move |th, st| {
                        if picked {
                            accent_btn(p)(th, st)
                        } else {
                            let mut s = ghost(p, color)(th, st);
                            if is_today {
                                s.border = iced::Border {
                                    color: p.accent,
                                    width: 1.0,
                                    radius: 8.0.into(),
                                };
                            }
                            s
                        }
                    })
                    .on_press(Message::EventPickDate(d))
                    .into()
            })
            .collect();
        weeks = weeks.push(dir_row(days).spacing(2));
    }
    container(column![header, weeks].spacing(8))
        .padding(12)
        .style(card(p, p.line, 12.0))
        .into()
}

/// New event, or changes to the open one.
fn form<'a>(app: &'a App, f: &'a EventForm, p: Palette) -> Element<'a, Message> {
    let label = |s: &'static str| text(s).size(13).font(medium()).color(p.text2);
    let input = |placeholder: &'static str, value: &'a str, field: EventField, width: f32| {
        text_input(placeholder, value)
            .on_input(move |v| Message::EventField(field, v))
            .on_submit(Message::SaveEvent)
            .size(14)
            .padding([9, 12])
            .width(width)
            .style(super::input(p, 8.0))
    };
    let when = |date: &'a str, time: &'a str, df: EventField, tf_: EventField| -> Element<'a, Message> {
        let open = f.picker.is_some_and(|(field, _)| field == df);
        let shown = NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map(date_label)
            .unwrap_or_else(|_| date.to_string());
        let date_btn = button(
            dir_row(vec![
                icon(icons::CALENDAR, 15.0, p.text2),
                text(shown).size(14).color(p.text).into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([9, 12])
        .width(230)
        .style(move |th, st| {
            let mut s = outline_btn(p, p.text)(th, st);
            if open {
                s.border.color = p.accent;
            }
            s
        })
        .on_press(Message::EventPicker(df));
        let mut r = vec![date_btn.into()];
        if !f.all_day {
            r.push(input(t("event_time_placeholder"), time, tf_, 90.0).into());
        }
        let row: Element<'a, Message> = dir_row(r).spacing(8).into();
        match f.picker.filter(|(field, _)| *field == df) {
            Some((_, month)) => {
                let chosen = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok();
                column![row, date_picker(month, chosen, p)].spacing(6).into()
            }
            None => row,
        }
    };

    let mut col = Column::new()
        .spacing(10)
        .push(
            text(if f.url.is_some() {
                t("edit_event")
            } else {
                t("new_event")
            })
            .size(22)
            .font(INTER_BOLD)
            .color(p.text),
        )
        .push(space().height(4))
        .push(label(t("event_title")))
        .push(
            text_input(t("event_title"), &f.summary)
                .on_input(|v| Message::EventField(EventField::Summary, v))
                .on_submit(Message::SaveEvent)
                .size(16)
                .padding([10, 12])
                .style(super::input(p, 8.0)),
        )
        .push(
            iced::widget::checkbox(f.all_day)
                .label(t("all_day"))
                .on_toggle(Message::EventAllDay)
                .size(16)
                .text_size(14),
        )
        .push(
            dir_row(vec![
                column![
                    label(t("event_starts")),
                    when(
                        &f.start_date,
                        &f.start_time,
                        EventField::StartDate,
                        EventField::StartTime
                    )
                ]
                .spacing(6)
                .into(),
                column![
                    label(t("event_ends")),
                    when(&f.end_date, &f.end_time, EventField::EndDate, EventField::EndTime)
                ]
                .spacing(6)
                .into(),
            ])
            .spacing(24)
            .wrap(),
        );

    let mut repeats = vec![
        RepeatChoice(Repeat::Never),
        RepeatChoice(Repeat::Daily),
        RepeatChoice(Repeat::Weekly),
        RepeatChoice(Repeat::Monthly),
        RepeatChoice(Repeat::Yearly),
    ];
    if f.repeat == Repeat::Custom {
        repeats.push(RepeatChoice(Repeat::Custom));
    }
    let mut pickers: Vec<Element<'a, Message>> = vec![
        column![
            label(t("repeat")),
            iced::widget::pick_list(repeats, Some(RepeatChoice(f.repeat)), |c: RepeatChoice| {
                Message::EventRepeat(c.0)
            })
            .text_size(14)
            .padding([8, 12])
            .width(220)
            .style(pick_style(p))
            .menu_style(menu_style(p)),
        ]
        .spacing(6)
        .into(),
    ];
    // Calendar choice for a new event, when there is more than one.
    if f.url.is_none() {
        let calendars: Vec<CalendarChoice> = app
            .config
            .account()
            .map(Backend::calendars)
            .unwrap_or_default()
            .into_iter()
            .map(|c| CalendarChoice {
                name: calendar_name(&c.url, &c.name),
                url: c.url,
            })
            .collect();
        if calendars.len() > 1 {
            let chosen = calendars.iter().find(|c| c.url == f.calendar).cloned();
            pickers.push(
                column![
                    label(t("event_calendar")),
                    iced::widget::pick_list(calendars, chosen, |c: CalendarChoice| Message::EventCalendar(
                        c.url
                    ))
                    .text_size(14)
                    .padding([8, 12])
                    .width(220)
                    .style(pick_style(p))
                    .menu_style(menu_style(p)),
                ]
                .spacing(6)
                .into(),
            );
        }
    }
    col = col
        .push(dir_row(pickers).spacing(24).wrap())
        .push(label(t("event_location")))
        .push(
            text_input(t("event_location"), &f.location)
                .on_input(|v| Message::EventField(EventField::Location, v))
                .size(14)
                .padding([9, 12])
                .style(super::input(p, 8.0)),
        )
        .push(label(t("event_notes")))
        .push(
            iced::widget::text_editor(&f.notes)
                .on_action(Message::EventNotes)
                .height(120)
                .size(14)
                .padding(10),
        );
    if let Some(e) = &f.error {
        col = col.push(text(e.as_str()).size(13).color(p.danger));
    }
    col = col.push(space().height(4)).push(
        dir_row(vec![
            button(text(t("event_save")).size(14).font(semibold()))
                .padding([9, 18])
                .style(accent_btn(p))
                .on_press_maybe((!f.saving && !f.summary.trim().is_empty()).then_some(Message::SaveEvent))
                .into(),
            button(text(t("cancel")).size(14).font(medium()))
                .padding([9, 18])
                .style(outline_btn(p, p.text))
                .on_press(Message::CancelEvent)
                .into(),
        ])
        .spacing(10),
    );

    container(scrollable(container(col).padding(32).max_width(720)).style(scroll_style(p)))
        .width(Fill)
        .height(Fill)
        .style(panel(p.bg))
        .into()
}
