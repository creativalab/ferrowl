//! Main window: top bar · folders · list · reader (Main.dc.html).

use super::*;
use crate::app::{App, Bulk, Conn, LIST_ID, ListFilter, Pane, Popup, READER_ID, SEARCH_ID, Section, Select};
use crate::i18n::{self, SUPPORT_EMAIL, t, tf};
use crate::mail::{Folder, FolderKind};
use crate::theme;
use iced::alignment::{Horizontal, Vertical};
use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Row, column, mouse_area, opaque, row, scrollable, sensor, space, stack, text_editor,
};
use iced::{Alignment, Length::Fill, Padding};
use std::collections::BTreeMap;

pub fn view(app: &App) -> Element<'_, Message> {
    let p = app.palette();
    let (list, detail) = match app.pane {
        Pane::Contacts => (super::contacts::list(app, p), super::contacts::detail(app, p)),
        Pane::Calendar => (super::calendar::list(app, p), super::calendar::detail(app, p)),
        Pane::Mail => (message_list(app, p), reader(app, p)),
    };
    // No message open: the list takes the whole width (no reading pane).
    let body = if app.pane == Pane::Mail && app.list_is_wide() {
        dir_row(vec![sidebar(app, p), vline(p.line), list]).height(Fill)
    } else {
        let edge = if app.pane == Pane::Mail {
            list_edge(app, p)
        } else {
            vline(p.line)
        };
        dir_row(vec![sidebar(app, p), vline(p.line), list, edge, detail]).height(Fill)
    };

    let base = container(column![top_bar(app, p), hline(p.line), body])
        .width(Fill)
        .height(Fill)
        .style(panel(p.bg));

    let mut layers: Vec<Element<'_, Message>> = vec![base.into()];
    if app.appearance_open {
        layers.push(appearance_popover(app, p));
    }
    if let Some(d) = app.drag.as_ref().filter(|d| d.active) {
        layers.push(drag_label(app, d, p));
    }
    if let Some(m) = &app.menu {
        layers.push(context_menu(app, m, p));
    }
    if let Some((kind, at)) = &app.popup {
        layers.push(popup_menu(app, *kind, *at, p));
    }
    if app.confirm_delete {
        layers.push(delete_dialog(app, p));
    }
    if let Some((i, content)) = &app.preview {
        layers.push(preview_overlay(app, *i, content.as_ref(), p));
    }
    if let Some(c) = &app.compose {
        layers.push(super::compose::view(app, c, p));
    }
    if let Some(toast) = toast_layer(app, p) {
        layers.push(toast);
    }
    stack(layers).width(Fill).height(Fill).into()
}

/// The line between the message list and the reader, which can be dragged
/// to widen or narrow the list.
fn list_edge<'a>(app: &App, p: Palette) -> Element<'a, Message> {
    let color = if app.list_resize.is_some() {
        p.accent
    } else {
        p.line
    };
    let line = container(space().width(1).height(Fill)).style(panel(color));
    mouse_area(container(line).width(7).height(Fill).center_x(7))
        .on_press(Message::ListResizeStart)
        .interaction(iced::mouse::Interaction::ResizingHorizontally)
        .into()
}

// ---------------------------------------------------------------- top bar

fn top_bar<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let brand = row![
        logo(&p, 32.0),
        text("Ferrowl").size(18).font(INTER_BOLD).color(p.text)
    ]
    .spacing(10)
    .align_y(Alignment::Center)
    .width(208);

    let search = container(
        row![
            icon(icons::SEARCH, 18.0, p.muted),
            text_input(t("search_placeholder"), &app.search)
                .id(SEARCH_ID)
                .on_input(Message::Search)
                .size(14)
                .padding(0)
                .style(bare_input(p)),
            kbd("Ctrl K", p),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .height(40)
    .padding([0, 14])
    .center_y(40)
    .width(Fill)
    .max_width(720)
    .style(card(p, p.field, 10.0));

    let syncing = app.loading_list || app.conn == Conn::Connecting;
    let sync = button(icon(icons::SYNC, 20.0, if syncing { p.accent } else { p.text2 }))
        .width(44)
        .height(44)
        .padding(12)
        .style(ghost(p, p.text2))
        .on_press(Message::Sync);
    let sync = tooltip(sync, t("sync"), p);

    let gear_on = app.appearance_open;
    let gear = button(icon(
        icons::GEAR,
        20.0,
        if gear_on { p.active_text } else { p.text2 },
    ))
    .width(44)
    .height(44)
    .padding(12)
    .style(move |th, st| {
        let mut s = ghost(p, p.text2)(th, st);
        if gear_on {
            s.background = Some(Background::Color(p.side_active));
        }
        s
    })
    .on_press(Message::ToggleAppearance);
    let gear = tooltip(gear, t("appearance"), p);

    let initials = app
        .config
        .account()
        .map(|a| a.initials())
        .unwrap_or_else(|| "?".into());
    let me = button(avatar(initials, p.ink, p.on_ink, 36.0))
        .padding(0)
        .style(ghost(p, p.text))
        .on_press(Message::OpenSettings(Section::Accounts));

    container(
        dir_row(vec![
            brand.into(),
            search.into(),
            space::horizontal().into(),
            row![sync, gear, me].spacing(8).align_y(Alignment::Center).into(),
        ])
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([12, 20])
    .width(Fill)
    .style(panel(p.chrome))
    .into()
}

// ---------------------------------------------------------------- sidebar

fn kind_icon(kind: FolderKind) -> &'static str {
    match kind {
        FolderKind::Inbox => icons::INBOX,
        FolderKind::Sent => icons::SEND,
        FolderKind::Drafts => icons::DRAFT,
        FolderKind::Archive => icons::ARCHIVE,
        FolderKind::Spam => icons::SPAM,
        FolderKind::Trash => icons::TRASH,
        FolderKind::Other => icons::FOLDER,
    }
}

fn sidebar<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let compose = button(
        container(
            row![
                icon(icons::PEN, 18.0, p.on_accent),
                text(t("compose")).size(15).font(semibold())
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .center_x(Fill)
        .center_y(Fill),
    )
    .width(Fill)
    .height(44)
    .style(accent_btn(p))
    .on_press_maybe(app.config.account().map(|_| Message::ComposeNew));

    let mut specials = Column::new().spacing(2);
    for kind in FolderKind::SPECIAL {
        if let Some(f) = app.folder_by_kind(kind) {
            let count = app.unseen.get(&f.name).copied().unwrap_or(0);
            specials = specials.push(folder_item(app, p, f, kind.label().to_string(), count));
        }
    }

    // Contacts and calendar, under the special folders.
    let specials = specials
        .push(pane_item(
            app,
            p,
            icons::CONTACTS,
            t("contacts"),
            Pane::Contacts,
            Message::OpenContacts,
        ))
        .push(pane_item(
            app,
            p,
            icons::CALENDAR,
            t("calendar"),
            Pane::Calendar,
            Message::OpenCalendar,
        ));

    let tree = folder_tree(app, p);

    let (dot, status_line) = match &app.conn {
        Conn::Online => (
            theme::hex("#3C9462"),
            app.last_sync
                .map(|d| tf("status_synced", &[("time", &d.format("%H:%M").to_string())]))
                .unwrap_or_else(|| t("account_connected").into()),
        ),
        Conn::Connecting => (p.field, t("status_connecting").into()),
        Conn::Offline => (p.field, t("status_offline").into()),
        Conn::Error(_) => (p.danger, t("status_error").into()),
    };
    // Accounts: the active one first-class, the others one click away.
    let multi = app.config.accounts.len() > 1;
    let mut accounts = Column::new().spacing(2);
    for (i, acc) in app.config.accounts.iter().enumerate() {
        let active = i == app.config.active;
        let color = if active { dot } else { p.field };
        let row_el = dir_row(vec![
            container(space().width(8).height(8))
                .style(chip_box(color, color, 4.0))
                .into(),
            text(trunc(&acc.email, 21))
                .size(14)
                .font(if active && multi { medium() } else { INTER })
                .color(if active { p.text } else { p.text2 })
                .width(Fill)
                .into(),
        ])
        .spacing(10)
        .align_y(Alignment::Center);
        // The active account opens its settings; another one becomes active.
        let pick = if active {
            Message::OpenAccountSettings(i)
        } else {
            Message::SwitchAccount(i)
        };
        let settings = tooltip(
            button(icon(icons::GEAR, 15.0, p.muted))
                .padding([5, 6])
                .style(ghost(p, p.muted))
                .on_press(Message::OpenAccountSettings(i)),
            t("account_settings"),
            p,
        );
        accounts = accounts.push(
            dir_row(vec![
                button(row_el)
                    .width(Fill)
                    .padding([7, 12])
                    .style(nav_item(p, active && multi))
                    .on_press(pick)
                    .into(),
                settings,
            ])
            .spacing(2)
            .align_y(Alignment::Center),
        );
    }
    // "+" right after the section name, full size so it is easy to see.
    let add = tooltip(
        button(icon(icons::PLUS, 16.0, p.text2))
            .padding(3)
            .style(ghost(p, p.text2))
            .on_press(Message::AddAccount),
        t("add_account"),
        p,
    );
    let account = column![
        container(
            dir_row(vec![
                container(
                    text(t("section_account").to_uppercase())
                        .size(12)
                        .font(semibold())
                        .color(p.muted)
                )
                .into(),
                add,
                space::horizontal().into(),
            ])
            .spacing(6)
            .align_y(Alignment::Center)
        )
        .padding([0, 12]),
        accounts,
    ]
    .spacing(4);

    let status = container(
        column![
            text(status_line).font(MONO).size(12).color(p.text2),
            text(tf("folder_count", &[("n", &app.headers.len().to_string())]))
                .font(MONO)
                .size(12)
                .color(p.text2),
            text(concat!("Ferrowl ", env!("CARGO_PKG_VERSION")))
                .font(MONO)
                .size(12)
                .color(p.muted),
        ]
        .spacing(4),
    )
    .padding(12)
    .width(Fill)
    .style(chip_box(p.chip, p.text2, 10.0));

    let mut col = column![compose, specials].spacing(20);
    if let Some(tree) = tree {
        col = col.push(
            column![
                section_label(t("section_folders"), p),
                scrollable(tree).height(Fill).style(scroll_style(p))
            ]
            .spacing(6)
            .height(Fill),
        );
    } else {
        col = col.push(space::vertical());
    }
    container(col.push(account).push(status))
        .width(240)
        .height(Fill)
        .padding([16, 12])
        .style(panel(p.side))
        .into()
}

/// Sidebar entry that shows contacts or the calendar.
fn pane_item<'a>(
    app: &App,
    p: Palette,
    ic: &'static str,
    label: &'static str,
    pane: Pane,
    msg: Message,
) -> Element<'a, Message> {
    let active = app.pane == pane;
    let fg = if active { p.active_text } else { p.text2 };
    button(
        container(
            row![
                container(icon(ic, 18.0, fg)).width(20).center_x(20),
                text(label)
                    .size(14)
                    .font(if active { semibold() } else { medium() })
                    .width(Fill),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .center_y(Fill),
    )
    .width(Fill)
    .height(40)
    .padding([0, 12])
    .style(nav_item(p, active))
    .on_press_maybe(app.config.account().map(|_| msg))
    .into()
}

fn section_label<'a>(s: &str, p: Palette) -> Element<'a, Message> {
    container(text(s.to_uppercase()).size(12).font(semibold()).color(p.muted))
        .padding([0, 12])
        .into()
}

/// Drop target highlight while a message is dragged over a folder.
fn droppable<'a>(app: &App, name: &str, content: Element<'a, Message>, p: Palette) -> Element<'a, Message> {
    let dragging = app.drag.as_ref().is_some_and(|d| d.active);
    let over = dragging && app.drop_folder.as_deref() == Some(name);
    let content: Element<'a, Message> = if over {
        container(content)
            .style(move |_| container::Style {
                background: Some(Background::Color(p.soft)),
                border: Border {
                    color: p.accent,
                    width: 2.0,
                    radius: radius(9.0),
                },
                ..Default::default()
            })
            .into()
    } else {
        content
    };
    mouse_area(content)
        .on_enter(Message::FolderEnter(name.to_string()))
        .on_exit(Message::FolderExit(name.to_string()))
        .into()
}

