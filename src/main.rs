// On Windows, in release builds, no console window behind the app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
#[cfg(target_os = "linux")]
mod desktop_entry;
mod html_view;
mod linkify;
mod preview;
mod print;
mod push;
mod single_instance;
mod theme;
mod ui;

use app::App;
// The engine lives in `ferrowl-core`; these keep `crate::mail` & co. working.
use ferrowl_core::{backend, cache, config, crypto, html, i18n, mail, markdown, providers};

fn main() -> iced::Result {
    // A second launch only raises the window that is already open.
    let _instance = match single_instance::start() {
        single_instance::Role::Primary(guard) => guard,
        single_instance::Role::Secondary => return Ok(()),
    };
    #[cfg(target_os = "linux")]
    desktop_entry::integrate_appimage();

    // lettre uses rustls' default crypto provider: pin it to ring.
    let _ = rustls::crypto::ring::default_provider().install_default();
    i18n::set(Some(&config::Config::load().ui.language));
    install_crash_handler();

    let icon =
        iced::window::icon::from_file_data(include_bytes!("../assets/icons/png/ferrowl-256.png"), None).ok();

    let mut app = iced::application(App::boot, App::update, App::view)
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .default_font(ui::INTER)
        .window(iced::window::Settings {
            size: iced::Size::new(1440.0, 900.0),
            min_size: Some(iced::Size::new(1000.0, 640.0)),
            icon,
            // Closing is handled in App (Message::Quit) so the process exits
            // at once instead of waiting for background IMAP work.
            exit_on_close_request: false,
            #[cfg(target_os = "linux")]
            platform_specific: iced::window::settings::PlatformSpecific {
                // Matches ferrowl.desktop, so desktops show the right icon.
                application_id: "ferrowl".into(),
                ..Default::default()
            },
            ..Default::default()
        });
    for font in ui::FONTS {
        app = app.font(font);
    }
    app.run()
}

/// On panic: writes a log and, if the main thread stopped,
/// shows a window with the support address.
fn install_crash_handler() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let log_path = dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("ferrowl")
            .join("crash.log");
        let report = format!(
            "Ferrowl {} — {}\nthread: {}\n{info}\n\n{}\n",
            env!("CARGO_PKG_VERSION"),
            jiff::Zoned::now().strftime("%Y-%m-%dT%H:%M:%S%.f%:z"),
            std::thread::current().name().unwrap_or("?"),
            std::backtrace::Backtrace::force_capture()
        );
        if let Some(dir) = log_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&log_path, &report);
        default_hook(info);

        // Panics in background threads become a generic error in the UI;
        // only the main thread closes the app.
        if std::thread::current().name() == Some("main") {
            let path = log_path.display().to_string();
            let _ = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title(i18n::t("err_crash_title"))
                .set_description(i18n::tf(
                    "err_crash_body",
                    &[("path", &path), ("email", i18n::SUPPORT_EMAIL)],
                ))
                .set_buttons(rfd::MessageButtons::Ok)
                .show();
        }
    }));
}
