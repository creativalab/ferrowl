//! Settings screen (Settings.dc.html): General (language), Accounts, Appearance.

use super::*;
use crate::app::{App, Conn, Field, Section};
use crate::config::{Density, Protocol};
use crate::i18n::{self, LANGS, t, tf};
use crate::providers::{Login, PROVIDERS, guide_url};
use ferrowl_core::rules::{Action, Rule};
use iced::widget::{Column, column, pick_list, row, scrollable, space, toggler};
use iced::{Alignment, Length::Fill, Padding};

pub fn view(app: &App, section: Section) -> Element<'_, Message> {
    let p = app.palette();
    let first_run = app.config.account().is_none();

    let back: Element<'_, Message> = if first_run {
        row![
            logo(&p, 32.0),
            text("Ferrowl").size(18).font(INTER_BOLD).color(p.text)
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into()
    } else {
        button(
            row![
                icon(
                    if i18n::is_rtl() {
                        icons::CHEVRON_RIGHT
                    } else {
                        icons::CHEVRON_LEFT
                    },
                    18.0,
                    p.text2
                ),
                text(t("back_to_mail")).size(14).font(medium())
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .height(44)
        .padding([12, 12])
        .style(ghost(p, p.text2))
        .on_press(Message::CloseSettings)
        .into()
    };
    let header = container(
        dir_row(vec![
            back,
            text(t("settings")).size(18).font(INTER_BOLD).color(p.text).into(),
            space::horizontal().into(),
        ])
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([12, 20])
    .width(Fill)
    .style(panel(p.chrome));

    let nav_btn = |s: Section, label: &'static str| {
        button(
            container(
                text(label)
                    .size(14)
                    .font(if s == section { semibold() } else { medium() }),
            )
            .center_y(Fill),
        )
        .width(Fill)
        .height(40)
        .padding([0, 12])
        .style(nav_item(p, s == section))
        .on_press(Message::OpenSettings(s))
    };
    let nav = container(
        column![
            nav_btn(Section::General, t("section_general")),
            nav_btn(Section::Accounts, t("section_accounts")),
            nav_btn(Section::Security, t("section_security")),
            nav_btn(Section::Rules, t("rules_title")),
            nav_btn(Section::Appearance, t("section_appearance")),
            space::vertical(),
            container(
                text(tf("version", &[("v", env!("CARGO_PKG_VERSION"))]))
                    .font(MONO)
                    .size(12)
                    .color(p.muted)
            )
            .padding(Padding {
                top: 16.0,
                right: 12.0,
                bottom: 4.0,
                left: 12.0
            }),
        ]
        .spacing(2),
    )
    .width(240)
    .height(Fill)
    .padding([16, 12])
    .style(panel(p.side));

    let content = match section {
        Section::General => general(app, p),
        Section::Accounts => accounts(app, p),
        Section::Security => security(app, p),
        Section::Rules => rules(app, p),
        Section::Appearance => appearance(app, p),
    };
    let main = scrollable(
        container(container(content).max_width(760))
            .padding(Padding {
                top: 32.0,
                right: 40.0,
                bottom: 48.0,
                left: 40.0,
            })
            .width(Fill),
    )
    .height(Fill)
    .style(scroll_style(p));

    let body = dir_row(vec![nav.into(), vline(p.line), main.into()]).height(Fill);
    let page = container(column![header, hline(p.line), body])
        .width(Fill)
        .height(Fill)
        .style(panel(p.bg));

    // Error/info notifications are shown here too.
    match super::mail_view::toast_layer(app, p) {
        Some(toast) => iced::widget::stack![page, toast].into(),
        None => page.into(),
    }
}

fn title<'a>(title: &'a str, desc: &'a str, p: Palette) -> Element<'a, Message> {
    column![
        text(title).size(26).font(INTER_BOLD).color(p.text),
        text(desc).size(15).color(p.muted)
    ]
    .spacing(6)
    .into()
}

/// Settings row: title + description on one side, control on the other.
fn setting_row<'a>(
    name: &'a str,
    desc: &'a str,
    control: Element<'a, Message>,
    p: Palette,
    last: bool,
) -> Element<'a, Message> {
    let r = container(
        dir_row(vec![
            column![
                text(name).size(15).font(medium()).color(p.text),
                text(desc).size(13).color(p.muted)
            ]
            .spacing(2)
            .width(Fill)
            .into(),
            control,
        ])
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([16, 0]);
    if last {
        r.into()
    } else {
        column![r, hline(p.line_soft)].into()
    }
}

fn group<'a>(rows: Vec<Element<'a, Message>>, p: Palette) -> Element<'a, Message> {
    container(Column::with_children(rows))
        .padding([4, 20])
        .width(Fill)
        .style(card(p, p.line, 14.0))
        .into()
}

// ---------------------------------------------------------------- General

#[derive(Debug, Clone, PartialEq, Eq)]
struct LangChoice {
    code: &'static str,
    label: String,
}