fn folder_item<'a>(app: &App, p: Palette, f: &Folder, label: String, count: u32) -> Element<'a, Message> {
    let item = folder_button(app, p, f, label, count);
    droppable(app, &f.name, item, p)
}

fn folder_button<'a>(app: &App, p: Palette, f: &Folder, label: String, count: u32) -> Element<'a, Message> {
    let active = app.pane == Pane::Mail && app.current_folder.as_deref() == Some(f.name.as_str());
    let fg = if active { p.active_text } else { p.text2 };
    let count = if count > 0 {
        count.to_string()
    } else {
        String::new()
    };
    button(
        container(
            row![
                container(icon(kind_icon(f.kind), 18.0, fg))
                    .width(20)
                    .center_x(20),
                text(trunc(&label, 22))
                    .size(14)
                    .font(if active { semibold() } else { medium() })
                    .width(Fill),
                text(count).size(12).font(MONO).color(fg),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .center_y(Fill),
    )
    .width(Fill)
    .height(40)
    .padding([0, 12])
    .style(nav_item(p, active))
    .on_press(Message::SelectFolder(f.name.clone()))
    .into()
}

/// Tree of the other folders (subfolders included), collapsed by default.
#[derive(Default)]
struct Node {
    label: String,
    folder: Option<String>,
    kind: Option<FolderKind>,
    children: BTreeMap<String, Node>,
}

fn folder_tree<'a>(app: &'a App, p: Palette) -> Option<Element<'a, Message>> {
    let mut root = Node::default();
    for f in &app.folders {
        if f.kind == FolderKind::Inbox {
            continue;
        }
        let mut segs: &[String] = &f.path;
        if segs.len() > 1 && segs[0].eq_ignore_ascii_case("INBOX") {
            segs = &segs[1..];
        }
        let mut node = &mut root;
        for seg in segs {
            node = node.children.entry(seg.to_lowercase()).or_insert_with(|| Node {
                label: seg.clone(),
                ..Default::default()
            });
        }
        if f.selectable {
            node.folder = Some(f.name.clone());
        }
        if f.kind != FolderKind::Other {
            node.kind = Some(f.kind);
            node.label = f.display();
        }
    }
    // Special folders without subfolders are already at the top.
    root.children
        .retain(|_, n| n.kind.is_none() || !n.children.is_empty());
    if root.children.is_empty() {
        return None;
    }
    let mut rows = Vec::new();
    for (key, node) in &root.children {
        push_node(app, p, &mut rows, node, key.clone(), 0);
    }
    Some(Column::with_children(rows).spacing(1).into())
}

