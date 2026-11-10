//! Common UI building blocks: fonts, icons, styles, formats.

pub mod calendar;
pub mod compose;
pub mod contacts;
pub mod hand;
pub mod insights;
pub mod invite;
pub mod mail_view;
pub mod settings;

use crate::app::Message;
use crate::i18n::{t, tf};
use crate::theme::Palette;
use chrono::{Datelike, Local, TimeZone};
use iced::widget::{button, container, svg, text, text_input};
use iced::{Background, Border, Color, Element, Font, Length, Shadow, Theme, Vector, border, font};

// ---------------------------------------------------------------- font

pub const INTER: Font = Font::with_name("Inter");
pub const INTER_MEDIUM: Font = Font {
    weight: font::Weight::Medium,
    ..INTER
};
pub const INTER_SEMIBOLD: Font = Font {
    weight: font::Weight::Semibold,
    ..INTER
};
pub const INTER_BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..INTER
};
pub const MONO: Font = Font::with_name("JetBrains Mono");

/// CJK system fonts usually ship only Regular and Bold: map the in-between
/// weights so the fallback does not pick an unrelated (e.g. serif) face.
fn cjk() -> bool {
    matches!(crate::i18n::current().code, "zh" | "ja" | "ko")
}

/// Medium weight (Regular for CJK).
pub fn medium() -> Font {
    if cjk() { INTER } else { INTER_MEDIUM }
}

/// Semibold weight (Bold for CJK).
pub fn semibold() -> Font {
    if cjk() { INTER_BOLD } else { INTER_SEMIBOLD }
}

pub const FONTS: [&[u8]; 10] = [
    include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/Inter-Bold.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../../assets/fonts/JetBrainsMono-Medium.ttf"),
    // Arabic in every weight: system installs often lack Medium/SemiBold.
    include_bytes!("../../assets/fonts/NotoSansArabic-Regular.ttf"),
    include_bytes!("../../assets/fonts/NotoSansArabic-Medium.ttf"),
    include_bytes!("../../assets/fonts/NotoSansArabic-SemiBold.ttf"),
    include_bytes!("../../assets/fonts/NotoSansArabic-Bold.ttf"),
];

// ---------------------------------------------------------------- icons (from the mockups)

macro_rules! icon_svg {
    ($($p:expr),*) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
            $($p,)*
            "</svg>"
        )
    };
}