impl std::fmt::Display for LangChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn general(app: &App, p: Palette) -> Element<'_, Message> {
    let mut choices = vec![LangChoice {
        code: "auto",
        label: tf("language_system", &[("lang", i18n::system_lang().native)]),
    }];
    choices.extend(LANGS.iter().map(|l| LangChoice {
        code: l.code,
        label: l.native.to_string(),
    }));
    let selected = choices
        .iter()
        .find(|c| c.code == app.config.ui.language)
        .cloned()
        .or_else(|| choices.first().cloned());
    let picker = pick_list(choices, selected, |c: LangChoice| {
        Message::SetLanguage(c.code.to_string())
    })
    .text_size(14)
    .padding([10, 12])
    .width(220)
    .style(pick_style(p))
    .menu_style(menu_style(p));

    column![
        title(t("section_general"), t("general_desc"), p),
        group(
            vec![
                setting_row(t("language"), t("language_desc"), picker.into(), p, false),
                setting_row(
                    t("notifications"),
                    t("notifications_desc"),
                    switch(p, app.config.ui.notifications, Message::ToggleNotifications),
                    p,
                    true,
                ),
            ],
            p,
        ),
    ]
    .spacing(28)
    .into()
}

// ---------------------------------------------------------------- Security

/// Blocked and allowed senders: two lists, each with a box to add one.
fn senders<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let list = |allow: bool| -> Element<'a, Message> {
        let entries = if allow {
            &app.allowed_senders
        } else {
            &app.blocked_senders
        };
        let typed = if allow {
            &app.sender_input.1
        } else {
            &app.sender_input.0
        };
        let mut col = Column::new().spacing(6).push(
            text(if allow {
                t("senders_allowed")
            } else {
                t("senders_blocked")
            })
            .size(14)
            .font(semibold())
            .color(p.text),
        );
        if entries.is_empty() {
            col = col.push(text(t("senders_none")).size(13).color(p.muted));
        }
        for e in entries {
            col = col.push(
                dir_row(vec![
                    text(e.as_str())
                        .size(14)
                        .font(MONO)
                        .color(p.text)
                        .width(Fill)
                        .into(),
                    button(text(t("senders_remove")).size(12).color(p.danger))
                        .padding([4, 10])
                        .style(ghost(p, p.danger))
                        .on_press(Message::RemoveSender(e.clone()))
                        .into(),
                ])
                .align_y(Alignment::Center),
            );
        }
        col = col.push(
            dir_row(vec![
                text_input(t("senders_placeholder"), typed)
                    .on_input(move |v| Message::SenderInput(allow, v))
                    .on_submit(Message::AddSender(allow))
                    .size(14)
                    .padding([8, 10])
                    .style(input(p, 8.0))
                    .into(),
                button(text(t("senders_add")).size(13).font(medium()))
                    .padding([8, 14])
                    .style(outline_btn(p, p.text))
                    .on_press_maybe(typed.contains('@').then_some(Message::AddSender(allow)))
                    .into(),
            ])
            .spacing(8)
            .align_y(Alignment::Center),
        );
        col.width(Fill).into()
    };
    container(
        column![
            text(t("senders_title")).size(16).font(INTER_BOLD).color(p.text),
            text(t("senders_desc")).size(14).color(p.text2),
            dir_row(vec![list(false), list(true)]).spacing(24),
        ]
        .spacing(14),
    )
    .padding(20)
    .width(Fill)
    .style(card(p, p.line, 14.0))
    .into()
}