fn push_node<'a>(
    app: &App,
    p: Palette,
    rows: &mut Vec<Element<'a, Message>>,
    node: &Node,
    id: String,
    depth: u16,
) {
    let expanded = app.expanded.contains(&id);
    let active = node.folder.is_some() && app.current_folder == node.folder;
    let fg = if active { p.active_text } else { p.text2 };
    let chevron: Element<'a, Message> = if node.children.is_empty() {
        space().width(22).into()
    } else {
        let chev = if expanded {
            icons::CHEVRON_DOWN
        } else if i18n::is_rtl() {
            icons::CHEVRON_LEFT
        } else {
            icons::CHEVRON_RIGHT
        };
        button(container(icon(chev, 14.0, p.muted)).center(Fill))
            .width(22)
            .height(32)
            .padding(0)
            .style(ghost(p, p.muted))
            .on_press(Message::ToggleNode(id.clone()))
            .into()
    };
    let label = button(
        container(
            row![
                icon(node.kind.map(kind_icon).unwrap_or(icons::FOLDER), 16.0, fg),
                text(trunc(&node.label, 22 - (depth as usize * 2).min(12)))
                    .size(13)
                    .font(if active { semibold() } else { INTER })
                    .color(fg)
                    .wrapping(Wrapping::None)
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .center_y(Fill)
        .clip(true),
    )
    .width(Fill)
    .height(32)
    .padding([0, 8])
    .style(nav_item(p, active))
    .on_press(match &node.folder {
        Some(f) => Message::SelectFolder(f.clone()),
        None => Message::ToggleNode(id.clone()),
    });
    rows.push(
        dir_row(vec![
            space().width(depth as f32 * 14.0).into(),
            chevron,
            match &node.folder {
                Some(f) => droppable(app, f, label.into(), p),
                None => label.into(),
            },
        ])
        .align_y(Alignment::Center)
        .into(),
    );
    if expanded {
        for (key, child) in &node.children {
            push_node(app, p, rows, child, format!("{id}\u{1f}{key}"), depth + 1);
        }
    }
}

// ---------------------------------------------------------------- message list

fn message_list<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let wide = app.list_is_wide();
    let title = app
        .current_folder
        .as_ref()
        .map(|f| app.folder_label(f))
        .unwrap_or_default();

    let seg = |label: &'static str, f: ListFilter| {
        button(
            text(label)
                .size(13)
                .font(if app.filter == f { semibold() } else { INTER }),
        )
        .padding([6, 9])
        .style(segment(p, app.filter == f))
        .on_press(Message::SetFilter(f))
    };
    let filters = container(
        row![
            seg(t("filter_all"), ListFilter::All),
            seg(t("filter_unread"), ListFilter::Unread),
            seg(t("filter_starred"), ListFilter::Starred),
            seg(t("filter_attachments"), ListFilter::Attachments),
        ]
        .spacing(4),
    )
    .padding(3)
    .style(chip_box(p.chip, p.text2, 9.0));

    // Toolbar: the select box (with its menu) and the actions on the
    // checked messages (or the open one), enabled once there is one.
    let rows_now = app.list_rows();
    let all_checked = !rows_now.is_empty()
        && rows_now
            .iter()
            .all(|(_, uids, _)| uids.iter().all(|u| app.checked.contains(u)));
    let any_checked = !app.checked.is_empty();
    let select_box = button(
        container(if all_checked {
            text("✓").size(11).color(p.on_accent)
        } else if any_checked {
            text("–").size(12).font(semibold()).color(p.accent)
        } else {
            text("").size(11)
        })
        .center(16),
    )
    .padding(0)
    .style(move |_, _| button::Style {
        background: Some(Background::Color(if all_checked {
            p.accent
        } else {
            Color::TRANSPARENT
        })),
        border: Border {
            color: if any_checked { p.accent } else { p.text2 },
            width: 1.5,
            radius: radius(4.0),
        },
        ..Default::default()
    })
    .on_press(Message::SelectBy(if all_checked {
        Select::None
    } else {
        Select::All
    }));
    let select_ctl = container(
        row![
            container(select_box).padding([0, 4]),
            button(icon(icons::CHEVRON_DOWN, 14.0, p.text2))
                .padding([4, 4])
                .style(ghost(p, p.text2))
                .on_press(Message::OpenPopup(Popup::Select)),
        ]
        .spacing(2)
        .align_y(Alignment::Center),
    )
    .padding([3, 4])
    .style(move |_| container::Style {
        border: Border {
            color: if app.popup.as_ref().is_some_and(|(k, _)| *k == Popup::Select) {
                p.accent
            } else {
                p.line
            },
            width: 1.0,
            radius: radius(8.0),
        },
        ..Default::default()
    });
    // The actions also work on the open message when nothing is checked.
    let any_target = any_checked || app.selected.is_some();
    let act = |ic: &'static str, tip: &'static str, msg: Message| {
        tooltip(
            button(icon(ic, 17.0, if any_target { p.text2 } else { p.field }))
                .padding([7, 8])
                .style(ghost(p, p.text2))
                .on_press_maybe(any_target.then_some(msg)),
            tip,
            p,
        )
    };
    let mut tools = vec![
        select_ctl.into(),
        container(container(space().width(1).height(22)).style(panel(p.line)))
            .padding([0, 8])
            .into(),
        act(icons::EYE, t("mark_read"), Message::Bulk(Bulk::Read)),
        act(icons::MAIL, t("mark_unread"), Message::Bulk(Bulk::Unread)),
        act(icons::STAR_FILLED, t("star_add"), Message::Bulk(Bulk::Star)),
        act(icons::ARCHIVE, t("archive"), Message::Bulk(Bulk::Archive)),
        if app.current_kind() == crate::mail::FolderKind::Spam {
            act(icons::INBOX, t("spam_not"), Message::Bulk(Bulk::Spam))
        } else {
            act(icons::SPAM, t("spam_report"), Message::Bulk(Bulk::Spam))
        },
        act(icons::FOLDER, t("move_to"), Message::OpenPopup(Popup::Move)),
        act(icons::TRASH, t("delete"), Message::Bulk(Bulk::Delete)),
    ];
    if app.gmail {
        tools.insert(
            tools.len() - 1,
            act(icons::TAG, t("labels"), Message::OpenPopup(Popup::Labels)),
        );
    }
    tools.push(space::horizontal().into());
    if any_checked {
        tools.push(
            text(tf("selected_count", &[("n", &app.checked.len().to_string())]))
                .size(12)
                .font(semibold())
                .color(p.text2)
                .wrapping(Wrapping::None)
                .into(),
        );
    }
    // Pages, at the far end of the toolbar: "Page [n▾] of M" and the arrows.
    let size = app.config.ui.page_size.max(1) as usize;
    let total = rows_now.len();
    let from = (app.page * size).min(total);
    let (back, forward) = if i18n::is_rtl() {
        (icons::CHEVRON_RIGHT, icons::CHEVRON_LEFT)
    } else {
        (icons::CHEVRON_LEFT, icons::CHEVRON_RIGHT)
    };
    let pages = total.div_ceil(size).max(1);
    let pager = dir_row(vec![
        text(t("page_label")).size(12).color(p.text2).into(),
        // The page number opens the list of pages, to jump to one.
        button(
            row![
                text((app.page + 1).to_string())
                    .size(12)
                    .font(semibold())
                    .color(p.text),
                icon(icons::CHEVRON_DOWN, 12.0, p.text2),
            ]
            .spacing(4)
            .align_y(Alignment::Center),
        )
        .padding([3, 8])
        .style(pick_btn(p))
        .on_press(Message::OpenPopup(Popup::Page))
        .into(),
        text(tf("page_of", &[("n", &pages.to_string())]))
            .size(12)
            .color(p.text2)
            .wrapping(Wrapping::None)
            .into(),
        button(icon(back, 16.0, if app.page > 0 { p.text2 } else { p.field }))
            .padding(5)
            .style(ghost(p, p.text2))
            .on_press_maybe((app.page > 0).then_some(Message::Page(-1)))
            .into(),
        // Forward also past the loaded messages: older ones come from the
        // local copy.
        button(icon(forward, 16.0, p.text2))
            .padding(5)
            .style(ghost(p, p.text2))
            .on_press(Message::Page(1))
            .into(),
    ])
    .spacing(6)
    .align_y(Alignment::Center);
    if any_checked {
        tools.push(space().width(12).into());
    }
    tools.push(pager.into());
    let toolbar = container(dir_row(tools).spacing(6).align_y(Alignment::Center)).padding(Padding {
        top: 14.0,
        right: 20.0,
        bottom: 10.0,
        left: 20.0,
    });

    // Folder name, with the sort order on its right.
    let sort_btn = button(
        row![
            icon(icons::SORT, 15.0, p.text2),
            text(t(app.config.ui.sort.key()))
                .size(13)
                .color(p.text2)
                .wrapping(Wrapping::None),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([5, 8])
    .style(ghost(p, p.text2))
    .on_press(Message::OpenPopup(Popup::Sort));
    let header = dir_row(vec![
        container(
            text(trunc(&title, 24))
                .size(20)
                .font(INTER_BOLD)
                .wrapping(Wrapping::None),
        )
        .width(Fill)
        .clip(true)
        .into(),
    ])
    .spacing(12)
    .align_y(Alignment::Center)
    .padding(Padding {
        top: 8.0,
        right: 20.0,
        bottom: 12.0,
        left: 20.0,
    });
    // Filters on the start side, the sort order on the end side.
    let view_mode = container(
        dir_row(vec![filters.into(), space::horizontal().into(), sort_btn.into()]).align_y(Alignment::Center),
    )
    .padding(Padding {
        top: 0.0,
        right: 20.0,
        bottom: 10.0,
        left: 20.0,
    });

    let visible = app.visible();
    let content: Element<'a, Message> = if visible.is_empty() {
        let msg = if app.loading_list || app.conn == Conn::Connecting {
            t("list_loading")
        } else {
            t("list_empty")
        };
        container(text(msg).size(14).color(p.muted))
            .center_x(Fill)
            .padding(40)
            .into()
    } else {
        let pad = app.density_padding();
        let preview = app.config.ui.show_preview;
        // One row per conversation (its newest message) or per message, in
        // the chosen order.
        let rows: Vec<_> = rows_now.into_iter().skip(from).take(size).collect();
        let threaded = app.config.ui.threaded;
        // Room for text in a row (padding and scrollbar off), so a wider
        // list shows longer senders, subjects and previews.
        let row_px = if wide {
            app.window_width - 240.0
        } else {
            app.list_width()
        } - 52.0;
        let mut col = Column::new();
        for (h, uids, unread) in rows {
            let count = uids.len();
            // Sender: the fixed column of the wide list, otherwise what the
            // date, star and count chip leave.
            let sender_px = if wide {
                230.0
            } else {
                row_px - 46.0 - 110.0 - if count > 1 { 32.0 } else { 0.0 }
            };
            let checked = uids.iter().all(|u| app.checked.contains(u));
            // An expanded conversation shows its messages below the row and
            // highlights the open one there; collapsed, the row stands for it.
            let expanded = count > 1 && uids.iter().any(|u| app.open_threads.contains(u));
            let selected = if expanded {
                false
            } else {
                app.selected.is_some_and(|s| uids.contains(&s))
            };
            let check = button(
                container(if checked {
                    text("✓").size(11).color(p.on_accent)
                } else {
                    text("").size(11)
                })
                .center(14),
            )
            .padding(0)
            .style(move |_, _| button::Style {
                background: Some(Background::Color(if checked {
                    p.accent
                } else {
                    Color::TRANSPARENT
                })),
                border: Border {
                    color: if checked { p.accent } else { p.field },
                    width: 1.5,
                    radius: radius(4.0),
                },
                ..Default::default()
            })
            .on_press(Message::Check(uids.clone(), !checked));
            // Leading 16 px column: a conversation's expand arrow, otherwise
            // the unread dot; the checkbox then sits right before the sender.
            let dot = || -> Element<'a, Message> {
                container(container(space().width(8).height(8)).style(chip_box(
                    if unread { p.accent } else { Color::TRANSPARENT },
                    p.accent,
                    4.0,
                )))
                .center(16)
                .into()
            };
            let mut top: Vec<Element<'a, Message>> = Vec::new();
            {
                top.push(if threaded && count > 1 {
                    tooltip(
                        button(icon(
                            if expanded {
                                icons::CHEVRON_DOWN
                            } else if i18n::is_rtl() {
                                icons::CHEVRON_LEFT
                            } else {
                                icons::CHEVRON_RIGHT
                            },
                            14.0,
                            p.text2,
                        ))
                        .padding(1)
                        .style(ghost(p, p.muted))
                        .on_press(Message::ToggleThread(uids.clone())),
                        if expanded {
                            t("thread_collapse")
                        } else {
                            t("thread_expand")
                        },
                        p,
                    )
                } else {
                    dot()
                });
            }
            top.extend([
                check.into(),
                container(
                    text(trunc(&h.from, fit(sender_px, 14.0)))
                        .wrapping(Wrapping::None)
                        .size(14)
                        .font(if unread { INTER_BOLD } else { medium() })
                        .color(p.text),
                )
                .width(if wide { iced::Length::Fixed(230.0) } else { Fill })
                .clip(true)
                .into(),
            ]);
            // Wide list: subject and preview on the same line as the sender.
            if wide {
                let mut line = vec![
                    text(h.subject.as_str())
                        .size(14)
                        .font(if unread { semibold() } else { INTER })
                        .color(p.text)
                        .wrapping(Wrapping::None)
                        .into(),
                ];
                if preview && !h.preview.is_empty() {
                    // The preview takes (and is clipped to) the room left by
                    // the subject, so a long preview never pushes the subject
                    // out of view; in right-to-left it hugs the subject.
                    let rtl = crate::i18n::is_rtl();
                    line.push(
                        container(
                            text(format!("— {}", h.preview))
                                .size(13)
                                .color(p.muted)
                                .wrapping(Wrapping::None),
                        )
                        .width(Fill)
                        .align_x(if rtl {
                            iced::alignment::Horizontal::Right
                        } else {
                            iced::alignment::Horizontal::Left
                        })
                        .clip(true)
                        .into(),
                    );
                }
                top.push(
                    container(dir_row(line).spacing(8).align_y(Alignment::Center))
                        .width(Fill)
                        .clip(true)
                        .into(),
                );
            }
            // Gmail labels as small chips (at most two, then "+n").
            let names: Vec<String> = h
                .labels
                .iter()
                .filter(|l| app.current_folder.as_deref() != Some(l.as_str()))
                .map(|l| app.folder_label(l))
                .collect();
            for name in names.iter().take(2) {
                top.push(
                    container(
                        text(trunc(name, 16))
                            .size(11)
                            .font(medium())
                            .color(p.text2)
                            .wrapping(Wrapping::None),
                    )
                    .padding([2, 8])
                    .style(chip_box(p.chip, p.text2, 999.0))
                    .into(),
                );
            }
            if names.len() > 2 {
                top.push(
                    text(format!("+{}", names.len() - 2))
                        .size(11)
                        .color(p.muted)
                        .into(),
                );
            }
            if h.has_attachment {
                top.push(icon(icons::CLIP, 14.0, p.muted));
            }
            if count > 1 {
                top.push(
                    container(text(count.to_string()).size(11).font(semibold()))
                        .padding([1, 6])
                        .style(chip_box(p.chip, p.text2, 999.0))
                        .into(),
                );
            }
            top.push(
                text(short_date(h.timestamp))
                    .font(MONO)
                    .size(12)
                    .color(p.muted)
                    .into(),
            );
            // Star: on when any message of the row is starred; a click
            // stars (or unstars) the whole row.
            let starred = uids.iter().any(|u| {
                app.headers
                    .iter()
                    .chain(app.search_results.iter().flatten())
                    .any(|m| m.uid == *u && m.flagged)
            });
            top.push(tooltip(
                button(icon(
                    if starred { icons::STAR_FILLED } else { icons::STAR },
                    15.0,
                    if starred { theme::hex("#E2A400") } else { p.field },
                ))
                .padding(2)
                .style(ghost(p, p.muted))
                .on_press(Message::ToggleStar(uids.clone(), !starred)),
                if starred { t("star_remove") } else { t("star_add") },
                p,
            ));

            let mut lines = column![dir_row(top).spacing(8).align_y(Alignment::Center)].spacing(4);
            if !wide {
                lines = lines.push(
                    container(
                        text(trunc(&h.subject, fit(row_px - 46.0, 14.0)))
                            .size(14)
                            .font(if unread { semibold() } else { INTER })
                            .color(p.text)
                            .wrapping(Wrapping::None),
                    )
                    .padding(indent(46.0))
                    .width(Fill)
                    .clip(true),
                );
            }
            if !wide && preview && !h.preview.is_empty() {
                lines = lines.push(
                    container(
                        text(trunc(&h.preview, fit(row_px - 46.0, 13.0)))
                            .size(13)
                            .color(p.muted)
                            .wrapping(Wrapping::None),
                    )
                    .padding(indent(46.0))
                    .width(Fill)
                    .clip(true),
                );
            }
            let row_btn = button(lines)
                .width(Fill)
                .padding([pad, 20.0])
                .style(move |_, st| button::Style {
                    background: Some(Background::Color(if selected {
                        p.soft
                    } else if matches!(st, button::Status::Hovered) {
                        mix(p.bg, p.soft, 0.5)
                    } else {
                        p.bg
                    })),
                    text_color: p.text,
                    ..Default::default()
                })
                .on_press(Message::Open(h.uid));
            // 3 px accent bar on the selected row.
            let item: Element<'a, Message> = if selected {
                let bar = container(space().width(3).height(Fill)).style(panel(p.accent));
                let bar = if i18n::is_rtl() {
                    container(bar).align_x(Horizontal::Right).width(Fill).height(Fill)
                } else {
                    container(bar).height(Fill)
                };
                stack![row_btn, bar].into()
            } else {
                row_btn.into()
            };
            // Hover tracking (drag start) and right-click menu.
            let uid = h.uid;
            let item = mouse_area(item)
                .on_enter(Message::RowEnter(uid))
                .on_exit(Message::RowExit(uid))
                .on_right_press(Message::OpenMenu(uid));
            col = col.push(item).push(hline(p.line_soft));
            if expanded {
                // The conversation's messages, newest first, indented under
                // its row.
                for m in app.conversation(h.uid).into_iter().rev() {
                    col = col
                        .push(thread_row(app, m, wide, pad, p))
                        .push(hline(p.line_soft));
                }
            }
        }
        scrollable(col)
            .id(LIST_ID)
            .height(Fill)
            .style(scroll_style(p))
            .into()
    };

    // Gmail inbox tabs (Primary, Promotions, Social, Updates, Forums), with
    // the unread count of each.
    let tabs: Element<'a, Message> = if app.tabs_shown() {
        let mut row_items: Vec<Element<'a, Message>> = Vec::new();
        for c in ferrowl_core::mail::Category::ALL {
            let active = app.category == c;
            let unread = app.headers.iter().filter(|h| h.category == c && !h.seen).count();
            let mut label = row![
                text(t(c.key()))
                    .size(13)
                    .font(if active { semibold() } else { medium() })
            ]
            .spacing(6)
            .align_y(Alignment::Center);
            if unread > 0 {
                label = label.push(
                    container(
                        text(unread.to_string())
                            .size(11)
                            .font(semibold())
                            .color(p.on_accent),
                    )
                    .padding([1, 7])
                    .style(chip_box(p.accent, p.on_accent, 999.0)),
                );
            }
            // The underline as wide as the tab's text (a Fill width would
            // stretch every tab across the row).
            let width = t(c.key()).chars().count() as f32 * 7.4 + if unread > 0 { 58.0 } else { 28.0 };
            row_items.push(
                button(column![
                    // Same width as the underline, text centered: background
                    // and underline always line up.
                    container(label).width(width).center_x(width).padding([8, 0]),
                    container(space().width(width).height(2)).style(panel(if active {
                        p.accent
                    } else {
                        Color::TRANSPARENT
                    })),
                ])
                .padding(0)
                // Tab look: a light background on the open (or hovered) tab,
                // rounded at the top only, square where it meets the line.
                .style(move |_, st| {
                    let hovered = matches!(st, button::Status::Hovered | button::Status::Pressed);
                    button::Style {
                        background: (active || hovered).then_some(Background::Color(if active {
                            p.soft
                        } else {
                            mix(p.bg, p.soft, 0.5)
                        })),
                        text_color: if active { p.accent_text } else { p.text2 },
                        border: Border {
                            radius: iced::border::Radius {
                                top_left: 8.0,
                                top_right: 8.0,
                                bottom_right: 0.0,
                                bottom_left: 0.0,
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    }
                })
                .on_press(Message::SetCategory(c))
                .into(),
            );
        }
        // Room below the tabs, before the filters.
        column![
            container(dir_row(row_items).spacing(4)).padding([0, 16]),
            hline(p.line_soft),
            space().height(14),
        ]
        .into()
    } else {
        space().height(0).into()
    };

    container(column![toolbar, header, tabs, view_mode, content])
        .width(if wide {
            Fill
        } else {
            iced::Length::Fixed(app.list_width())
        })
        .height(Fill)
        .style(panel(p.bg))
        .into()
}