pub mod icons {
    pub const STAR: &str =
        icon_svg!(r#"<path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z"/>"#);
    /// Filled star (the svg color fills the shape too).
    pub const STAR_FILLED: &str = concat!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="black" stroke="black" stroke-width="2" stroke-linejoin="round">"#,
        r#"<path d="M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9z"/>"#,
        "</svg>"
    );
    pub const SORT: &str = icon_svg!(
        r#"<path d="M7 4v16"/><path d="M3 8l4-4 4 4"/><path d="M17 20V4"/><path d="M13 16l4 4 4-4"/>"#
    );
    pub const PRINTER: &str = icon_svg!(
        r#"<path d="M6 9V3h12v6"/><rect x="3" y="9" width="18" height="8" rx="2"/><path d="M6 14h12v7H6z"/>"#
    );
    pub const BLOCK: &str = icon_svg!(r#"<circle cx="12" cy="12" r="9"/><path d="M5.6 5.6l12.8 12.8"/>"#);
    pub const TAG: &str = icon_svg!(
        r#"<path d="M20.6 13.4l-7.2 7.2a2 2 0 0 1-2.8 0L3 13V3h10l7.6 7.6a2 2 0 0 1 0 2.8z"/><circle cx="7.5" cy="7.5" r="1.5"/>"#
    );
    pub const PLUS: &str = icon_svg!(r#"<path d="M12 5v14"/><path d="M5 12h14"/>"#);
    pub const CALENDAR: &str = icon_svg!(
        r#"<rect x="3" y="4" width="18" height="18" rx="2"/><path d="M16 2v4"/><path d="M8 2v4"/><path d="M3 10h18"/>"#
    );
    pub const PIN: &str = icon_svg!(
        r#"<path d="M20 10c0 6-8 12-8 12s-8-6-8-12a8 8 0 0 1 16 0z"/><circle cx="12" cy="10" r="3"/>"#
    );
    pub const REPEAT: &str = icon_svg!(
        r#"<path d="M17 1l4 4-4 4"/><path d="M3 11V9a4 4 0 0 1 4-4h14"/><path d="M7 23l-4-4 4-4"/><path d="M21 13v2a4 4 0 0 1-4 4H3"/>"#
    );
    pub const CLOCK: &str = icon_svg!(r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>"#);
    pub const CONTACTS: &str = icon_svg!(
        r#"<circle cx="9" cy="8" r="4"/><path d="M2 21v-1a7 7 0 0 1 14 0v1"/><path d="M16 3.5a4 4 0 0 1 0 9"/><path d="M22 21v-1a7 7 0 0 0-4-6.3"/>"#
    );
    pub const PHONE: &str = icon_svg!(
        r#"<path d="M22 16.9v3a2 2 0 0 1-2.2 2 19.8 19.8 0 0 1-8.6-3.1 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.1 4.2 2 2 0 0 1 4.1 2h3a2 2 0 0 1 2 1.7c.1.9.4 1.8.7 2.7a2 2 0 0 1-.5 2.1L8 9.8a16 16 0 0 0 6 6l1.3-1.3a2 2 0 0 1 2.1-.4c.9.3 1.8.6 2.7.7a2 2 0 0 1 1.7 2z"/>"#
    );
    pub const SEARCH: &str = icon_svg!(r#"<circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/>"#);
    pub const SYNC: &str = icon_svg!(
        r#"<path d="M21 12a9 9 0 0 1-15.5 6.2L3 16"/><path d="M3 12a9 9 0 0 1 15.5-6.2L21 8"/><path d="M21 3v5h-5"/><path d="M3 21v-5h5"/>"#
    );
    pub const GEAR: &str = icon_svg!(
        r#"<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>"#
    );
    pub const PEN: &str =
        icon_svg!(r#"<path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>"#);
    pub const INBOX: &str = icon_svg!(
        r#"<path d="M22 12h-6l-2 3h-4l-2-3H2"/><path d="M5.5 5h13L22 12v6a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2v-6z"/>"#
    );
    pub const SEND: &str = icon_svg!(r#"<path d="M22 2L11 13"/><path d="M22 2l-7 20-4-9-9-4z"/>"#);
    pub const DRAFT: &str = icon_svg!(
        r#"<path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M14 3v6h6"/>"#
    );
    pub const ARCHIVE: &str = icon_svg!(
        r#"<rect x="2" y="4" width="20" height="5" rx="1"/><path d="M4 9v10a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9"/><path d="M10 13h4"/>"#
    );
    pub const SPAM: &str = icon_svg!(
        r#"<path d="M12 2l9 4v6c0 5-4 9-9 10-5-1-9-5-9-10V6z"/><path d="M12 8v5"/><path d="M12 16h.01"/>"#
    );
    pub const TRASH: &str =
        icon_svg!(r#"<path d="M3 6h18"/><path d="M8 6V4h8v2"/><path d="M6 6l1 15h10l1-15"/>"#);
    pub const FOLDER: &str = icon_svg!(
        r#"<path d="M3 6a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"#
    );
    pub const CHEVRON_RIGHT: &str = icon_svg!(r#"<path d="M9 6l6 6-6 6"/>"#);
    pub const CHEVRON_DOWN: &str = icon_svg!(r#"<path d="M6 9l6 6 6-6"/>"#);
    pub const CHEVRON_LEFT: &str = icon_svg!(r#"<path d="M15 18l-6-6 6-6"/>"#);
    pub const CLIP: &str =
        icon_svg!(r#"<path d="M21 11l-9 9a5 5 0 0 1-7-7l9-9a3.5 3.5 0 0 1 5 5l-9 9a2 2 0 0 1-3-3l8-8"/>"#);
    pub const REPLY: &str = icon_svg!(r#"<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 6 6v5"/>"#);
    pub const REPLY_ALL: &str =
        icon_svg!(r#"<path d="M7 14L2 9l5-5"/><path d="M12 14L7 9l5-5"/><path d="M7 9h8a6 6 0 0 1 6 6v5"/>"#);
    pub const EYE: &str = icon_svg!(
        r#"<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>"#
    );
    pub const EYE_OFF: &str = icon_svg!(
        r#"<path d="M9.9 5.2A9.6 9.6 0 0 1 12 5c6.5 0 10 7 10 7a17.6 17.6 0 0 1-2.6 3.5"/>"#,
        r#"<path d="M6.6 6.6C3.7 8.4 2 12 2 12s3.5 7 10 7a9.7 9.7 0 0 0 5.4-1.6"/>"#,
        r#"<path d="M9.9 9.9a3 3 0 0 0 4.2 4.2"/><path d="M2 2l20 20"/>"#
    );
    pub const FORWARD: &str = icon_svg!(r#"<path d="M15 14l5-5-5-5"/><path d="M20 9H10a6 6 0 0 0-6 6v5"/>"#);
    pub const LOCK: &str =
        icon_svg!(r#"<rect x="4" y="11" width="16" height="10" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>"#);
    pub const CLOSE: &str = icon_svg!(r#"<path d="M6 6l12 12"/><path d="M18 6L6 18"/>"#);
    pub const MAIL: &str =
        icon_svg!(r#"<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M3 7l9 6 9-6"/>"#);
    pub const DOWNLOAD: &str =
        icon_svg!(r#"<path d="M12 3v12"/><path d="M7 10l5 5 5-5"/><path d="M5 21h14"/>"#);
    // Compose formatting toolbar.
    pub const BOLD: &str =
        icon_svg!(r#"<path d="M7 5h6a3.5 3.5 0 0 1 0 7H7z"/><path d="M7 12h7a3.5 3.5 0 0 1 0 7H7z"/>"#);
    pub const ITALIC: &str = icon_svg!(r#"<path d="M19 4h-9"/><path d="M14 20H5"/><path d="M15 4L9 20"/>"#);
    pub const STRIKE: &str = icon_svg!(
        r#"<path d="M4 12h16"/><path d="M16 6.5A4 4 0 0 0 12.5 5h-1A3.5 3.5 0 0 0 8 8.5c0 1.5 1 2.6 3 3.5"/><path d="M8 17.5A4 4 0 0 0 11.5 19h1a3.5 3.5 0 0 0 3.5-3.5c0-.6-.1-1.1-.4-1.5"/>"#
    );
    pub const HEADING: &str = icon_svg!(r#"<path d="M6 4v16"/><path d="M18 4v16"/><path d="M6 12h12"/>"#);
    pub const LIST: &str = icon_svg!(
        r#"<path d="M9 6h11"/><path d="M9 12h11"/><path d="M9 18h11"/><circle cx="4.5" cy="6" r="1"/><circle cx="4.5" cy="12" r="1"/><circle cx="4.5" cy="18" r="1"/>"#
    );
    pub const LIST_NUM: &str = icon_svg!(
        r#"<path d="M10 6h10"/><path d="M10 12h10"/><path d="M10 18h10"/><path d="M4 4h1v5"/><path d="M4 9h2"/><path d="M4 14.5a1.5 1.5 0 0 1 3 .5L4 19h3"/>"#
    );
    pub const QUOTE: &str = icon_svg!(
        r#"<path d="M4 18v-5a6 6 0 0 1 6-6"/><path d="M4 13h5v5H4z"/><path d="M14 18v-5a6 6 0 0 1 6-6"/><path d="M14 13h5v5h-5z"/>"#
    );
    pub const CODE: &str = icon_svg!(r#"<path d="M8 7l-5 5 5 5"/><path d="M16 7l5 5-5 5"/>"#);
    pub const LINK: &str = icon_svg!(
        r#"<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>"#
    );
}

pub fn icon<'a>(src: &'static str, size: f32, color: Color) -> Element<'a, Message> {
    svg(svg::Handle::from_memory(src.as_bytes()))
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
        .into()
}

/// Header logo: the background follows the active theme's `logo` color.
pub fn logo<'a>(p: &Palette, size: f32) -> Element<'a, Message> {
    let c = to_hex(p.logo);
    let src = format!(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><rect width="64" height="64" rx="15" fill="{c}"/><polygon points="13,25 13,14.5 22.5,21" fill="#FBF3EA" stroke="#FBF3EA" stroke-width="1.2" stroke-linejoin="round"/><polygon points="51,25 51,14.5 41.5,21" fill="#FBF3EA" stroke="#FBF3EA" stroke-width="1.2" stroke-linejoin="round"/><rect x="13" y="20.75" width="38" height="27.75" rx="4" fill="#FBF3EA"/><path d="M15 23.25 L32 36.25 L49 23.25" fill="none" stroke="{c}" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"/><polygon points="23.75,32.5 28.08,35 28.08,40 23.75,42.5 19.42,40 19.42,35" fill="#2A1A12"/><polygon points="40.25,32.5 44.58,35 44.58,40 40.25,42.5 35.92,40 35.92,35" fill="#2A1A12"/><polygon points="30,37.5 34,37.5 32,41" fill="{c}"/></svg>"##
    );
    svg(svg::Handle::from_memory(src.into_bytes()))
        .width(size)
        .height(size)
        .into()
}

fn to_hex(c: Color) -> String {
    let [r, g, b, _] = c.into_rgba8();
    format!("#{r:02X}{g:02X}{b:02X}")
}

// ---------------------------------------------------------------- styles

pub fn radius(r: f32) -> border::Radius {
    border::Radius::from(r)
}

pub fn panel(bg: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        ..Default::default()
    }
}

pub fn card(p: Palette, border_color: Color, r: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(p.bg)),
        text_color: Some(p.text),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: radius(r),
        },
        ..Default::default()
    }
}

pub fn chip_box(bg: Color, fg: Color, r: f32) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(fg),
        border: Border {
            radius: radius(r),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// iced has no right/bottom-only border: we use separate lines (`hline`/`vline`).
pub fn hline<'a>(c: Color) -> Element<'a, Message> {
    container(iced::widget::space().width(Length::Fill).height(1))
        .width(Length::Fill)
        .height(1)
        .style(panel(c))
        .into()
}

pub fn vline<'a>(c: Color) -> Element<'a, Message> {
    container(iced::widget::space().width(1).height(Length::Fill))
        .width(1)
        .height(Length::Fill)
        .style(panel(c))
        .into()
}

/// Transparent button that gets colored on hover.
pub fn ghost(p: Palette, fg: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: match status {
            button::Status::Hovered | button::Status::Pressed => Some(Background::Color(p.chip)),
            _ => None,
        },
        text_color: if matches!(status, button::Status::Disabled) {
            p.muted
        } else {
            fg
        },
        border: Border {
            radius: radius(10.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn accent_btn(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let bg = match status {
            button::Status::Hovered => mix(p.accent, p.text, 0.12),
            button::Status::Pressed => mix(p.accent, p.text, 0.22),
            button::Status::Disabled => mix(p.accent, p.bg, 0.5),
            _ => p.accent,
        };
        button::Style {
            background: Some(Background::Color(bg)),
            text_color: p.on_accent,
            border: Border {
                radius: radius(10.0),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

pub fn outline_btn(p: Palette, fg: Color) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered | button::Status::Pressed => p.chip,
            _ => p.bg,
        })),
        text_color: if matches!(status, button::Status::Disabled) {
            p.muted
        } else {
            fg
        },
        border: Border {
            color: p.field,
            width: 1.0,
            radius: radius(8.0),
        },
        ..Default::default()
    }
}

/// Sidebar item (folders, settings sections).
pub fn nav_item(p: Palette, active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: if active {
            Some(Background::Color(p.side_active))
        } else if matches!(status, button::Status::Hovered) {
            Some(Background::Color(mix(p.side, p.side_active, 0.5)))
        } else {
            None
        },
        text_color: if active { p.active_text } else { p.text2 },
        border: Border {
            radius: radius(9.0),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Selectable segment (All / Unread / Attachments, density).
pub fn segment(p: Palette, on: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| button::Style {
        background: if on {
            Some(Background::Color(p.bg))
        } else if matches!(status, button::Status::Hovered) {
            Some(Background::Color(mix(p.chip, p.bg, 0.5)))
        } else {
            None
        },
        text_color: if on { p.text } else { p.text2 },
        border: Border {
            radius: radius(7.0),
            ..Default::default()
        },
        shadow: if on {
            Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.12),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 2.0,
            }
        } else {
            Shadow::default()
        },
        ..Default::default()
    }
}

pub fn input(p: Palette, r: f32) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| text_input::Style {
        background: Background::Color(p.bg),
        border: Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                p.accent
            } else {
                p.field
            },
            width: 1.0,
            radius: radius(r),
        },
        icon: p.muted,
        placeholder: p.muted,
        value: p.text,
        selection: mix(p.accent, p.bg, 0.7),
    }
}

/// Borderless field (used inside containers that already have a border).
/// A small button that opens a menu, drawn like a drop-down field.
pub fn pick_btn(p: Palette) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let active = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: Some(Background::Color(if active {
                mix(p.chip, p.soft, 0.6)
            } else {
                p.chip
            })),
            text_color: p.text,
            border: Border {
                color: if active { p.accent } else { p.line },
                width: 1.0,
                radius: radius(7.0),
            },
            ..Default::default()
        }
    }
}

/// Drop-down lists in the theme's colors (the default iced look is grey
/// and square): a field like the filter chips, and a menu like a card.
pub fn pick_style(
    p: Palette,
) -> impl Fn(&Theme, iced::widget::pick_list::Status) -> iced::widget::pick_list::Style {
    move |_, status| {
        use iced::widget::pick_list::Status;
        let active = matches!(status, Status::Hovered | Status::Opened { .. });
        iced::widget::pick_list::Style {
            text_color: p.text,
            placeholder_color: p.text2,
            handle_color: p.text2,
            background: Background::Color(if active { mix(p.chip, p.soft, 0.6) } else { p.chip }),
            border: Border {
                color: if active { p.accent } else { p.line },
                width: 1.0,
                radius: radius(9.0),
            },
        }
    }
}

pub fn menu_style(p: Palette) -> impl Fn(&Theme) -> iced::widget::overlay::menu::Style {
    move |_| iced::widget::overlay::menu::Style {
        background: Background::Color(p.bg),
        border: Border {
            color: p.line,
            width: 1.0,
            radius: radius(10.0),
        },
        text_color: p.text,
        selected_text_color: p.active_text,
        selected_background: Background::Color(p.soft),
        shadow: Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.18),
            offset: Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
    }
}

pub fn bare_input(p: Palette) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, _| text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: p.muted,
        placeholder: p.muted,
        value: p.text,
        selection: mix(p.accent, p.bg, 0.7),
    }
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgb(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
    )
}

// ---------------------------------------------------------------- text

pub use ferrowl_core::initials;

/// Avatar color, stable per address.
pub fn avatar_color(key: &str) -> Color {
    const COLORS: [&str; 7] = [
        "#1E5AA8", "#2F7D4F", "#6B4FA0", "#A63A0C", "#4A443F", "#8466BE", "#3C78A0",
    ];
    let h = key
        .to_lowercase()
        .bytes()
        .fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
    crate::theme::hex(COLORS[h as usize % COLORS.len()])
}

pub fn avatar<'a>(label: String, bg: Color, fg: Color, size: f32) -> Element<'a, Message> {
    container(text(label).font(semibold()).size(size * 0.36).color(fg))
        .width(size)
        .height(size)
        .center_x(size)
        .center_y(size)
        .style(chip_box(bg, fg, size / 2.0))
        .into()
}

/// Truncates to `max` characters with an ellipsis (iced 0.14 has no ellipsis).
pub fn trunc(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out = out.trim_end().to_string();
        out.push('…');
        out
    }
}

fn local(ts: i64) -> Option<chrono::DateTime<Local>> {
    if ts == 0 {
        return None;
    }
    Local.timestamp_opt(ts, 0).single()
}

fn month(m0: u32, short: bool) -> &'static str {
    let key = format!("{}_{}", if short { "mon" } else { "month" }, m0 + 1);
    t(&key)
}

/// Compact time for the list: 09:42 · Yesterday · Oct 29 · 29/10/24
pub fn short_date(ts: i64) -> String {
    let Some(d) = local(ts) else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let day = d.date_naive();
    if day == today {
        d.format("%H:%M").to_string()
    } else if Some(day) == today.pred_opt() {
        t("short_yesterday").to_string()
    } else if day.year() == today.year() {
        tf(
            "short_this_year",
            &[
                ("day", &day.day().to_string()),
                ("mon", month(day.month0(), true)),
            ],
        )
    } else {
        d.format(t("short_other_fmt")).to_string()
    }
}

/// Long date for the reader: today, 09:42 · yesterday, 18:03 · October 29, 16:47
pub fn long_date(ts: i64) -> String {
    let Some(d) = local(ts) else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let day = d.date_naive();
    let time = d.format("%H:%M").to_string();
    let dd = day.day().to_string();
    let yy = day.year().to_string();
    let mm = month(day.month0(), false);
    if day == today {
        tf("date_today", &[("time", &time)])
    } else if Some(day) == today.pred_opt() {
        tf("date_yesterday", &[("time", &time)])
    } else if day.year() == today.year() {
        tf("date_this_year", &[("day", &dd), ("month", mm), ("time", &time)])
    } else {
        tf(
            "date_other",
            &[("day", &dd), ("month", mm), ("year", &yy), ("time", &time)],
        )
    }
}

pub fn human_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes.div_ceil(1024))
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)).replace('.', ",")
    }
}

pub fn extension(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_uppercase())
        .filter(|e| e.len() <= 4)
        .unwrap_or_else(|| "FILE".into())
}

// ---------------------------------------------------------------- shared widgets

/// Horizontal row that mirrors its children for right-to-left languages (Arabic).
pub fn dir_row<'a>(mut children: Vec<Element<'a, Message>>) -> iced::widget::Row<'a, Message> {
    if crate::i18n::is_rtl() {
        children.reverse();
    }
    iced::widget::Row::with_children(children)
}