fn security(app: &App, p: Palette) -> Element<'_, Message> {
    let Some(acc) = app.config.account() else {
        return title(t("section_security"), t("security_desc"), p);
    };
    let btn = |label: &'static str, msg: Message, primary: bool| -> Element<'_, Message> {
        let b = button(text(label).size(14).font(medium()))
            .height(40)
            .padding([10, 16])
            .on_press(msg);
        if primary {
            b.style(accent_btn(p)).into()
        } else {
            b.style(outline_btn(p, p.text)).into()
        }
    };

    let key_line: Element<'_, Message> = match &app.pgp_fpr {
        Some(fpr) => column![
            text(t("pgp_your_key")).size(13).font(medium()).color(p.text2),
            text(fpr.clone()).font(MONO).size(13).color(p.text),
        ]
        .spacing(4)
        .into(),
        None => text(t("pgp_no_key")).size(14).color(p.muted).into(),
    };
    let mut actions = vec![];
    if app.pgp_fpr.is_none() {
        actions.push(btn(t("pgp_generate"), Message::PgpGenerate, true));
    } else {
        actions.push(btn(t("pgp_export"), Message::PgpExport, false));
    }
    actions.push(btn(t("pgp_import_own"), Message::PgpImportOwn, false));

    let passphrase = column![
        text(t("pgp_passphrase")).size(13).font(medium()).color(p.text2),
        dir_row(vec![
            text_input("", &app.pgp_pass_draft)
                .on_input(Message::PgpPassphrase)
                .on_submit(Message::PgpSavePassphrase)
                .secure(true)
                .size(14)
                .padding([10, 12])
                .style(input(p, 8.0))
                .width(Fill)
                .into(),
            btn(t("pgp_save"), Message::PgpSavePassphrase, false),
        ])
        .spacing(10)
        .align_y(Alignment::Center),
        text(t("pgp_passphrase_desc")).size(12).color(p.muted),
    ]
    .spacing(6);

    let own = container(
        column![
            text(tf("pgp_key_for", &[("email", &acc.email)]))
                .size(16)
                .font(INTER_BOLD)
                .color(p.text),
            key_line,
            passphrase,
            dir_row(actions).spacing(10),
        ]
        .spacing(18),
    )
    .padding(20)
    .width(Fill)
    .style(card(p, p.line, 14.0));

    let contacts = container(
        column![
            text(t("pgp_contacts")).size(16).font(INTER_BOLD).color(p.text),
            text(tf("pgp_contacts_desc", &[("n", &app.pgp_contacts.to_string())]))
                .size(14)
                .color(p.text2),
            btn(t("pgp_import_contacts"), Message::PgpImportContacts, false),
        ]
        .spacing(14),
    )
    .padding(20)
    .width(Fill)
    .style(card(p, p.line, 14.0));

    let privacy = group(
        vec![setting_row(
            t("remote_images"),
            t("remote_images_desc"),
            switch(p, app.config.ui.load_remote_images, Message::ToggleRemoteImages),
            p,
            true,
        )],
        p,
    );
    column![
        title(t("section_security"), t("security_desc"), p),
        privacy,
        senders(app, p),
        own,
        contacts
    ]
    .spacing(28)
    .into()
}

// ---------------------------------------------------------------- Accounts

/// POP3: how long downloaded messages stay on the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KeepChoice(Option<u32>);

impl std::fmt::Display for KeepChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            None => f.write_str(t("keep_forever")),
            Some(0) => f.write_str(t("keep_none")),
            Some(n) => f.write_str(&tf("keep_days", &[("n", &n.to_string())])),
        }
    }
}

/// Id of the e-mail field, focused when adding an account.
pub const EMAIL_FIELD: &str = "account-email";

fn field_id(f: Field) -> iced::widget::Id {
    match f {
        Field::Email => iced::widget::Id::new(EMAIL_FIELD),
        _ => iced::widget::Id::unique(),
    }
}

