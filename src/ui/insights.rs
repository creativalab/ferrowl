//! What a message is about, picked out of its text by the engine
//! (`ferrowl_core::insights`): a shipment to follow, a code to copy, a bill
//! whose due date can go in the calendar. Shown above the message, like an
//! invitation.

use super::*;
use crate::app::App;
use crate::i18n::{t, tf};
use ferrowl_core::ical::day_label;
use ferrowl_core::insights::Insight;
use iced::widget::{Column, space};
use iced::{Alignment, Length::Fill};

pub fn view<'a>(app: &'a App, uid: u32, p: Palette) -> Option<Element<'a, Message>> {
    let (for_uid, insights) = app.insights.as_ref()?;
    if *for_uid != uid || insights.is_empty() {
        return None;
    }
    let title = |s: String| text(s).size(12).font(semibold()).color(p.accent_text);
    let action = |label: &'static str, msg: Message, primary: bool| -> Element<'a, Message> {
        button(text(label).size(13).font(medium()))
            .padding([7, 14])
            .style(move |th, st| {
                if primary {
                    accent_btn(p)(th, st)
                } else {
                    outline_btn(p, p.text)(th, st)
                }
            })
            .on_press(msg)
            .into()
    };
    let cards = insights.iter().map(|insight| {
        let mut col = Column::new().spacing(8);
        match insight {
            Insight::Shipment { carrier, number, url } => {
                let head = if carrier.is_empty() {
                    t("insight_shipment").to_string()
                } else {
                    format!("{} · {carrier}", t("insight_shipment"))
                };
                let mut buttons = Vec::new();
                if !url.is_empty() {
                    buttons.push(action(t("insight_track"), Message::OpenLink(url.clone()), true));
                }
                buttons.push(action(
                    t("insight_copy"),
                    Message::CopyInsight(number.clone()),
                    false,
                ));
                col = col
                    .push(title(head))
                    .push(
                        text(number.as_str())
                            .size(16)
                            .font(iced::Font::MONOSPACE)
                            .color(p.text),
                    )
                    .push(dir_row(buttons).spacing(8).align_y(Alignment::Center).wrap());
            }
            Insight::Code { code } => {
                col = col
                    .push(title(t("insight_code").to_string()))
                    .push(
                        text(code.as_str())
                            .size(24)
                            .font(iced::Font::MONOSPACE)
                            .color(p.text),
                    )
                    .push(action(
                        t("insight_copy"),
                        Message::CopyInsight(code.clone()),
                        true,
                    ));
            }
            Insight::Payment { number, amount, due } => {
                let head = if number.is_empty() {
                    t("insight_bill").to_string()
                } else {
                    format!("{} · {number}", t("insight_bill"))
                };
                col = col.push(title(head));
                if !amount.is_empty() {
                    col = col.push(text(amount.as_str()).size(18).font(INTER_BOLD).color(p.text));
                }
                if let Some(day) = due {
                    col = col
                        .push(
                            text(tf("insight_due", &[("date", &day_label(*day))]))
                                .size(14)
                                .color(p.text),
                        )
                        .push(space().height(2))
                        .push(action(
                            t("insight_add_due"),
                            Message::AddDue(*day, number.clone(), amount.clone()),
                            false,
                        ));
                }
            }
        }
        container(col)
            .padding(18)
            .width(Fill)
            .style(card(p, p.line, 12.0))
            .into()
    });
    Some(Column::with_children(cards).spacing(12).into())
}