/// Keyboard shortcut badge, e.g. "Ctrl K".
pub fn kbd<'a>(label: &'a str, p: Palette) -> Element<'a, Message> {
    container(text(label).font(MONO).size(12).color(p.muted))
        .padding([2, 6])
        .style(move |_| container::Style {
            border: Border {
                color: p.field,
                width: 1.0,
                radius: radius(6.0),
            },
            ..Default::default()
        })
        .into()
}

/// Small dark tooltip below the widget.
pub fn tooltip<'a>(
    content: impl Into<Element<'a, Message>>,
    label: &'a str,
    p: Palette,
) -> Element<'a, Message> {
    iced::widget::tooltip(
        content,
        container(text(label).size(12).color(p.on_ink))
            .padding([4, 8])
            .style(chip_box(p.ink, p.on_ink, 6.0)),
        iced::widget::tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

pub fn scroll_style(
    p: Palette,
) -> impl Fn(&Theme, iced::widget::scrollable::Status) -> iced::widget::scrollable::Style {
    use iced::widget::scrollable;
    move |theme, status| {
        let mut s = scrollable::default(theme, status);
        let scroller_color = match status {
            scrollable::Status::Hovered { .. } | scrollable::Status::Dragged { .. } => p.field,
            _ => mix(p.field, p.bg, 0.4),
        };
        for rail in [&mut s.vertical_rail, &mut s.horizontal_rail] {
            rail.background = None;
            rail.border = Border::default();
            rail.scroller.background = Background::Color(scroller_color);
            rail.scroller.border = Border {
                radius: radius(4.0),
                ..Default::default()
            };
        }
        s
    }
}

fn bar<'a>(c: Color, h: f32, width: Length) -> Element<'a, Message> {
    container(iced::widget::space().width(width).height(h))
        .style(chip_box(c, c, 2.0))
        .into()
}

/// One theme card: a miniature of the window (sidebar + selected row) plus the name.
pub fn theme_card<'a>(
    def: &'static crate::theme::ThemeDef,
    on: bool,
    p: Palette,
    h: f32,
) -> Element<'a, Message> {
    use iced::widget::{column, row};
    let c = def.palette;
    let side = container(
        column![
            bar(c.accent, h * 0.12, Length::Fill),
            bar(c.field, h * 0.06, Length::Fill),
            bar(c.field, h * 0.06, Length::Fill),
        ]
        .spacing(h * 0.07),
    )
    .padding([h * 0.1, h * 0.08])
    .width(Length::FillPortion(3))
    .height(Length::Fill)
    .style(panel(c.side));
    let selected_row =
        container(iced::widget::space().width(Length::Fill).height(h * 0.17)).style(move |_| {
            container::Style {
                background: Some(Background::Color(c.soft)),
                border: Border {
                    color: c.accent,
                    width: 0.0,
                    radius: radius(3.0),
                },
                ..Default::default()
            }
        });
    let main = container(
        column![
            iced::widget::stack![selected_row, bar(c.accent, h * 0.17, Length::Fixed(3.0))],
            bar(c.field, h * 0.06, Length::FillPortion(8)),
            bar(c.field, h * 0.06, Length::FillPortion(6)),
        ]
        .spacing(h * 0.08),
    )
    .padding([h * 0.12, h * 0.1])
    .width(Length::FillPortion(7))
    .height(Length::Fill)
    .style(panel(c.bg));
    let mini = container(dir_row(vec![side.into(), main.into()]))
        .height(h)
        .width(Length::Fill)
        .clip(true)
        .style(move |_| container::Style {
            border: Border {
                color: c.line,
                width: 1.0,
                radius: radius(8.0),
            },
            ..Default::default()
        });
    let name = row![
        container(iced::widget::space().width(10).height(10)).style(chip_box(c.accent, c.accent, 5.0)),
        text(t(&format!("theme_{}", def.key)))
            .size(13)
            .font(semibold())
            .color(p.text)
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center);
    button(column![mini, name].spacing(8))
        .width(Length::Fill)
        .padding(8)
        .style(move |_, st| button::Style {
            background: Some(Background::Color(p.bg)),
            text_color: p.text,
            border: Border {
                color: if on {
                    p.accent
                } else if matches!(st, button::Status::Hovered) {
                    p.field
                } else {
                    p.line
                },
                width: 2.0,
                radius: radius(12.0),
            },
            ..Default::default()
        })
        .on_press(Message::SetTheme(def.key))
        .into()
}