fn accounts(app: &App, p: Palette) -> Element<'_, Message> {
    let first_run = app.config.account().is_none();
    let heading = if first_run {
        title(t("setup_title"), t("setup_desc"), p)
    } else {
        title(t("section_accounts"), t("accounts_desc"), p)
    };

    let mut col = column![heading].spacing(28);

    // One card per account: click to edit it below. The one in use has a badge.
    let mut list = Column::new().spacing(10);
    for (i, acc) in app.config.accounts.iter().enumerate() {
        let selected = app.editing == Some(i);
        let in_use = i == app.config.active;
        let (badge_bg, badge_fg, badge) = if in_use && app.conn == Conn::Online {
            (p.sec_bg, p.sec_text, t("account_connected"))
        } else if in_use {
            (p.chip, p.text2, t("account_offline"))
        } else {
            (p.chip, p.muted, t("account_inactive"))
        };
        let name = if acc.display_name.is_empty() {
            acc.email.as_str()
        } else {
            acc.display_name.as_str()
        };
        let mut cells: Vec<Element<'_, Message>> = vec![
            avatar(
                acc.initials(),
                if in_use { p.ink } else { p.field },
                p.on_ink,
                40.0,
            ),
            column![
                text(name).size(15).font(semibold()).color(p.text),
                text(format!("{} · {}", acc.email, acc.imap_host))
                    .size(13)
                    .color(p.muted)
            ]
            .spacing(2)
            .width(Fill)
            .into(),
        ];
        if !in_use {
            cells.push(
                button(text(t("use_account")).size(13).font(medium()))
                    .padding([6, 12])
                    .style(outline_btn(p, p.text2))
                    .on_press(Message::SwitchAccount(i))
                    .into(),
            );
        }
        cells.push(
            container(text(badge).size(12).font(semibold()))
                .padding([4, 10])
                .style(chip_box(badge_bg, badge_fg, 999.0))
                .into(),
        );
        list = list.push(
            button(dir_row(cells).spacing(14).align_y(Alignment::Center))
                .width(Fill)
                .padding([14, 16])
                .style(move |_, st| button::Style {
                    background: Some(Background::Color(if selected {
                        p.soft
                    } else if matches!(st, button::Status::Hovered) {
                        p.chip
                    } else {
                        p.bg
                    })),
                    text_color: p.text,
                    border: Border {
                        color: if selected { p.accent } else { p.line },
                        width: if selected { 2.0 } else { 1.0 },
                        radius: radius(14.0),
                    },
                    ..Default::default()
                })
                .on_press(Message::EditAccount(i)),
        );
    }
    // Adding an account: the form replaces the list, so it is in view at once.
    let adding = !first_run && app.editing.is_none();
    if adding {
        col = col.push(
            button(text(t("cancel")).size(14).font(medium()))
                .padding([10, 16])
                .style(outline_btn(p, p.text))
                .on_press(Message::EditAccount(app.config.active)),
        );
    } else if !first_run {
        list = list.push(
            button(text(format!("+  {}", t("add_account"))).size(14).font(medium()))
                .padding([10, 16])
                .style(outline_btn(p, p.accent_text))
                .on_press(Message::NewAccount),
        );
        col = col.push(list);
    }

    let d = &app.draft;
    let shown = app.show_password;
    let field =
        |label: &'static str, value: &str, f: Field, secure: bool, mono: bool| -> Element<'_, Message> {
            let input_el = text_input("", value)
                .id(field_id(f))
                .on_input(move |v| Message::Draft(f, v))
                .secure(secure && !shown)
                .size(14)
                .padding([10, 12])
                .font(if mono { MONO } else { INTER })
                .style(input(p, 8.0));
            // Secret fields get an eye to show what is being typed.
            let control: Element<'_, Message> = if secure {
                dir_row(vec![
                    input_el.width(Fill).into(),
                    tooltip(
                        button(icon(
                            if shown { icons::EYE_OFF } else { icons::EYE },
                            17.0,
                            p.text2,
                        ))
                        .padding([9, 10])
                        .style(ghost(p, p.text2))
                        .on_press(Message::TogglePassword),
                        t(if shown { "hide_password" } else { "show_password" }),
                        p,
                    ),
                ])
                .spacing(4)
                .align_y(Alignment::Center)
                .into()
            } else {
                input_el.into()
            };
            column![text(label).size(13).font(medium()).color(p.text2), control]
                .spacing(6)
                .width(Fill)
                .into()
        };
    let port = |label: &'static str, value: &str, f: Field| -> Element<'_, Message> {
        column![
            text(label).size(13).font(medium()).color(p.text2),
            text_input("", value)
                .on_input(move |v| Message::Draft(f, v.chars().filter(char::is_ascii_digit).collect()))
                .size(14)
                .padding([10, 12])
                .font(MONO)
                .width(96)
                .style(input(p, 8.0))
        ]
        .spacing(6)
        .into()
    };
    let pair = |a: Element<'static, Message>, b: Element<'static, Message>| dir_row(vec![a, b]).spacing(16);

    let username_hint: Element<'_, Message> = if d.username.is_empty() && !d.email.is_empty() {
        button(text(t("use_email_as_username")).size(13).color(p.accent_text))
            .padding([2, 0])
            .style(ghost(p, p.accent_text))
            .on_press(Message::UseEmailAsUsername)
            .into()
    } else {
        space().into()
    };

    let can_test = d.to_account().is_some() && !app.testing;
    let test_label = if app.testing {
        t("testing")
    } else {
        t("test_connection")
    };
    let status: Element<'_, Message> = match &app.test_result {
        Some(Ok(msg)) => text(msg.clone()).size(13).color(p.sec_text).width(Fill).into(),
        Some(Err(e)) => text(trunc(e, 120)).size(13).color(p.danger).width(Fill).into(),
        None => space::horizontal().into(),
    };

    let adding = app.editing.is_none();
    let preset = d.preset.and_then(|i| PROVIDERS.get(i));
    let mut form = Column::new().spacing(18).push(
        text(if adding {
            t("new_account")
        } else {
            t("account_details")
        })
        .size(16)
        .font(INTER_BOLD)
        .color(p.text),
    );

    // New account: pick the provider and the servers fill themselves in.
    if adding {
        let chip = |label: &'static str, choice: Option<usize>| -> Element<'_, Message> {
            let selected = d.preset == choice;
            button(text(label).size(13).font(medium()))
                .padding([8, 14])
                .style(move |theme, status| {
                    if selected {
                        accent_btn(p)(theme, status)
                    } else {
                        outline_btn(p, p.text)(theme, status)
                    }
                })
                .on_press(Message::PickProvider(choice))
                .into()
        };
        let mut chips: Vec<Element<'_, Message>> = PROVIDERS
            .iter()
            .enumerate()
            .map(|(i, pr)| chip(pr.name, Some(i)))
            .collect();
        chips.push(chip(t("provider_other"), None));
        form = form.push(
            column![
                text(t("provider_label")).size(13).font(medium()).color(p.text2),
                dir_row(chips).spacing(8).wrap(),
            ]
            .spacing(8),
        );
    }

    form = form.push(pair(
        field(
            t("display_name"),
            &d.display_name,
            Field::DisplayName,
            false,
            false,
        ),
        field(t("email"), &d.email, Field::Email, false, false),
    ));

    // Google / Microsoft: "Sign in with…" (OAuth) when this build has the
    // provider's app ID.
    use ferrowl_core::oauth::Provider as OAuth;
    let oauth_for = |pr: &crate::providers::Provider| -> Option<OAuth> {
        match pr.login {
            Login::MicrosoftSignIn => Some(OAuth::Microsoft),
            _ if pr.name == "Google" => Some(OAuth::Google),
            _ => None,
        }
        .filter(|o| o.available())
    };
    let signed_in = |provider: OAuth| -> Element<'_, Message> {
        let name = match provider {
            OAuth::Microsoft => "Microsoft",
            OAuth::Google => "Google",
        };
        container(
            dir_row(vec![
                text(t("oauth_signed_in").replace("{provider}", name))
                    .size(14)
                    .color(p.sec_text)
                    .width(Fill)
                    .into(),
                button(text(t("oauth_use_password")).size(12).color(p.accent_text))
                    .padding([2, 4])
                    .style(ghost(p, p.accent_text))
                    .on_press(Message::OAuthUsePassword)
                    .into(),
            ])
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .padding([10, 12])
        .style(chip_box(p.sec_bg, p.sec_text, 8.0))
        .into()
    };
    if let Some(provider) = d.oauth {
        // Already signed in with the provider: no password to type.
        form = form.push(signed_in(provider));
    } else if let Some(provider) = preset
        .and_then(oauth_for)
        // An existing Gmail / Outlook.com account too.
        .or_else(|| OAuth::for_email(&d.email).filter(|o| o.available()))
    {
        let label = match provider {
            OAuth::Microsoft => t("oauth_microsoft"),
            OAuth::Google => t("oauth_google"),
        };
        let mut row_items: Vec<Element<'_, Message>> = vec![
            button(text(label).size(14).font(semibold()))
                .height(40)
                .padding([10, 18])
                .style(accent_btn(p))
                .on_press_maybe((!app.oauth_busy).then_some(Message::OAuthSignIn(provider)))
                .into(),
        ];
        if app.oauth_busy {
            row_items.push(text(t("oauth_waiting")).size(13).color(p.muted).into());
        }
        form = form.push(dir_row(row_items).spacing(12).align_y(Alignment::Center));
        if provider == OAuth::Google {
            // The app password stays possible (accounts outside the test list).
            form = form
                .push(text(t("oauth_or_app_password")).size(13).color(p.text2))
                .push(field(t("password"), &d.password, Field::Password, true, false));
        }
    } else if let Some(pr) = preset {
        // Known provider: only the password is left to type.
        form = form.push(field(t("password"), &d.password, Field::Password, true, false));
        let hint: Option<(String, Option<String>)> = match pr.login {
            Login::AppPassword => Some((
                tf("app_password_hint", &[("provider", pr.name)]),
                pr.guide.map(|g| guide_url(g, i18n::current().code)),
            )),
            Login::MicrosoftSignIn => Some((t("ms_signin_hint").to_string(), None)),
        };
        if let Some((msg, link)) = hint {
            let mut row_items: Vec<Element<'_, Message>> = vec![text(msg).size(13).color(p.text2).into()];
            if let Some(url) = link {
                row_items.push(
                    button(text(t("app_password_howto")).size(13).color(p.accent_text))
                        .padding([2, 0])
                        .style(ghost(p, p.accent_text))
                        .on_press(Message::OpenLink(url))
                        .into(),
                );
            }
            form = form.push(dir_row(row_items).spacing(8).align_y(Alignment::Center).wrap());
        }
    } else {
        let pop3 = d.protocol == Protocol::Pop3;
        let kind_chip = |label: &'static str, proto: Protocol| -> Element<'_, Message> {
            let selected = d.protocol == proto;
            button(text(label).size(13).font(medium()))
                .padding([8, 14])
                .style(move |theme, status| {
                    if selected {
                        accent_btn(p)(theme, status)
                    } else {
                        outline_btn(p, p.text)(theme, status)
                    }
                })
                .on_press(Message::SetProtocol(proto))
                .into()
        };
        form = form.push(
            column![
                text(t("account_type")).size(13).font(medium()).color(p.text2),
                dir_row(vec![
                    kind_chip("IMAP", Protocol::Imap),
                    kind_chip("POP3", Protocol::Pop3)
                ])
                .spacing(8),
            ]
            .spacing(8),
        );
        form = form
            .push(pair(
                column![
                    field(t("username"), &d.username, Field::Username, false, false),
                    username_hint
                ]
                .spacing(4)
                .width(Fill)
                .into(),
                field(t("password"), &d.password, Field::Password, true, false),
            ))
            .push(
                dir_row(vec![
                    field(
                        if pop3 { t("pop3_server") } else { t("imap_server") },
                        &d.imap_host,
                        Field::ImapHost,
                        false,
                        true,
                    ),
                    port(t("port"), &d.imap_port, Field::ImapPort),
                ])
                .spacing(16),
            )
            .push(
                dir_row(vec![
                    field(t("smtp_server"), &d.smtp_host, Field::SmtpHost, false, true),
                    port(t("port"), &d.smtp_port, Field::SmtpPort),
                ])
                .spacing(16),
            )
            .push(
                text(if pop3 {
                    t("server_hint_pop3")
                } else {
                    t("server_hint")
                })
                .size(12)
                .color(p.muted),
            );
        if pop3 {
            let choices: Vec<KeepChoice> = [None, Some(30), Some(7), Some(0)]
                .into_iter()
                .map(KeepChoice)
                .collect();
            form = form.push(
                column![
                    text(t("keep_on_server")).size(13).font(medium()).color(p.text2),
                    pick_list(choices, Some(KeepChoice(d.pop3_keep_days)), |c: KeepChoice| {
                        Message::SetKeepDays(c.0)
                    })
                    .text_size(14)
                    .padding([10, 12])
                    .width(280)
                    .style(pick_style(p))
                    .menu_style(menu_style(p)),
                    text(t("pop3_local_note")).size(12).color(p.muted),
                ]
                .spacing(6),
            );
        }
    }

    // Contacts and calendar servers: found from the address when left empty.
    form = form.push(
        column![
            field(t("carddav_url"), &d.carddav_url, Field::CardDav, false, true),
            field(t("caldav_url"), &d.caldav_url, Field::CalDav, false, true),
            text(t("carddav_url_hint")).size(12).color(p.muted),
        ]
        .spacing(4),
    );
    // A login of their own, for servers that are not the mail provider.
    form = form.push(
        column![
            text(t("dav_credentials")).size(13).font(semibold()).color(p.text),
            field(
                t("dav_username"),
                &d.dav_username,
                Field::DavUsername,
                false,
                false
            ),
            field(
                t("dav_password"),
                &d.dav_password,
                Field::DavPassword,
                true,
                false
            ),
            text(t("dav_credentials_hint")).size(12).color(p.muted),
        ]
        .spacing(4),
    );

    form = form.push(
        dir_row(vec![
            button(text(test_label).size(14).font(medium()))
                .height(40)
                .padding([10, 16])
                .style(outline_btn(p, p.text))
                .on_press_maybe(can_test.then_some(Message::TestConnection))
                .into(),
            status,
            button(text(t("save")).size(14).font(semibold()))
                .height(40)
                .padding([10, 16])
                .style(accent_btn(p))
                .on_press_maybe(d.to_account().map(|_| Message::SaveAccount))
                .into(),
        ])
        .spacing(10)
        .align_y(Alignment::Center),
    );

    let details = container(form)
        .padding(20)
        .width(Fill)
        .style(card(p, p.line, 14.0));

    col = col.push(details);

    // Remove (with confirmation) for an existing account.
    if app.editing.is_some() {
        let remove: Element<'_, Message> = if app.confirm_remove {
            dir_row(vec![
                text(t("confirm_remove_account"))
                    .size(13)
                    .color(p.danger)
                    .width(Fill)
                    .into(),
                button(text(t("cancel")).size(13))
                    .padding([8, 14])
                    .style(outline_btn(p, p.text))
                    .on_press(Message::EditAccount(app.editing.unwrap_or(0)))
                    .into(),
                button(text(t("remove_account")).size(13).font(semibold()))
                    .padding([8, 14])
                    .style(move |_, _| button::Style {
                        background: Some(Background::Color(p.danger)),
                        text_color: if p.dark { p.bg } else { Color::WHITE },
                        border: Border {
                            radius: radius(8.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .on_press(Message::RemoveAccount(true))
                    .into(),
            ])
            .spacing(10)
            .align_y(Alignment::Center)
            .into()
        } else {
            dir_row(vec![
                space::horizontal().into(),
                button(text(t("remove_account")).size(13).color(p.danger))
                    .padding([8, 14])
                    .style(ghost(p, p.danger))
                    .on_press(Message::RemoveAccount(false))
                    .into(),
            ])
            .into()
        };
        col = col.push(remove);
    }
    col.into()
}

// ---------------------------------------------------------------- Appearance

fn appearance(app: &App, p: Palette) -> Element<'_, Message> {
    let current = app.config.ui.density;
    let seg = |d: Density, label: &'static str| {
        button(
            text(label)
                .size(13)
                .font(if d == current { semibold() } else { INTER }),
        )
        .padding([6, 12])
        .style(segment(p, d == current))
        .on_press(Message::SetDensity(d))
    };
    let density = container(
        row![
            seg(Density::Compact, t("density_compact")),
            seg(Density::Comfortable, t("density_comfortable")),
            seg(Density::Spacious, t("density_spacious")),
        ]
        .spacing(4),
    )
    .padding(3)
    .style(chip_box(p.chip, p.text2, 9.0));

    // Conversations or one row per message.
    let mode = |on: bool, label: &'static str| {
        let selected = app.config.ui.threaded == on;
        button(
            text(label)
                .size(13)
                .font(if selected { semibold() } else { INTER }),
        )
        .padding([6, 12])
        .style(segment(p, selected))
        .on_press(Message::SetThreaded(on))
    };
    let view_mode =
        container(row![mode(true, t("view_threads")), mode(false, t("view_messages"))].spacing(4))
            .padding(3)
            .style(chip_box(p.chip, p.text2, 9.0));
    let page = |n: u32| {
        let selected = app.config.ui.page_size == n;
        button(
            text(n.to_string())
                .size(13)
                .font(if selected { semibold() } else { INTER }),
        )
        .padding([6, 12])
        .style(segment(p, selected))
        .on_press(Message::SetPageSize(n))
    };
    let page_size = container(row![page(20), page(50), page(100)].spacing(4))
        .padding(3)
        .style(chip_box(p.chip, p.text2, 9.0));

    let preview = toggler(app.config.ui.show_preview)
        .on_toggle(Message::TogglePreview)
        .size(24)
        .style(move |_, st| {
            let on = matches!(
                st,
                iced::widget::toggler::Status::Active { is_toggled: true }
                    | iced::widget::toggler::Status::Hovered { is_toggled: true }
            );
            iced::widget::toggler::Style {
                background: Background::Color(if on { p.accent } else { p.knob_off }),
                background_border_width: 0.0,
                background_border_color: Color::TRANSPARENT,
                foreground: Background::Color(Color::WHITE),
                foreground_border_width: 0.0,
                foreground_border_color: Color::TRANSPARENT,
                text_color: None,
                border_radius: None,
                padding_ratio: 0.12,
            }
        });

    column![
        title(t("section_appearance"), t("appearance_desc"), p),
        column![
            text(t("color_theme")).size(14).font(semibold()).color(p.text2),
            theme_grid(&app.config.ui.theme, p, 84.0, 12.0)
        ]
        .spacing(12),
        group(
            vec![
                setting_row(t("density"), t("density_desc"), density.into(), p, false),
                setting_row(t("list_view"), t("list_view_desc"), view_mode.into(), p, false),
                setting_row(t("page_size"), t("page_size_desc"), page_size.into(), p, false),
                setting_row(t("preview"), t("preview_desc"), preview.into(), p, true),
            ],
            p
        ),
    ]
    .spacing(28)
    .into()
}

