//! Ferrowl push relay.
//!
//! Keeps an IMAP IDLE connection on the inbox of each registered account
//! and wakes the phone through Firebase Cloud Messaging when new mail
//! arrives. It never reads or stores message contents; account settings
//! are stored encrypted with keys that only the phones hold (see `seal`).
//!
//! Configuration (environment):
//! - `FERROWL_PUSH_DB`: SQLite file (default `ferrowl-push.sqlite`)
//! - `FERROWL_PUSH_BIND`: listen address (default `127.0.0.1:8080`, behind
//!   an HTTPS reverse proxy such as Caddy)
//! - `GOOGLE_APPLICATION_CREDENTIALS`: Firebase service account key (JSON),
//!   for Android phones.
//! - `FERROWL_APNS_KEY` (path of the `.p8` key), `FERROWL_APNS_KEY_ID`,
//!   `FERROWL_APNS_TEAM_ID`, `FERROWL_APNS_TOPIC` (bundle id, default
//!   `com.ferrowl.app`): Apple Push Notification service, for iPhones.
//!
//! A platform without credentials only logs its notifications (development).

mod addr;
mod api;
mod apns;
mod fcm;
mod seal;
mod store;
mod watch;

use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    ferrowl_core::i18n::set(Some("en"));

    let db = std::env::var("FERROWL_PUSH_DB").unwrap_or_else(|_| "ferrowl-push.sqlite".into());
    let bind = std::env::var("FERROWL_PUSH_BIND").unwrap_or_else(|_| "127.0.0.1:8080".into());
    let fcm: Option<Box<dyn fcm::Notifier>> = match std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
        Ok(path) => Some(Box::new(fcm::Fcm::from_file(&path)?)),
        Err(_) => {
            eprintln!("GOOGLE_APPLICATION_CREDENTIALS not set: Android notifications are only logged");
            None
        }
    };
    // Empty values (the unit file before the Apple ids are filled in) count as unset.
    let set = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let apns: Option<Box<dyn fcm::Notifier>> = match (
        set("FERROWL_APNS_KEY"),
        set("FERROWL_APNS_KEY_ID"),
        set("FERROWL_APNS_TEAM_ID"),
    ) {
        (Some(key), Some(key_id), Some(team)) => {
            let topic = set("FERROWL_APNS_TOPIC").unwrap_or_else(|| "com.ferrowl.app".into());
            Some(Box::new(apns::Apns::new(&key, &key_id, &team, &topic)?))
        }
        _ => {
            eprintln!("FERROWL_APNS_KEY / _KEY_ID / _TEAM_ID not set: iPhone notifications are only logged");
            None
        }
    };
    let notifier: Arc<dyn fcm::Notifier> = Arc::new(fcm::Push { fcm, apns });

    let state = api::AppState {
        store: Arc::new(store::Store::open(&db)?),
        watchers: Arc::new(watch::Watchers::default()),
        notifier,
        keys: Arc::default(),
        check_login: api::real_login_check,
    };
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!("ferrowl-push listening on {bind}");
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