fn indent(n: f32) -> Padding {
    if i18n::is_rtl() {
        Padding {
            right: n,
            ..Padding::ZERO
        }
    } else {
        Padding {
            left: n,
            ..Padding::ZERO
        }
    }
}

// ---------------------------------------------------------------- reader

fn tool_btn<'a>(
    p: Palette,
    ic: &'static str,
    label: Option<&'static str>,
    msg: Message,
) -> Element<'a, Message> {
    let content: Element<'a, Message> = match label {
        Some(l) => row![icon(ic, 18.0, p.text2), text(l).size(14)]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        None => icon(ic, 18.0, p.text2),
    };
    let b = button(container(content).center_y(Fill))
        .height(44)
        .padding(if label.is_some() { [0, 14] } else { [0, 13] })
        .style(ghost(p, p.text2))
        .on_press(msg);
    b.into()
}

fn reader<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let Some(msg) = &app.message else {
        let inner: Element<'a, Message> = if app.loading_message {
            text(t("loading_message")).size(14).color(p.muted).into()
        } else {
            column![
                icon(icons::MAIL, 48.0, p.field),
                text(t("select_message")).size(15).color(p.muted)
            ]
            .spacing(12)
            .align_x(Alignment::Center)
            .into()
        };
        return container(inner).center(Fill).style(panel(p.bg)).into();
    };

    let show_html = app.config.ui.prefer_html && msg.html.is_some();
    let kind = app.current_kind();
    let mut tools = vec![
        tooltip(
            tool_btn(p, icons::CLOSE, None, Message::CloseReader),
            t("close"),
            p,
        ),
        tool_btn(p, icons::REPLY, Some(t("reply")), Message::Reply),
        tooltip(
            tool_btn(p, icons::REPLY_ALL, None, Message::ReplyAll),
            t("reply_all"),
            p,
        ),
        tool_btn(p, icons::FORWARD, Some(t("forward")), Message::Forward),
    ];
    if kind != FolderKind::Archive {
        tools.push(tool_btn(p, icons::ARCHIVE, Some(t("archive")), Message::Archive));
    }
    let delete_tip = if kind == FolderKind::Trash || app.folder_by_kind(FolderKind::Trash).is_none() {
        t("delete_forever")
    } else {
        t("delete")
    };
    tools.push(tooltip(
        tool_btn(p, icons::TRASH, None, Message::Delete),
        delete_tip,
        p,
    ));
    tools.push(tooltip(
        tool_btn(p, icons::MAIL, None, Message::ToggleRead),
        t("mark_unread"),
        p,
    ));
    if kind == FolderKind::Spam {
        tools.push(tooltip(
            tool_btn(p, icons::INBOX, None, Message::SpamOpen),
            t("spam_not"),
            p,
        ));
    } else {
        tools.push(tooltip(
            tool_btn(p, icons::SPAM, None, Message::SpamOpen),
            t("spam_report"),
            p,
        ));
        tools.push(tooltip(
            tool_btn(p, icons::BLOCK, None, Message::BlockSender),
            t("block_sender"),
            p,
        ));
    }
    tools.push(tooltip(
        tool_btn(p, icons::PRINTER, None, Message::Print),
        t("print"),
        p,
    ));
    let starred = app
        .headers
        .iter()
        .chain(app.search_results.iter().flatten())
        .any(|h| h.uid == msg.uid && h.flagged);
    tools.push(tooltip(
        button(
            container(icon(
                if starred { icons::STAR_FILLED } else { icons::STAR },
                18.0,
                if starred { theme::hex("#E2A400") } else { p.text2 },
            ))
            .center_y(Fill),
        )
        .height(44)
        .padding([0, 13])
        .style(ghost(p, p.text2))
        .on_press(Message::ToggleStar(vec![msg.uid], !starred)),
        if starred { t("star_remove") } else { t("star_add") },
        p,
    ));
    tools.push(space::horizontal().into());
    if msg.html.is_some() {
        let label = if show_html { t("view_text") } else { t("view_html") };
        tools.push(tooltip(
            button(text(label).size(12).font(medium()))
                .padding([6, 10])
                .style(outline_btn(p, p.text2))
                .on_press(Message::ToggleHtml),
            t("view_toggle_tip"),
            p,
        ));
    }
    if let Some(pgp) = &msg.pgp {
        let chip = |label: &'static str, bg: Color, fg: Color| -> Element<'a, Message> {
            container(
                row![
                    icon(icons::LOCK, 14.0, fg),
                    text(label).size(12).font(medium()).wrapping(Wrapping::None)
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            )
            .padding([6, 10])
            .style(chip_box(bg, fg, 8.0))
            .into()
        };
        let warn_bg = mix(p.bg, p.danger, 0.1);
        if pgp.error.is_some() {
            tools.push(chip(t("badge_cannot_decrypt"), warn_bg, p.danger));
        } else if pgp.encrypted {
            tools.push(chip(t("badge_encrypted"), p.sec_bg, p.sec_text));
        }
        match pgp.signature {
            Some(crate::mail::SigState::Valid) => tools.push(chip(t("badge_signed"), p.sec_bg, p.sec_text)),
            Some(crate::mail::SigState::Invalid) => {
                tools.push(chip(t("badge_sig_invalid"), warn_bg, p.danger))
            }
            Some(crate::mail::SigState::UnknownKey) => {
                tools.push(chip(t("badge_sig_unknown"), p.chip, p.text2))
            }
            None => {}
        }
    }
    let toolbar = container(dir_row(tools).spacing(4).align_y(Alignment::Center))
        .padding([10, 16])
        .width(Fill);

    let mut recipients = msg.to.clone();
    if !msg.cc.is_empty() {
        recipients = format!("{recipients}, {}: {}", t("cc"), msg.cc);
    }
    let me_line = tf("to_line", &[("to", &trunc(&recipients, 70))]);
    let sender = dir_row(vec![
        avatar(
            initials(&msg.from),
            avatar_color(&msg.from_address),
            Color::WHITE,
            44.0,
        ),
        column![
            dir_row(vec![
                text(trunc(&msg.from, 40))
                    .size(15)
                    .font(semibold())
                    .color(p.text)
                    .into(),
                text(format!("<{}>", msg.from_address))
                    .size(15)
                    .color(p.muted)
                    .into(),
            ])
            .spacing(6),
            text(format!("{me_line} · {}", long_date(msg.timestamp)))
                .size(13)
                .color(p.muted),
        ]
        .spacing(2)
        .width(Fill)
        .into(),
    ])
    .spacing(14)
    .align_y(Alignment::Center);

    let body: Element<'a, Message> = if show_html {
        html_body(app, p)
    } else {
        let shown = text_editor(&app.body)
            .highlight_with::<crate::linkify::Highlighter>(
                crate::linkify::Settings(p.accent_text),
                crate::linkify::format,
            )
            .on_action(Message::BodyAction)
            .size(15)
            .line_height(1.65)
            .padding(0)
            .style(move |_, _| text_editor::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                placeholder: p.muted,
                value: p.text2,
                selection: mix(p.accent, p.bg, 0.7),
            });
        // An invisible copy laid out the same way under the shown text:
        // the app moves its cursor to the mouse to find links under it,
        // without touching the user's selection.
        let probe = text_editor(&app.body_probe)
            .size(15)
            .line_height(1.65)
            .padding(0)
            .style(|_, _| text_editor::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                placeholder: Color::TRANSPARENT,
                value: Color::TRANSPARENT,
                selection: Color::TRANSPARENT,
            });
        mouse_area(super::hand::hand(stack![probe, shown], app.body_link_hover))
            .on_move(Message::BodyHover)
            .on_exit(Message::BodyLeave)
            .into()
    };

    let mut content = column![
        text(&msg.subject)
            .size(26)
            .font(INTER_BOLD)
            .color(p.text)
            .line_height(1.25),
    ]
    .spacing(24)
    // Fill (up to 820): a shrinking column would size the HTML message to
    // its widest sibling instead of the room the reader has.
    .width(Fill)
    .max_width(820);
    content = content.push(sender);
    if let Some(card) = super::invite::view(app, msg.uid, p) {
        content = content.push(card);
    }
    content = content.push(body);

    if !msg.attachments.is_empty() {
        let label = if msg.attachments.len() == 1 {
            t("attachments_one").to_string()
        } else {
            tf("attachments_many", &[("n", &msg.attachments.len().to_string())])
        };
        let mut cards = Row::new().spacing(12);
        for (i, a) in msg.attachments.iter().enumerate() {
            let previewable = crate::preview::kind(&a.name, &a.data) != crate::preview::Kind::None;
            // Thumbnail (image / first PDF page) or the extension.
            let thumb: Element<'a, Message> = match app.thumbs.get(&i) {
                Some(h) => container(
                    iced::widget::image(h.clone())
                        .width(56)
                        .height(56)
                        .content_fit(iced::ContentFit::Cover),
                )
                .clip(true)
                .style(move |_| container::Style {
                    border: Border {
                        color: p.line,
                        width: 1.0,
                        radius: radius(8.0),
                    },
                    ..Default::default()
                })
                .into(),
                None => container(text(extension(&a.name)).font(MONO).size(11).color(p.accent_text))
                    .center_x(56)
                    .center_y(56)
                    .style(chip_box(p.chip, p.accent_text, 8.0))
                    .into(),
            };
            let small_btn = |svg: &'static str, tip: &'static str, msg: Message| -> Element<'a, Message> {
                tooltip(
                    button(icon(svg, 16.0, p.text2))
                        .width(32)
                        .height(32)
                        .padding(8)
                        .style(ghost(p, p.text2))
                        .on_press(msg),
                    tip,
                    p,
                )
            };
            let mut buttons = Row::new().spacing(2);
            if previewable {
                buttons = buttons.push(small_btn(
                    icons::EYE,
                    t("preview_attachment"),
                    Message::PreviewAttachment(i),
                ));
            }
            buttons = buttons.push(small_btn(
                icons::DOWNLOAD,
                t("download"),
                Message::SaveAttachment(i),
            ));
            let info = button(
                row![
                    thumb,
                    column![
                        text(trunc(&a.name, 28)).size(14).font(medium()).color(p.text),
                        text(human_size(a.data.len())).size(12).color(p.muted),
                    ]
                    .spacing(2),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding(0)
            .style(move |_, _| button::Style {
                text_color: p.text,
                ..Default::default()
            })
            .on_press(if previewable {
                Message::PreviewAttachment(i)
            } else {
                Message::SaveAttachment(i)
            });
            let card = container(row![info, buttons].spacing(10).align_y(Alignment::Center))
                .padding(Padding {
                    top: 8.0,
                    right: 8.0,
                    bottom: 8.0,
                    left: 8.0,
                })
                .style(move |_| container::Style {
                    background: Some(Background::Color(p.bg)),
                    border: Border {
                        color: p.line,
                        width: 1.0,
                        radius: radius(12.0),
                    },
                    ..Default::default()
                });
            cards = cards.push(card);
        }
        content = content.push(
            column![
                text(label).size(13).font(semibold()).color(p.text2),
                cards.wrap().vertical_spacing(12)
            ]
            .spacing(10),
        );
    }

    // Sticky Reply / Forward, centered at the bottom of the reading pane.
    let action = |svg: &'static str, label: &'static str, key: &'static str, msg: Message| {
        button(
            row![
                icon(svg, 16.0, p.text),
                text(label).size(14).font(medium()).color(p.text),
                kbd(key, p),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([10, 16])
        .style(move |_, st| button::Style {
            background: Some(Background::Color(if matches!(st, button::Status::Hovered) {
                p.chip
            } else {
                p.bg
            })),
            text_color: p.text,
            border: Border {
                color: p.field,
                width: 1.0,
                radius: radius(999.0),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.12),
                offset: Vector::new(0.0, 4.0),
                blur_radius: 14.0,
            },
            ..Default::default()
        })
        .on_press(msg)
    };
    let actions = container(
        row![
            action(icons::REPLY, t("reply"), "R", Message::Reply),
            action(icons::REPLY_ALL, t("reply_all"), "A", Message::ReplyAll),
            action(icons::FORWARD, t("forward"), "F", Message::Forward),
        ]
        .spacing(12),
    )
    .center_x(Fill)
    .align_bottom(Fill)
    .padding(Padding {
        top: 0.0,
        right: 0.0,
        bottom: 18.0,
        left: 0.0,
    });
    // Room so the end of the message is not hidden behind the buttons.
    content = content.push(space().height(56));

    let mut col = column![toolbar, hline(p.line)];
    let scroll = scrollable(container(content).padding([28, 40]).width(Fill))
        .id(READER_ID)
        .height(Fill)
        .style(scroll_style(p));
    // Destination of the link under the pointer, like a browser's status bar.
    let status: Element<'a, Message> = match &app.hover_link {
        Some(url) if show_html => container(
            container(text(trunc(url, 70)).size(12).font(MONO).color(p.text2))
                .padding([4, 10])
                .style(chip_box(p.chip, p.text2, 6.0)),
        )
        .padding(8)
        .align_bottom(Fill)
        .into(),
        _ => space().into(),
    };
    col = col.push(stack![scroll, actions, status]);
    container(col).width(Fill).height(Fill).style(panel(p.bg)).into()
}