// ---------------------------------------------------------------- Rules

/// Where a rule sends matching mail, for the drop-down.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Destination {
    action: Option<Action>,
    label: String,
}

impl std::fmt::Display for Destination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// "Mark as read · Archive": what a rule does, for the list.
fn rule_summary(app: &App, rule: &Rule) -> String {
    rule.actions
        .iter()
        .map(|a| match a {
            Action::MarkRead => t("rules_mark_read").to_string(),
            Action::Star => t("rules_star").to_string(),
            Action::Silence => t("rules_silence").to_string(),
            Action::Archive => t("archive").to_string(),
            Action::MoveTo(f) => format!("{} {}", t("rules_destination"), app.folder_label(f)),
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn rules(app: &App, p: Palette) -> Element<'_, Message> {
    if app.config.account().is_none() {
        return title(t("rules_title"), t("rules_desc"), p);
    };
    let saved = &app.rules;
    let mut list: Vec<Element<'_, Message>> = Vec::new();
    for (i, rule) in saved.iter().enumerate() {
        let name = if rule.name.is_empty() {
            rule.condition.clone()
        } else {
            rule.name.clone()
        };
        let id = rule.id.clone();
        let on = rule.enabled;
        let row = dir_row(vec![
            button(
                column![
                    text(name).size(15).font(medium()).color(p.text),
                    text(rule_summary(app, rule)).size(13).color(p.muted),
                ]
                .spacing(2),
            )
            .width(Fill)
            .padding([4, 0])
            .style(ghost(p, p.text))
            .on_press(Message::RuleEdit(id.clone()))
            .into(),
            toggler(on)
                .on_toggle({
                    let id = id.clone();
                    move |v| Message::RuleSwitch(id.clone(), v)
                })
                .size(24)
                .style(switch_style(p))
                .into(),
            button(icon(icons::TRASH, 16.0, p.danger))
                .padding([6, 8])
                .style(ghost(p, p.danger))
                .on_press(Message::RuleDelete(id))
                .into(),
        ])
        .spacing(12)
        .align_y(Alignment::Center);
        let row = container(row).padding([12, 0]);
        list.push(if i + 1 < saved.len() {
            column![row, hline(p.line_soft)].into()
        } else {
            row.into()
        });
    }
    if list.is_empty() {
        list.push(
            container(text(t("rules_none")).size(14).color(p.muted))
                .padding([16, 0])
                .into(),
        );
    }
    let add = button(text(t("rules_add")).size(14).font(medium()))
        .padding([10, 16])
        .style(accent_btn(p))
        .on_press_maybe(app.rule_edit.is_none().then_some(Message::RuleNew));

    let mut page = column![title(t("rules_title"), t("rules_desc"), p), group(list, p), add,].spacing(20);
    if let Some((rule, _)) = &app.rule_edit {
        page = page.push(rule_editor(app, rule, p));
    }
    page.into()
}

fn rule_editor<'a>(app: &'a App, rule: &'a Rule, p: Palette) -> Element<'a, Message> {
    let heading = |s: &'a str| text(s).size(14).font(semibold()).color(p.text);
    let mut preview = Column::new().spacing(4);
    // Nothing typed yet: no preview.
    let typed = !rule.condition.trim().is_empty();
    if typed && app.rule_preview.is_empty() {
        preview = preview.push(text(t("rules_preview_none_desktop")).size(13).color(p.muted));
    } else {
        for (from, subject) in &app.rule_preview {
            preview = preview.push(
                dir_row(vec![
                    text(trunc(from, 28))
                        .size(13)
                        .font(medium())
                        .color(p.text)
                        .width(200)
                        .into(),
                    text(trunc(subject, 70))
                        .size(13)
                        .color(p.text2)
                        .width(Fill)
                        .into(),
                ])
                .spacing(12),
            );
        }
    }
    let check = |label: &'static str, action: Action| {
        let on = rule.actions.contains(&action);
        iced::widget::checkbox(on)
            .label(label)
            .on_toggle(move |v| Message::RuleAction(action.clone(), v))
            .size(16)
            .text_size(14)
    };

    let mut choices = vec![
        Destination {
            action: None,
            label: t("rules_stay").to_string(),
        },
        Destination {
            action: Some(Action::Archive),
            label: t("archive").to_string(),
        },
    ];
    choices.extend(
        app.folders
            .iter()
            .filter(|f| f.selectable && f.kind != crate::mail::FolderKind::Inbox)
            .map(|f| Destination {
                action: Some(Action::MoveTo(f.name.clone())),
                label: f.display(),
            }),
    );
    let current = rule
        .actions
        .iter()
        .find(|a| matches!(a, Action::Archive | Action::MoveTo(_)))
        .cloned();
    let selected = choices.iter().find(|c| c.action == current).cloned();
    let destination = pick_list(choices, selected, |c: Destination| {
        Message::RuleDestination(c.action)
    })
    .placeholder(t("rules_pick_folder"))
    .text_size(14)
    .padding([10, 12])
    .width(280)
    .style(pick_style(p))
    .menu_style(menu_style(p));

    let can_save = !rule.condition.trim().is_empty() && !rule.actions.is_empty();
    container(
        column![
            heading(t("rules_condition")),
            text_input(t("rules_condition_hint"), &rule.condition)
                .on_input(Message::RuleCondition)
                .size(14)
                .font(MONO)
                .padding([10, 12])
                .style(input(p, 8.0)),
            text(t("rules_condition_help")).size(13).color(p.muted),
            heading(t("rules_preview")),
            preview,
            heading(t("rules_then")),
            check(t("rules_mark_read"), Action::MarkRead),
            check(t("rules_star"), Action::Star),
            check(t("rules_silence"), Action::Silence),
            heading(t("rules_destination")),
            destination,
            heading(t("rules_name")),
            text_input("", &rule.name)
                .on_input(Message::RuleName)
                .size(14)
                .padding([10, 12])
                .style(input(p, 8.0)),
            iced::widget::checkbox(rule.enabled)
                .label(t("rules_enabled"))
                .on_toggle(Message::RuleEnabled)
                .size(16)
                .text_size(14),
            dir_row(vec![
                space::horizontal().into(),
                button(text(t("cancel")).size(14).font(medium()))
                    .padding([10, 16])
                    .style(outline_btn(p, p.text))
                    .on_press(Message::RuleCancel)
                    .into(),
                button(text(t("rules_save")).size(14).font(medium()))
                    .padding([10, 16])
                    .style(accent_btn(p))
                    .on_press_maybe(can_save.then_some(Message::RuleSave))
                    .into(),
            ])
            .spacing(8),
        ]
        .spacing(10),
    )
    .padding(20)
    .width(Fill)
    .style(card(p, p.line, 14.0))
    .into()
}

/// On/off switch in the theme's colors.
fn switch<'a>(p: Palette, on: bool, msg: fn(bool) -> Message) -> Element<'a, Message> {
    toggler(on).on_toggle(msg).size(24).style(switch_style(p)).into()
}

fn switch_style(
    p: Palette,
) -> impl Fn(&Theme, iced::widget::toggler::Status) -> iced::widget::toggler::Style {
    move |_, st| {
        let on = matches!(
            st,
            iced::widget::toggler::Status::Active { is_toggled: true }
                | iced::widget::toggler::Status::Hovered { is_toggled: true }
        );
        iced::widget::toggler::Style {
            background: Background::Color(if on { p.accent } else { p.knob_off }),
            background_border_width: 0.0,
            background_border_color: Color::TRANSPARENT,
            foreground: Background::Color(Color::WHITE),
            foreground_border_width: 0.0,
            foreground_border_color: Color::TRANSPARENT,
            text_color: None,
            border_radius: None,
            padding_ratio: 0.12,
        }
    }
}