/// 3-column grid with all six themes.
pub fn theme_grid<'a>(current: &str, p: Palette, h: f32, gap: f32) -> Element<'a, Message> {
    let themes = crate::theme::themes();
    let mut col = iced::widget::Column::new().spacing(gap);
    for chunk in themes.chunks(3) {
        let cards: Vec<Element<'a, Message>> = chunk
            .iter()
            .map(|d| theme_card(d, d.key == current, p, h))
            .collect();
        col = col.push(dir_row(cards).spacing(gap));
    }
    col.into()
}

/// Markdown rendering settings following the theme (compose preview, HTML mail).
pub fn md_settings(p: Palette, size: f32) -> iced::widget::markdown::Settings {
    let style = iced::widget::markdown::Style {
        font: INTER,
        inline_code_highlight: iced::advanced::text::Highlight {
            background: Background::Color(p.chip),
            border: Border {
                radius: radius(4.0),
                ..Default::default()
            },
        },
        inline_code_padding: iced::Padding::from([0, 3]),
        inline_code_color: p.accent_text,
        inline_code_font: MONO,
        code_block_font: MONO,
        link_color: p.accent_text,
    };
    let mut settings = iced::widget::markdown::Settings::with_text_size(size, style);
    // Mail headings are often oversized: keep them closer to body text.
    settings.h1_size = iced::Pixels(size * 1.5);
    settings.h2_size = iced::Pixels(size * 1.3);
    settings.h3_size = iced::Pixels(size * 1.15);
    settings.h4_size = iced::Pixels(size * 1.05);
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trunc_works() {
        assert_eq!(trunc("ciao", 10), "ciao");
        assert_eq!(trunc("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(500), "500 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(40_000_000), "38,1 MB");
        assert_eq!(extension("Preventivo_v2.pdf"), "PDF");
        assert_eq!(extension("noext"), "FILE");
    }
}