/// How many characters of text at `size` px fit in `px` (Inter averages
/// about half an em per character; the text is also clipped).
fn fit(px: f32, size: f32) -> usize {
    (px / (size * 0.5)).max(8.0) as usize
}

/// One message of an expanded conversation in the list: a compact,
/// indented row (unread dot, sender, preview, date); a click opens it.
fn thread_row<'a>(
    app: &'a App,
    m: &'a ferrowl_core::mail::Header,
    wide: bool,
    pad: f32,
    p: Palette,
) -> Element<'a, Message> {
    let current = app.selected == Some(m.uid);
    let unread = !m.seen;
    let mut line: Vec<Element<'a, Message>> = vec![
        container(space().width(6).height(6))
            .style(chip_box(
                if unread { p.accent } else { Color::TRANSPARENT },
                p.accent,
                3.0,
            ))
            .into(),
        text(trunc(
            &m.from,
            if wide {
                28
            } else {
                fit(app.list_width() - 160.0, 13.0)
            },
        ))
        .size(13)
        .font(if unread { semibold() } else { INTER })
        .color(p.text)
        .wrapping(Wrapping::None)
        .width(if wide { Length::Fixed(200.0) } else { Fill })
        .into(),
    ];
    if wide {
        line.push(
            container(
                text(m.preview.as_str())
                    .size(13)
                    .color(p.muted)
                    .wrapping(Wrapping::None),
            )
            .width(Fill)
            .clip(true)
            .into(),
        );
    }
    if m.has_attachment {
        line.push(icon(icons::CLIP, 13.0, p.muted));
    }
    line.push(
        text(short_date(m.timestamp))
            .font(MONO)
            .size(12)
            .color(p.muted)
            .into(),
    );
    let row_btn = button(dir_row(line).spacing(8).align_y(Alignment::Center))
        .width(Fill)
        .padding({
            // Indented past the row's arrow and checkbox (mirrored in RTL).
            let (start, end) = (52.0, 20.0);
            let rtl = i18n::is_rtl();
            Padding {
                top: pad * 0.6,
                bottom: pad * 0.6,
                left: if rtl { end } else { start },
                right: if rtl { start } else { end },
            }
        })
        .style(move |_, st| button::Style {
            background: Some(Background::Color(if current {
                p.soft
            } else if matches!(st, button::Status::Hovered) {
                mix(p.bg, p.soft, 0.5)
            } else {
                mix(p.bg, p.chip, 0.4)
            })),
            text_color: p.text,
            ..Default::default()
        })
        .on_press(Message::Open(m.uid));
    let item: Element<'a, Message> = if current {
        let bar = container(space().width(3).height(Fill)).style(panel(p.accent));
        let bar = if i18n::is_rtl() {
            container(bar).align_x(Horizontal::Right).width(Fill).height(Fill)
        } else {
            container(bar).height(Fill)
        };
        stack![row_btn, bar].into()
    } else {
        row_btn.into()
    };
    let uid = m.uid;
    mouse_area(item)
        .on_enter(Message::RowEnter(uid))
        .on_exit(Message::RowExit(uid))
        .on_right_press(Message::OpenMenu(uid))
        .into()
}

