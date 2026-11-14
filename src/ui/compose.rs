//! "New message" dialog (Compose.dc.html), drawn over the main window
//! and following the active theme.

use super::*;
use crate::app::{App, Compose, ComposeField, ComposeMode, Fmt};
use crate::i18n::t;
use iced::keyboard::{Key, key::Named};
use iced::widget::text_editor::{Binding, KeyPress};
use iced::widget::{Column, column, opaque, row, scrollable, space, text_editor};
use iced::{Alignment, Length::Fill, Padding};

pub fn view<'a>(app: &'a App, c: &'a Compose, p: Palette) -> Element<'a, Message> {
    let title = match c.mode {
        ComposeMode::New => t("compose_new"),
        ComposeMode::Reply => t("compose_reply"),
        ComposeMode::Forward => t("compose_forward"),
    };

    // Dark title bar (ink), like the mockup.
    let head = container(
        dir_row(vec![
            text(title)
                .size(14)
                .font(semibold())
                .color(p.on_ink)
                .width(Fill)
                .into(),
            button(icon(icons::CLOSE, 16.0, p.on_ink))
                .width(36)
                .height(36)
                .padding(10)
                .style(move |_, st| button::Style {
                    background: matches!(st, button::Status::Hovered)
                        .then_some(Background::Color(mix(p.ink, p.on_ink, 0.15))),
                    text_color: p.on_ink,
                    border: Border {
                        radius: radius(8.0),
                        ..Default::default()
                    },
                    ..Default::default()
                })
                // Not while sending: a failure must find its draft.
                .on_press_maybe((!c.sending).then_some(Message::ComposeClose))
                .into(),
        ])
        .align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 10.0,
        right: 10.0,
        bottom: 10.0,
        left: 20.0,
    })
    .width(Fill)
    .style(panel(p.ink));

    let from = app
        .config
        .account()
        .map(|a| {
            if a.display_name.is_empty() {
                a.email.clone()
            } else {
                format!("{} <{}>", a.display_name, a.email)
            }
        })
        .unwrap_or_default();

    let field_row = |label: &'static str, content: Element<'a, Message>| -> Element<'a, Message> {
        column![
            container(
                dir_row(vec![
                    text(label).size(13).color(p.muted).width(64).into(),
                    content,
                ])
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding([0, 20])
            .height(48)
            .center_y(48),
            hline(p.line_soft),
        ]
        .into()
    };
    let input = |value: &'a str, field: ComposeField, bold: bool| -> Element<'a, Message> {
        text_input("", value)
            .on_input(move |v| Message::ComposeField(field, v))
            .on_submit(Message::ComposePickFirst(field))
            .size(14)
            .padding([8, 0])
            .font(if bold { semibold() } else { INTER })
            .style(bare_input(p))
            .width(Fill)
            .into()
    };

    let cc_toggle = button(text(t("cc_bcc")).size(13).color(p.muted))
        .padding([4, 6])
        .style(ghost(p, p.muted))
        .on_press(Message::ComposeToggleCc);

    // Address suggestions under the field being typed in.
    let suggestions = |field: ComposeField| -> Option<Element<'a, Message>> {
        let (f, list) = c.suggest.as_ref()?;
        if *f != field {
            return None;
        }
        let mut col = Column::new().spacing(2);
        for (name, address) in list {
            let label = if name.is_empty() {
                address.clone()
            } else {
                format!("{name}  <{address}>")
            };
            col = col.push(
                button(text(label).size(13).color(p.text))
                    .width(Fill)
                    .padding([6, 10])
                    .style(ghost(p, p.text))
                    .on_press(Message::ComposePick(field, name.clone(), address.clone())),
            );
        }
        Some(
            container(container(col).padding(4).width(Fill).style(card(p, p.line, 8.0)))
                .padding([4, 20])
                .into(),
        )
    };

    let mut fields = Column::new()
        .push(field_row(
            t("from"),
            text(from).size(14).color(p.text).width(Fill).into(),
        ))
        .push(field_row(
            t("to"),
            dir_row(vec![input(&c.to, ComposeField::To, false), cc_toggle.into()])
                .align_y(Alignment::Center)
                .into(),
        ));
    if let Some(s) = suggestions(ComposeField::To) {
        fields = fields.push(s);
    }
    if c.show_cc {
        fields = fields.push(field_row(t("cc"), input(&c.cc, ComposeField::Cc, false)));
        if let Some(s) = suggestions(ComposeField::Cc) {
            fields = fields.push(s);
        }
        fields = fields.push(field_row(t("bcc"), input(&c.bcc, ComposeField::Bcc, false)));
        if let Some(s) = suggestions(ComposeField::Bcc) {
            fields = fields.push(s);
        }
    }
    fields = fields.push(field_row(
        t("subject"),
        input(&c.subject, ComposeField::Subject, true),
    ));

    // Format bar: Markdown helpers (sent as HTML + plain text), or plain text only.
    let fmt_btn = |svg: &'static str, tip: &'static str, fmt: Fmt| -> Element<'a, Message> {
        let enabled = c.rich && c.preview.is_none();
        tooltip(
            button(icon(svg, 16.0, if enabled { p.text2 } else { p.line }))
                .width(32)
                .height(32)
                .padding(8)
                .style(ghost(p, p.text2))
                .on_press_maybe(enabled.then_some(Message::ComposeFormat(fmt))),
            tip,
            p,
        )
    };
    let sep = || -> Element<'a, Message> {
        container(space::horizontal().width(1))
            .height(18)
            .style(panel(p.line))
            .into()
    };
    let pill = |label: &'static str, on: bool, msg: Message| -> Element<'a, Message> {
        button(text(label).size(12).font(medium()))
            .padding([5, 10])
            .style(move |_, st| button::Style {
                background: Some(Background::Color(if on {
                    p.soft
                } else if matches!(st, button::Status::Hovered) {
                    p.chip
                } else {
                    Color::TRANSPARENT
                })),
                text_color: if on { p.accent_text } else { p.muted },
                border: Border {
                    color: if on { mix(p.accent, p.bg, 0.6) } else { p.line },
                    width: 1.0,
                    radius: radius(6.0),
                },
                ..Default::default()
            })
            .on_press(msg)
            .into()
    };
    let mut tools: Vec<Element<'a, Message>> = vec![
        fmt_btn(icons::BOLD, t("fmt_bold"), Fmt::Bold),
        fmt_btn(icons::ITALIC, t("fmt_italic"), Fmt::Italic),
        fmt_btn(icons::STRIKE, t("fmt_strike"), Fmt::Strike),
        fmt_btn(icons::HEADING, t("fmt_heading"), Fmt::Heading),
        sep(),
        fmt_btn(icons::LIST, t("fmt_bullets"), Fmt::Bullets),
        fmt_btn(icons::LIST_NUM, t("fmt_numbers"), Fmt::Numbers),
        fmt_btn(icons::QUOTE, t("fmt_quote"), Fmt::Quote),
        sep(),
        fmt_btn(icons::LINK, t("fmt_link"), Fmt::Link),
        fmt_btn(icons::CODE, t("fmt_code"), Fmt::Code),
        space::horizontal().into(),
    ];
    if c.rich {
        tools.push(pill(
            if c.preview.is_some() {
                t("compose_edit")
            } else {
                t("compose_preview")
            },
            c.preview.is_some(),
            Message::ComposeTogglePreview,
        ));
    }
    tools.push(pill(
        if c.rich { t("rich_text") } else { t("plain_text") },
        c.rich,
        Message::ComposeToggleRich,
    ));
    let format_bar = column![
        container(dir_row(tools).spacing(2).align_y(Alignment::Center)).padding([6, 14]),
        hline(p.line_soft),
    ];

    let editor = text_editor(&c.body)
        .placeholder(t("body_placeholder"))
        .on_action(Message::ComposeEdit)
        .key_binding(|kp: KeyPress| {
            if kp.modifiers.command() {
                let fmt = match kp.key.as_ref() {
                    // Ctrl/⌘ + Enter sends.
                    Key::Named(Named::Enter) => return Some(Binding::Custom(Message::ComposeSend)),
                    Key::Character("b") => Some(Fmt::Bold),
                    Key::Character("i") => Some(Fmt::Italic),
                    Key::Character("k") => Some(Fmt::Link),
                    _ => None,
                };
                if let Some(fmt) = fmt {
                    return Some(Binding::Custom(Message::ComposeFormat(fmt)));
                }
            }
            Binding::from_key_press(kp)
        })
        .size(15)
        .line_height(1.6)
        .height(Fill)
        .padding([18, 20])
        .style(move |_, _| text_editor::Style {
            background: Background::Color(Color::TRANSPARENT),
            border: Border::default(),
            placeholder: p.muted,
            value: p.text2,
            selection: mix(p.accent, p.bg, 0.7),
        });

    let body_area: Element<'a, Message> = match &c.preview {
        Some(items) => {
            let md = iced::widget::markdown::view(items, md_settings(p, 15.0)).map(|_url| Message::Noop);
            scrollable(container(md).padding([18, 20]).width(Fill))
                .height(Fill)
                .style(scroll_style(p))
                .into()
        }
        None => editor.into(),
    };

    let mut attachments = iced::widget::Row::new().spacing(10);
    for (i, path) in c.attachments.iter().enumerate() {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let size = std::fs::metadata(path).map(|m| m.len() as usize).unwrap_or(0);
        attachments = attachments.push(
            container(
                row![
                    container(text(extension(&name)).font(MONO).size(10).color(p.accent_text))
                        .center_x(32)
                        .center_y(32)
                        .style(chip_box(p.chip, p.accent_text, 6.0)),
                    text(trunc(&name, 28)).size(13).font(medium()).color(p.text),
                    text(human_size(size)).size(12).color(p.muted),
                    button(icon(icons::CLOSE, 12.0, p.muted))
                        .padding(6)
                        .style(ghost(p, p.muted))
                        .on_press(Message::ComposeRemove(i)),
                ]
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding(Padding {
                top: 8.0,
                right: 6.0,
                bottom: 8.0,
                left: 8.0,
            })
            .style(card(p, p.line, 10.0)),
        );
    }

    let send_label = if c.sending { t("sending") } else { t("send") };
    let send = button(
        row![
            icon(icons::SEND, 16.0, p.on_accent),
            text(send_label).size(15).font(semibold()),
            container(text("Ctrl ↵").font(MONO).size(11).color(p.on_accent))
                .padding([2, 6])
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color {
                        a: 0.18,
                        ..p.on_accent
                    })),
                    border: Border {
                        radius: radius(5.0),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .height(44)
    .padding([12, 18])
    .style(accent_btn(p))
    .on_press_maybe((!c.sending).then_some(Message::ComposeSend));

    let attach = tooltip(
        button(icon(icons::CLIP, 18.0, p.text2))
            .width(44)
            .height(44)
            .padding(13)
            .style(ghost(p, p.text2))
            .on_press(Message::ComposeAttach),
        t("attach"),
        p,
    );
    let discard = tooltip(
        button(icon(icons::TRASH, 18.0, p.muted))
            .width(44)
            .height(44)
            .padding(13)
            .style(ghost(p, p.muted))
            .on_press_maybe((!c.sending).then_some(Message::ComposeClose)),
        t("discard"),
        p,
    );
    // OpenPGP toggles (need a key: otherwise they point to Settings › Security).
    let has_key = app.pgp_fpr.is_some();
    let pgp_toggle = |label: &'static str, on: bool, msg: Message| -> Element<'a, Message> {
        let fg = if on { p.accent_text } else { p.muted };
        let b = button(
            row![
                icon(icons::LOCK, 14.0, fg),
                text(label).size(13).font(medium()).color(fg)
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .padding([8, 10])
        .style(move |_, st| button::Style {
            background: Some(Background::Color(if on {
                p.soft
            } else if matches!(st, button::Status::Hovered) {
                p.chip
            } else {
                Color::TRANSPARENT
            })),
            text_color: fg,
            border: Border {
                color: if on {
                    mix(p.accent, p.bg, 0.6)
                } else {
                    Color::TRANSPARENT
                },
                width: 1.0,
                radius: radius(8.0),
            },
            ..Default::default()
        });
        if has_key {
            b.on_press(msg).into()
        } else {
            tooltip(
                b.on_press(Message::OpenSettings(crate::app::Section::Security)),
                t("pgp_needs_key"),
                p,
            )
        }
    };
    let sign = pgp_toggle(t("pgp_sign"), c.sign, Message::ComposeToggleSign);
    let encrypt = pgp_toggle(t("pgp_encrypt"), c.encrypt, Message::ComposeToggleEncrypt);

    let error: Element<'a, Message> = match &c.error {
        Some(e) => text(trunc(e, 90)).size(13).color(p.danger).width(Fill).into(),
        None => space::horizontal().into(),
    };
    let footer = column![
        hline(p.line_soft),
        container(
            dir_row(vec![send.into(), attach, sign, encrypt, error, discard])
                .spacing(8)
                .align_y(Alignment::Center)
        )
        .padding([12, 16])
        .width(Fill)
        .style(panel(p.chrome)),
    ];

    let mut body = column![head, fields, format_bar, body_area];
    if !c.attachments.is_empty() {
        body = body.push(
            container(scrollable(attachments.wrap().vertical_spacing(10)).height(iced::Length::Shrink))
                .padding(Padding {
                    top: 0.0,
                    right: 20.0,
                    bottom: 12.0,
                    left: 20.0,
                })
                .max_height(140),
        );
    }
    body = body.push(footer);

    let dialog = container(body)
        .max_width(760)
        .max_height(640)
        .width(Fill)
        .height(Fill)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.bg)),
            text_color: Some(p.text),
            border: Border {
                radius: radius(14.0),
                ..Default::default()
            },
            shadow: Shadow {
                color: Color::from_rgba(0.16, 0.10, 0.07, 0.25),
                offset: Vector::new(0.0, 12.0),
                blur_radius: 40.0,
            },
            ..Default::default()
        });

    // Dimmed backdrop; clicking it does not close (avoids losing a draft by accident).
    opaque(
        container(opaque(dialog))
            .center(Fill)
            .padding(24)
            .style(move |_| container::Style {
                background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.35))),
                ..Default::default()
            }),
    )
}
