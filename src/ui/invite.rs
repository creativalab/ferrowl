//! Invitation card in the reader: what, when, where and who for a message
//! carrying a meeting invitation, with Accept / Maybe / Decline; for a
//! cancellation, an offer to remove the event; for a reply to one of our
//! invitations, who answered and how.

use super::*;
use crate::app::App;
use crate::i18n::{t, tf};
use ferrowl_core::ical::{self, day_label};
use ferrowl_core::invite::Reply;
use iced::widget::{Column, column, space};
use iced::{Alignment, Length::Fill};

pub fn view<'a>(app: &'a App, uid: u32, p: Palette) -> Option<Element<'a, Message>> {
    let c = app.invite.as_ref().filter(|c| c.uid == uid)?;
    let inv = &c.invitation;
    let title = match inv.method.as_str() {
        "CANCEL" => t("invite_cancelled_title"),
        "REPLY" => t("invite_reply_title"),
        _ => t("invite_title"),
    };

    // When: day (or days) and time.
    let o = &inv.when;
    let first = o.first_day.unwrap_or_else(|| ical::local(o.start).date());
    let mut when = day_label(first);
    if let Some(last) = o.last_day.filter(|l| *l != first) {
        when = format!("{when} – {}", day_label(last));
    }
    if o.all_day {
        when = format!("{when} · {}", t("all_day"));
    } else {
        let (s, e) = (ical::local(o.start), ical::local(o.end));
        when = format!("{when} · {} – {}", s.strftime("%H:%M"), e.strftime("%H:%M"));
    }

    let line = |ic: &'static str, value: String| -> Element<'a, Message> {
        dir_row(vec![
            container(icon(ic, 15.0, p.muted)).padding([2, 0]).into(),
            text(value).size(14).color(p.text).width(Fill).into(),
        ])
        .spacing(10)
        .into()
    };

    let mut col = Column::new()
        .spacing(8)
        .push(text(title).size(12).font(semibold()).color(p.accent_text))
        .push(text(inv.summary.as_str()).size(18).font(INTER_BOLD).color(p.text))
        .push(line(icons::CLOCK, when));
    if o.recurring {
        col = col.push(line(icons::REPEAT, t("event_repeats").to_string()));
    }
    if !inv.location.is_empty() {
        col = col.push(line(icons::PIN, inv.location.clone()));
    }
    if !inv.organizer.is_empty() && inv.method != "REPLY" {
        let who = if inv.organizer_name.is_empty() {
            inv.organizer.clone()
        } else {
            format!("{} <{}>", inv.organizer_name, inv.organizer)
        };
        col = col.push(line(icons::MAIL, format!("{}: {who}", t("organizer"))));
    }

    match inv.method.as_str() {
        "REQUEST" => {
            let choice = |label: &'static str, reply: Reply| -> Element<'a, Message> {
                let chosen = c.reply == Some(reply);
                button(text(label).size(13).font(medium()))
                    .padding([7, 14])
                    .style(move |th, st| {
                        if chosen {
                            accent_btn(p)(th, st)
                        } else {
                            outline_btn(p, p.text)(th, st)
                        }
                    })
                    .on_press_maybe((!c.busy).then_some(Message::InviteRespond(reply)))
                    .into()
            };
            let mut row_items = vec![
                choice(t("invite_accept"), Reply::Accepted),
                choice(t("invite_tentative"), Reply::Tentative),
                choice(t("invite_decline"), Reply::Declined),
            ];
            if let Some(r) = c.reply {
                let status = match r {
                    Reply::Accepted => t("invite_you_accepted"),
                    Reply::Tentative => t("invite_you_tentative"),
                    Reply::Declined => t("invite_you_declined"),
                };
                row_items.push(text(status).size(13).color(p.text2).into());
            }
            col = col
                .push(space().height(4))
                .push(dir_row(row_items).spacing(8).align_y(Alignment::Center).wrap());
        }
        "CANCEL" if c.in_calendar => {
            col = col.push(space().height(4)).push(
                button(text(t("invite_remove")).size(13).font(medium()))
                    .padding([7, 14])
                    .style(outline_btn(p, p.text))
                    .on_press_maybe((!c.busy).then_some(Message::InviteRemove)),
            );
        }
        "REPLY" => {
            for a in &inv.replies {
                let who = if a.name.is_empty() {
                    a.email.clone()
                } else {
                    a.name.clone()
                };
                let key = match a.partstat.as_str() {
                    "ACCEPTED" => "invite_reply_accepted",
                    "TENTATIVE" => "invite_reply_tentative",
                    "DECLINED" => "invite_reply_declined",
                    _ => continue,
                };
                col = col.push(
                    text(tf(key, &[("who", &who)]))
                        .size(14)
                        .font(medium())
                        .color(p.text),
                );
            }
        }
        _ => {}
    }

    Some(
        container(column![col])
            .padding(18)
            .width(Fill)
            .style(card(p, p.line, 12.0))
            .into(),
    )
}