/// The HTML message as rendered tiles, with link hover/click.
fn html_body<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    // Measures the width the reader gives the message, so it is laid out
    // for exactly that.
    // Only on_resize: it reports every size change, also while the top of
    // the message is scrolled out of view (on_show would gate it on that).
    sensor(container(html_page(app, p)).width(Fill))
        .on_resize(|s| Message::ReaderWidth(s.width))
        .into()
}

fn html_page<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let Some(r) = &app.html_render else {
        return container(text(t("html_loading")).size(14).color(p.muted))
            .padding([24, 0])
            .into();
    };
    let pointer = app.hover_link.is_some();
    let mut tiles = Column::new();
    let mut offset = 0.0;
    let zoom = r.zoom;
    for (handle, h) in &r.tiles {
        let top = offset;
        tiles = tiles.push(
            mouse_area(
                iced::widget::image(handle.clone())
                    .width(r.width as f32)
                    .height(*h * zoom),
            )
            // Back to the message's own coordinates for hit testing.
            .on_move(move |pt| Message::HtmlPointer(pt.x / zoom, top + pt.y / zoom))
            .on_press(Message::HtmlPress)
            .on_exit(Message::HtmlLeave)
            .interaction(if pointer {
                iced::mouse::Interaction::Pointer
            } else {
                iced::mouse::Interaction::Text
            }),
        );
        offset += h;
    }
    // The message is painted as a light page: on a white reader it sits
    // flat like plain text does; on a dark or tinted one a soft frame keeps
    // its edges from looking cut.
    let white = p.bg.r > 0.98 && p.bg.g > 0.98 && p.bg.b > 0.98;
    let page = container(tiles).clip(true).style(move |_| container::Style {
        border: if white {
            Border::default()
        } else {
            Border {
                color: p.line_soft,
                width: 1.0,
                radius: radius(10.0),
            }
        },
        ..Default::default()
    });
    let mut col = Column::new().spacing(12);
    if r.blocked_remote {
        col = col.push(
            container(
                dir_row(vec![
                    text(t("images_blocked"))
                        .size(13)
                        .color(p.text2)
                        .width(Fill)
                        .into(),
                    button(text(t("load_images")).size(13).font(medium()))
                        .padding([6, 12])
                        .style(outline_btn(p, p.text))
                        .on_press(Message::LoadRemoteImages)
                        .into(),
                ])
                .spacing(10)
                .align_y(Alignment::Center),
            )
            .padding([8, 12])
            .style(chip_box(p.chip, p.text2, 10.0)),
        );
    }
    col.push(page).into()
}

// ---------------------------------------------------------------- Appearance panel

fn appearance_popover<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let header = dir_row(vec![
        column![
            text(t("settings")).size(12).color(p.muted),
            text(t("appearance")).size(17).font(INTER_BOLD).color(p.text)
        ]
        .spacing(2)
        .width(Fill)
        .into(),
        button(icon(icons::CLOSE, 18.0, p.text2))
            .width(44)
            .height(44)
            .padding(13)
            .style(ghost(p, p.text2))
            .on_press(Message::CloseAppearance)
            .into(),
    ])
    .align_y(Alignment::Center)
    .padding(Padding {
        top: 14.0,
        right: 12.0,
        bottom: 14.0,
        left: 20.0,
    });

    let all_settings = button(
        container(
            row![
                text(t("all_settings")).size(14).font(semibold()),
                icon(
                    if i18n::is_rtl() {
                        icons::CHEVRON_LEFT
                    } else {
                        icons::CHEVRON_RIGHT
                    },
                    16.0,
                    p.text
                )
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .center_x(Fill)
        .center_y(Fill),
    )
    .width(Fill)
    .height(44)
    .style(move |th, st| {
        let mut s = outline_btn(p, p.text)(th, st);
        s.border.radius = radius(10.0);
        s
    })
    .on_press(Message::OpenSettings(Section::Appearance));

    let body = column![
        text(t("color_theme")).size(13).font(semibold()).color(p.text2),
        theme_grid(&app.config.ui.theme, p, 58.0, 10.0),
        container(text(t("theme_note")).size(13).color(p.text2))
            .padding([12, 14])
            .width(Fill)
            .style(chip_box(p.chip, p.text2, 10.0)),
        all_settings,
    ]
    .spacing(14)
    .padding(Padding {
        top: 18.0,
        right: 20.0,
        bottom: 20.0,
        left: 20.0,
    });

    let card_box = container(column![header, hline(p.line), body])
        .width(440)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.bg)),
            text_color: Some(p.text),
            border: Border {
                color: p.line,
                width: 1.0,
                radius: radius(16.0),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.22),
                offset: Vector::new(0.0, 20.0),
                blur_radius: 50.0,
            },
            ..Default::default()
        });

    // Click outside the panel = close.
    mouse_area(
        container(opaque(card_box))
            .width(Fill)
            .height(Fill)
            .align_x(if i18n::is_rtl() {
                Horizontal::Left
            } else {
                Horizontal::Right
            })
            .align_y(Vertical::Top)
            .padding(Padding {
                top: 64.0,
                right: 20.0,
                bottom: 0.0,
                left: 20.0,
            }),
    )
    .on_press(Message::CloseAppearance)
    .into()
}

