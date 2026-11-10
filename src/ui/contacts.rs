//! Contacts pane: the list (with search) in place of the message list, the
//! open contact in place of the reader. Contacts come from the account's
//! CardDAV server (see `ferrowl_core::carddav`).

use super::*;
use crate::app::{App, SyncState};
use crate::i18n::{t, tf};
use ferrowl_core::vcard::Contact;
use iced::widget::{Column, column, row, scrollable, space};
use iced::{Alignment, Length::Fill, Padding};

pub fn list<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let syncing = app.contacts_sync == SyncState::Running;
    let sync = tooltip(
        button(icon(icons::SYNC, 18.0, p.text2))
            .padding(8)
            .style(ghost(p, p.text2))
            .on_press_maybe((!syncing).then_some(Message::SyncContacts)),
        t("contacts_sync"),
        p,
    );
    let header = dir_row(vec![
        text(t("contacts")).size(20).font(INTER_BOLD).width(Fill).into(),
        sync,
    ])
    .align_y(Alignment::Center)
    .padding(Padding {
        top: 16.0,
        right: 16.0,
        bottom: 8.0,
        left: 20.0,
    });

    let search = container(
        row![
            icon(icons::SEARCH, 16.0, p.muted),
            text_input(t("contacts_search"), &app.contacts_query)
                .on_input(Message::ContactsQuery)
                .size(14)
                .padding(0)
                .style(bare_input(p)),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([8, 12])
    .style(chip_box(p.chip, p.text2, 9.0));

    // What the last sync did: running, failed (why), or how many contacts.
    let status: Element<'a, Message> = match &app.contacts_sync {
        SyncState::Running => text(t("contacts_syncing")).size(12).color(p.muted).into(),
        // No contacts server: not an error, contacts stay on this computer.
        SyncState::Failed(e) if e == t("err_dav_none") => {
            text(t("contacts_local_note")).size(12).color(p.muted).into()
        }
        SyncState::Failed(e) => text(e.as_str()).size(12).color(p.danger).into(),
        _ => text(tf("contacts_count", &[("n", &app.contacts.len().to_string())]))
            .size(12)
            .font(MONO)
            .color(p.muted)
            .into(),
    };

    let content: Element<'a, Message> = if app.contacts.is_empty() {
        container(text(t("contacts_empty")).size(14).color(p.muted))
            .padding(20)
            .into()
    } else {
        let mut col = Column::new();
        for (i, c) in app.contacts.iter().enumerate() {
            col = col.push(contact_row(app, i, c, p));
        }
        scrollable(col).height(Fill).style(scroll_style(p)).into()
    };

    container(column![
        header,
        container(column![search, status].spacing(8)).padding([0, 16]),
        space().height(8),
        hline(p.line_soft),
        content
    ])
    .width(410)
    .height(Fill)
    .style(panel(p.bg))
    .into()
}

fn contact_row<'a>(app: &App, i: usize, c: &'a Contact, p: Palette) -> Element<'a, Message> {
    let active = app.contact_selected == Some(i);
    let key = c.emails.first().unwrap_or(&c.name);
    let detail = if !c.emails.is_empty() {
        c.emails[0].as_str()
    } else {
        c.phones.first().map(String::as_str).unwrap_or_default()
    };
    button(
        dir_row(vec![
            avatar(initials(&c.name), avatar_color(key), Color::WHITE, 36.0),
            column![
                text(trunc(&c.name, 36)).size(14).font(semibold()).color(p.text),
                text(trunc(detail, 42)).size(13).color(p.text2),
            ]
            .spacing(2)
            .width(Fill)
            .into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([10, 20])
    .style(nav_item(p, active))
    .on_press(Message::ContactSelect(i))
    .into()
}

pub fn detail<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let Some(c) = app.contact_selected.and_then(|i| app.contacts.get(i)) else {
        let inner = column![
            icon(icons::CONTACTS, 48.0, p.field),
            text(t("contacts")).size(15).color(p.muted)
        ]
        .spacing(12)
        .align_x(Alignment::Center);
        return container(inner).center(Fill).style(panel(p.bg)).into();
    };

    let key = c.emails.first().unwrap_or(&c.name);
    let mut head = Column::new()
        .spacing(4)
        .push(text(c.name.as_str()).size(22).font(INTER_BOLD).color(p.text));
    if !c.organization.is_empty() && c.organization != c.name {
        head = head.push(text(c.organization.as_str()).size(14).color(p.text2));
    }
    let top = dir_row(vec![
        avatar(initials(&c.name), avatar_color(key), Color::WHITE, 64.0),
        head.width(Fill).into(),
    ])
    .spacing(18)
    .align_y(Alignment::Center);

    let mut body = Column::new().spacing(10).push(top).push(space().height(12));
    for addr in &c.emails {
        body = body.push(
            dir_row(vec![
                icon(icons::MAIL, 16.0, p.muted),
                text(addr.as_str()).size(14).color(p.text).into(),
                button(text(t("contact_write")).size(13).font(medium()))
                    .padding([6, 12])
                    .style(outline_btn(p, p.text))
                    .on_press(Message::WriteTo(c.name.clone(), addr.clone()))
                    .into(),
                space::horizontal().into(),
            ])
            .spacing(12)
            .align_y(Alignment::Center),
        );
    }
    for tel in &c.phones {
        body = body.push(
            dir_row(vec![
                icon(icons::PHONE, 16.0, p.muted),
                text(tel.as_str()).size(14).font(MONO).color(p.text).into(),
            ])
            .spacing(12)
            .align_y(Alignment::Center),
        );
    }
    if !c.note.trim().is_empty() {
        body = body.push(space().height(8)).push(
            container(text(c.note.as_str()).size(14).color(p.text2))
                .padding(14)
                .width(Fill)
                .style(chip_box(p.chip, p.text2, 10.0)),
        );
    }

    container(scrollable(container(body).padding(32).max_width(720)).style(scroll_style(p)))
        .width(Fill)
        .height(Fill)
        .style(panel(p.bg))
        .into()
}
