//! Contacts pane: the list (with search) in place of the message list, the
//! open contact in place of the reader. Contacts come from the account's
//! CardDAV server (see `ferrowl_core::carddav`).

use super::*;
use crate::app::{App, ContactField, ContactForm, SyncState};
use crate::i18n::{t, tf};
use ferrowl_core::vcard::Contact;
use iced::widget::{Column, column, row, scrollable, space};
use iced::{Alignment, Background, Border, Length::Fill, Padding};

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
    let add = tooltip(
        button(icon(icons::PLUS, 18.0, p.text2))
            .padding(8)
            .style(ghost(p, p.text2))
            .on_press(Message::NewContact),
        t("contact_new"),
        p,
    );
    let header = dir_row(vec![
        text(t("contacts")).size(20).font(INTER_BOLD).width(Fill).into(),
        add,
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
    if let Some(f) = &app.contact_form {
        return form(f, p);
    }
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

    // Edit / delete (asks first).
    let actions: Element<'a, Message> = if app.confirm_contact_delete {
        column![
            text(t("contact_delete")).size(14).color(p.text),
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
                    .on_press(Message::DeleteContact(true))
                    .into(),
                button(text(t("cancel")).size(13).font(medium()))
                    .padding([7, 14])
                    .style(outline_btn(p, p.text))
                    .on_press(Message::CancelContact)
                    .into(),
            ])
            .spacing(8),
        ]
        .spacing(10)
        .into()
    } else {
        dir_row(vec![
            button(text(t("contact_edit")).size(13).font(medium()))
                .padding([7, 14])
                .style(outline_btn(p, p.text))
                .on_press(Message::EditContact)
                .into(),
            button(text(t("delete")).size(13).font(medium()))
                .padding([7, 14])
                .style(outline_btn(p, p.danger))
                .on_press(Message::DeleteContact(false))
                .into(),
        ])
        .spacing(8)
        .into()
    };
    body = body.push(space().height(8)).push(actions);

    page(body.into(), p)
}

fn page<'a>(body: Element<'a, Message>, p: Palette) -> Element<'a, Message> {
    container(scrollable(container(body).padding(32).max_width(720)).style(scroll_style(p)))
        .width(Fill)
        .height(Fill)
        .style(panel(p.bg))
        .into()
}

/// New contact, or changes to the open one.
fn form<'a>(f: &'a ContactForm, p: Palette) -> Element<'a, Message> {
    let label = |s: &'static str| text(s).size(13).font(medium()).color(p.text2);
    let input = |placeholder: &'static str, value: &'a str, field: ContactField| {
        text_input(placeholder, value)
            .on_input(move |v| Message::ContactField(field, v))
            .on_submit(Message::SaveContact)
            .size(14)
            .padding([9, 12])
            .style(super::input(p, 8.0))
    };
    let add = |s: &'static str, email: bool| {
        button(text(s).size(13).font(medium()))
            .padding([6, 12])
            .style(outline_btn(p, p.text))
            .on_press(Message::ContactAddLine(email))
    };

    let mut col = Column::new()
        .spacing(10)
        .push(
            text(if f.url.is_empty() {
                t("contact_new")
            } else {
                t("contact_edit")
            })
            .size(22)
            .font(INTER_BOLD)
            .color(p.text),
        )
        .push(space().height(4))
        .push(label(t("contact_name")))
        .push(input(t("contact_name"), &f.name, ContactField::Name))
        .push(label(t("contact_org")))
        .push(input(
            t("contact_org"),
            &f.organization,
            ContactField::Organization,
        ))
        .push(label(t("contact_emails")));
    for (i, e) in f.emails.iter().enumerate() {
        col = col.push(input("name@example.com", e, ContactField::Email(i)));
    }
    col = col
        .push(add(t("contact_add_email"), true))
        .push(label(t("contact_phones")));
    for (i, n) in f.phones.iter().enumerate() {
        col = col.push(input("+39 …", n, ContactField::Phone(i)));
    }
    col = col
        .push(add(t("contact_add_phone"), false))
        .push(label(t("contact_note")))
        .push(
            iced::widget::text_editor(&f.note)
                .on_action(Message::ContactNote)
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
                .on_press_maybe(f.can_save().then_some(Message::SaveContact))
                .into(),
            button(text(t("cancel")).size(14).font(medium()))
                .padding([9, 18])
                .style(outline_btn(p, p.text))
                .on_press(Message::CancelContact)
                .into(),
        ])
        .spacing(10),
    );
    page(col.into(), p)
}