// ---------------------------------------------------------------- notifications

pub fn toast_layer<'a>(app: &'a App, p: Palette) -> Option<Element<'a, Message>> {
    let toast = app.toast.as_ref()?;
    let (bg, fg) = (p.ink, p.on_ink);
    let mut col = column![text(&toast.text).size(14).color(fg)].spacing(6);
    if toast.error {
        let details = format!(
            "{}\n\nFerrowl {} · {} {}\n{}",
            toast.text,
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            SUPPORT_EMAIL
        );
        col = col.push(
            row![
                text(tf("err_support_hint", &[("email", SUPPORT_EMAIL)]))
                    .size(12)
                    .color(mix(fg, bg, 0.3))
                    .width(Fill),
                button(text(t("copy_details")).size(12).font(semibold()))
                    .padding([4, 10])
                    .style(move |_, st| button::Style {
                        background: Some(Background::Color(if matches!(st, button::Status::Hovered) {
                            mix(bg, fg, 0.25)
                        } else {
                            mix(bg, fg, 0.12)
                        })),
                        text_color: fg,
                        border: Border {
                            radius: radius(6.0),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .on_press(Message::CopyDetails(details)),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        );
    }
    let close = button(icon(icons::CLOSE, 14.0, fg))
        .padding(6)
        .style(move |_, _| button::Style {
            text_color: fg,
            ..Default::default()
        })
        .on_press(Message::DismissToast(toast.id));
    let accent = if toast.error { p.danger } else { p.accent };
    let boxed = container(
        row![
            // Accent bar beside the text. A fixed height: `Fill` would make
            // the toast as tall as the window.
            container(space().width(4).height(if toast.error { 46 } else { 22 }))
                .style(chip_box(accent, accent, 2.0)),
            col.width(Fill),
            close
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .padding([12, 14])
    .max_width(620)
    .style(move |_| container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(fg),
        border: Border {
            radius: radius(12.0),
            ..Default::default()
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.25),
            offset: Vector::new(0.0, 8.0),
            blur_radius: 24.0,
        },
        ..Default::default()
    });
    Some(
        container(opaque(boxed))
            .width(Fill)
            .height(Fill)
            .align_x(Horizontal::Center)
            .align_y(Vertical::Bottom)
            .padding(24)
            .into(),
    )
}

// ---------------------------------------------------------------- drag and drop

/// Label following the pointer while a message is dragged.
fn drag_label<'a>(app: &'a App, d: &'a crate::app::Drag, p: Palette) -> Element<'a, Message> {
    let chip = container(
        row![
            icon(icons::MAIL, 14.0, p.on_accent),
            text(d.label.clone()).size(13).font(medium())
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([6, 12])
    .style(move |_| container::Style {
        background: Some(Background::Color(p.accent)),
        text_color: Some(p.on_accent),
        border: Border {
            radius: radius(999.0),
            ..Default::default()
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.2),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    });
    container(chip)
        .padding(Padding {
            top: app.cursor.y + 14.0,
            left: app.cursor.x + 14.0,
            right: 0.0,
            bottom: 0.0,
        })
        .width(Fill)
        .height(Fill)
        .into()
}

// ---------------------------------------------------------------- context menu

fn context_menu<'a>(app: &'a App, m: &'a crate::app::ContextMenu, p: Palette) -> Element<'a, Message> {
    use crate::app::CtxAction;
    let uid = m.uid;
    let item = |svg: &'static str, label: String, action: CtxAction, danger: bool| -> Element<'a, Message> {
        let fg = if danger { p.danger } else { p.text };
        button(
            row![icon(svg, 15.0, fg), text(label).size(13).color(fg)]
                .spacing(10)
                .align_y(Alignment::Center),
        )
        .width(Fill)
        .padding([7, 12])
        .style(ghost(p, fg))
        .on_press(Message::MenuAction(uid, action))
        .into()
    };
    // Item with the keyboard shortcut shown as a hint at the end of the row.
    let item_kbd = |svg: &'static str,
                    label: String,
                    key: &'static str,
                    action: CtxAction,
                    danger: bool|
     -> Element<'a, Message> {
        let fg = if danger { p.danger } else { p.text };
        button(
            row![
                icon(svg, 15.0, fg),
                text(label)
                    .size(13)
                    .color(fg)
                    .width(Fill)
                    .wrapping(Wrapping::None),
                text(key)
                    .size(11)
                    .font(MONO)
                    .color(p.muted)
                    .wrapping(Wrapping::None),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .padding([7, 12])
        .style(ghost(p, fg))
        .on_press(Message::MenuAction(uid, action))
        .into()
    };
    let sep = || {
        container(space().height(1))
            .width(Fill)
            .style(panel(p.line_soft))
            .into()
    };

    let header = app.visible().into_iter().find(|h| h.uid == uid).cloned();
    let seen = header.as_ref().is_some_and(|h| h.seen);
    let kind = app.current_kind();

    let items: Vec<Element<'a, Message>> = if m.folders {
        // "Move to…": every selectable folder but the current one.
        let mut list = Column::new().spacing(2);
        for f in app
            .folders
            .iter()
            .filter(|f| f.selectable && Some(&f.name) != app.current_folder.as_ref())
        {
            list = list.push(item(
                kind_icon(f.kind),
                trunc(&f.display(), 32),
                CtxAction::Move(f.name.clone()),
                false,
            ));
        }
        vec![
            container(text(t("move_to")).size(12).font(semibold()).color(p.muted))
                .padding([6, 12])
                .into(),
            scrollable(list)
                .height(iced::Length::Shrink)
                .style(scroll_style(p))
                .into(),
        ]
    } else {
        let mut v = vec![
            item(icons::MAIL, t("open").into(), CtxAction::Open, false),
            sep(),
            item(icons::REPLY, t("reply").into(), CtxAction::Reply, false),
            item(
                icons::REPLY_ALL,
                t("reply_all").into(),
                CtxAction::ReplyAll,
                false,
            ),
            item(icons::FORWARD, t("forward").into(), CtxAction::Forward, false),
            sep(),
            item(
                icons::MAIL,
                if seen { t("mark_unread") } else { t("mark_read") }.into(),
                CtxAction::ToggleRead,
                false,
            ),
        ];
        if kind != FolderKind::Archive {
            v.push(item(
                icons::ARCHIVE,
                t("archive").into(),
                CtxAction::Archive,
                false,
            ));
        }
        v.push(item(
            icons::FOLDER,
            format!("{}…", t("move_to")),
            CtxAction::MoveMenu,
            false,
        ));
        v.push(sep());
        if kind == FolderKind::Trash {
            v.push(item(
                icons::TRASH,
                t("delete_forever").into(),
                CtxAction::Delete,
                true,
            ));
        } else {
            v.push(item(icons::TRASH, t("delete").into(), CtxAction::Delete, true));
            v.push(item_kbd(
                icons::TRASH,
                t("delete_forever").into(),
                "Shift+Del",
                CtxAction::DeleteForever,
                true,
            ));
        }
        v
    };

    let menu = container(Column::with_children(items).spacing(2))
        .width(290)
        .max_height(420)
        .padding(6)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.bg)),
            border: Border {
                color: p.line,
                width: 1.0,
                radius: radius(10.0),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        });
    // Keep the menu inside the window.
    let x = m.at.x.min(app.window_width - 300.0).max(0.0);
    let y = m.at.y.min(app.window_height - 400.0).max(0.0);
    let placed = container(opaque(menu)).padding(Padding {
        top: y,
        left: x,
        right: 0.0,
        bottom: 0.0,
    });
    // Click anywhere else closes it.
    stack![
        mouse_area(container(space()).width(Fill).height(Fill))
            .on_press(Message::CloseMenu)
            .on_right_press(Message::CloseMenu),
        placed,
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

/// The list's drop-down menus (select, sort, move): opened at the click,
/// closed by a click anywhere else.
fn popup_menu<'a>(app: &'a App, kind: Popup, at: iced::Point, p: Palette) -> Element<'a, Message> {
    use ferrowl_core::sort::SortOrder;
    let item = |label: String, msg: Message, checked: bool, danger: bool| -> Element<'a, Message> {
        let fg = if danger {
            p.danger
        } else if checked {
            p.accent_text
        } else {
            p.text
        };
        button(
            row![
                text(label)
                    .size(14)
                    .color(fg)
                    .width(Fill)
                    .wrapping(Wrapping::None),
                if checked {
                    text("✓").size(14).color(p.accent_text)
                } else {
                    text("")
                },
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .padding([8, 14])
        .style(ghost(p, fg))
        .on_press(msg)
        .into()
    };
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    match kind {
        Popup::Select => {
            for s in [Select::All, Select::Read, Select::Unread, Select::Starred] {
                items.push(item(s.to_string(), Message::SelectBy(s), false, false));
            }
            items.push(item(
                t("select_none").into(),
                Message::SelectBy(Select::None),
                false,
                true,
            ));
        }
        Popup::Sort => {
            items.push(
                container(text(t("sort_by")).size(13).font(semibold()).color(p.text))
                    .padding([8, 14])
                    .into(),
            );
            for o in SortOrder::ALL {
                items.push(item(
                    t(o.key()).into(),
                    Message::SetSort(o),
                    app.config.ui.sort == o,
                    false,
                ));
            }
        }
        Popup::Page => {
            let size = app.config.ui.page_size.max(1) as usize;
            let total = app.list_rows().len();
            let pages = total.div_ceil(size).max(1);
            for n in 0..pages {
                items.push(item(
                    (n + 1).to_string(),
                    Message::GoPage(n),
                    app.page == n,
                    false,
                ));
            }
        }
        Popup::Labels => {
            let targets = app.targets();
            let has = |name: &str| {
                !targets.is_empty()
                    && targets.iter().all(|u| {
                        app.headers
                            .iter()
                            .chain(app.search_results.iter().flatten())
                            .any(|h| h.uid == *u && h.labels.iter().any(|l| l == name))
                    })
            };
            let choices = app.label_choices();
            if choices.is_empty() {
                items.push(
                    container(text(t("labels_none")).size(13).color(p.muted))
                        .padding([8, 14])
                        .into(),
                );
            }
            for f in choices {
                let on = has(&f.name);
                items.push(item(
                    trunc(&app.folder_label(&f.name), 30),
                    Message::ToggleLabel(f.name.clone(), !on),
                    on,
                    false,
                ));
            }
        }
        Popup::Move => {
            for f in app
                .folders
                .iter()
                .filter(|f| f.selectable && app.current_folder.as_deref() != Some(f.name.as_str()))
            {
                items.push(item(
                    trunc(&app.folder_label(&f.name), 30),
                    Message::BulkMove(crate::app::MoveTarget {
                        name: f.name.clone(),
                        label: app.folder_label(&f.name),
                    }),
                    false,
                    false,
                ));
            }
        }
    }
    let menu = container(scrollable(Column::with_children(items).spacing(1)).style(scroll_style(p)))
        .width(240)
        .max_height(420)
        .padding(6)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.bg)),
            border: Border {
                color: p.line,
                width: 1.0,
                radius: radius(10.0),
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
            ..Default::default()
        });
    // Just below the click, kept inside the window.
    let x = (at.x - 20.0).min(app.window_width - 250.0).max(0.0);
    let y = (at.y + 14.0).min(app.window_height - 300.0).max(0.0);
    stack![
        mouse_area(container(space()).width(Fill).height(Fill))
            .on_press(Message::ClosePopup)
            .on_right_press(Message::ClosePopup),
        container(opaque(menu)).padding(Padding {
            top: y,
            left: x,
            right: 0.0,
            bottom: 0.0,
        }),
    ]
    .width(Fill)
    .height(Fill)
    .into()
}

// ---------------------------------------------------------------- attachment preview

fn preview_overlay<'a>(
    app: &'a App,
    i: usize,
    content: Option<&'a crate::preview::Content>,
    p: Palette,
) -> Element<'a, Message> {
    use crate::preview::Content;
    let Some(a) = app.message.as_ref().and_then(|m| m.attachments.get(i)) else {
        return space().into();
    };
    let head = container(
        dir_row(vec![
            column![
                text(trunc(&a.name, 60)).size(15).font(semibold()).color(p.text),
                text(human_size(a.data.len())).size(12).color(p.muted),
            ]
            .spacing(2)
            .width(Fill)
            .into(),
            button(
                row![
                    icon(icons::DOWNLOAD, 16.0, p.on_accent),
                    text(t("download")).size(14).font(semibold())
                ]
                .spacing(8)
                .align_y(Alignment::Center),
            )
            .padding([9, 14])
            .style(accent_btn(p))
            .on_press(Message::SaveAttachment(i))
            .into(),
            button(icon(icons::CLOSE, 18.0, p.text2))
                .width(40)
                .height(40)
                .padding(11)
                .style(ghost(p, p.text2))
                .on_press(Message::ClosePreview)
                .into(),
        ])
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([12, 16])
    .width(Fill);

    let centered = |e: Element<'a, Message>| container(e).center_x(Fill).padding(24);
    let body: Element<'a, Message> = match content {
        None => centered(text(t("html_loading")).size(14).color(p.muted).into()).into(),
        Some(Content::Image(h)) => scrollable(centered(
            iced::widget::image(h.clone())
                .content_fit(iced::ContentFit::ScaleDown)
                .into(),
        ))
        .height(Fill)
        .style(scroll_style(p))
        .into(),
        Some(Content::Svg(h)) => {
            centered(iced::widget::svg(h.clone()).width(Fill).height(Fill).into()).into()
        }
        Some(Content::Pages(pages, total)) => {
            let mut col = Column::new().spacing(16).align_x(Alignment::Center);
            for (h, w, ht) in pages {
                col = col.push(
                    container(
                        iced::widget::image(h.clone())
                            .width(*w as f32 * 0.75)
                            .height(*ht as f32 * 0.75),
                    )
                    .style(move |_| container::Style {
                        shadow: Shadow {
                            color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
                            offset: Vector::new(0.0, 2.0),
                            blur_radius: 10.0,
                        },
                        ..Default::default()
                    }),
                );
            }
            if *total > pages.len() {
                col = col.push(
                    text(tf("pdf_more_pages", &[("n", &(total - pages.len()).to_string())]))
                        .size(13)
                        .color(p.muted),
                );
            }
            scrollable(centered(col.into()))
                .height(Fill)
                .style(scroll_style(p))
                .into()
        }
        Some(Content::Text(t_, cut)) => {
            let mut col = Column::new()
                .spacing(10)
                .push(text(t_.as_str()).font(MONO).size(13).color(p.text));
            if *cut {
                col = col.push(text(t("preview_cut")).size(12).color(p.muted));
            }
            scrollable(container(col).padding(24).width(Fill))
                .height(Fill)
                .style(scroll_style(p))
                .into()
        }
        Some(Content::Error(e)) => centered(
            column![
                text(t("preview_failed")).size(14).color(p.text2),
                text(trunc(e, 200)).size(12).font(MONO).color(p.muted)
            ]
            .spacing(6)
            .into(),
        )
        .into(),
    };

    let dialog = container(column![head, hline(p.line), body])
        .max_width(1000)
        .width(Fill)
        .height(Fill)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(p.chrome)),
            border: Border {
                radius: radius(14.0),
                ..Default::default()
            },
            shadow: Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
                offset: Vector::new(0.0, 12.0),
                blur_radius: 40.0,
            },
            ..Default::default()
        });
    opaque(
        mouse_area(
            container(opaque(dialog))
                .center(Fill)
                .padding(32)
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.45))),
                    ..Default::default()
                }),
        )
        .on_press(Message::ClosePreview),
    )
}

// ---------------------------------------------------------------- delete confirmation

/// Modal "delete forever?" dialog (Enter confirms, Esc or a click outside cancels).
fn delete_dialog<'a>(app: &'a App, p: Palette) -> Element<'a, Message> {
    let subject = app
        .selected
        .and_then(|uid| {
            app.visible()
                .into_iter()
                .find(|h| h.uid == uid)
                .map(|h| h.subject.clone())
        })
        .or_else(|| app.message.as_ref().map(|m| m.subject.clone()))
        .unwrap_or_default();
    let danger_btn = button(
        row![
            icon(icons::TRASH, 16.0, if p.dark { p.bg } else { Color::WHITE }),
            text(t("delete_forever")).size(14).font(semibold())
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([10, 16])
    .style(move |_, st| button::Style {
        background: Some(Background::Color(if matches!(st, button::Status::Hovered) {
            mix(p.danger, Color::BLACK, 0.12)
        } else {
            p.danger
        })),
        text_color: if p.dark { p.bg } else { Color::WHITE },
        border: Border {
            radius: radius(10.0),
            ..Default::default()
        },
        ..Default::default()
    })
    .on_press(Message::ConfirmDelete(true));
    let cancel = button(text(t("cancel")).size(14).font(medium()))
        .padding([10, 16])
        .style(outline_btn(p, p.text))
        .on_press(Message::ConfirmDelete(false));

    // Texts start on the right in right-to-left languages.
    let text_start = if i18n::is_rtl() {
        Alignment::End
    } else {
        Alignment::Start
    };
    let dialog = container(
        column![
            dir_row(vec![
                container(icon(icons::TRASH, 22.0, p.danger))
                    .center_x(44)
                    .center_y(44)
                    .style(chip_box(mix(p.bg, p.danger, 0.12), p.danger, 22.0))
                    .into(),
                column![
                    text(t("confirm_delete_title"))
                        .size(18)
                        .font(INTER_BOLD)
                        .color(p.text),
                    text(trunc(&subject, 70)).size(14).font(medium()).color(p.text2),
                ]
                .spacing(4)
                .width(Fill)
                .align_x(text_start)
                .into(),
            ])
            .spacing(14)
            .align_y(Alignment::Center),
            text(t("cannot_undo")).size(14).color(p.muted),
            dir_row(vec![space::horizontal().into(), cancel.into(), danger_btn.into()]).spacing(10),
        ]
        .spacing(20)
        .align_x(text_start),
    )
    .padding(24)
    .max_width(480)
    .style(move |_| container::Style {
        background: Some(Background::Color(p.bg)),
        border: Border {
            color: p.line,
            width: 1.0,
            radius: radius(14.0),
        },
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.3),
            offset: Vector::new(0.0, 12.0),
            blur_radius: 40.0,
        },
        ..Default::default()
    });
    opaque(
        mouse_area(
            container(opaque(dialog))
                .center(Fill)
                .padding(24)
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.4))),
                    ..Default::default()
                }),
        )
        .on_press(Message::ConfirmDelete(false)),
    )
}
