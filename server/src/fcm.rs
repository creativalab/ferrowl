//! Push targets and Firebase Cloud Messaging (HTTP v1 API), which wakes
//! Android phones (iPhones: see `apns`).
//!
//! The message is *data-only* and carries just the account id: no sender,
//! no subject. The app then fetches the mail itself, straight from the mail
//! server.

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Result of a notification.
#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    Sent,
    /// The phone uninstalled the app or the token expired: forget it.
    Unregistered,
}

/// Where a phone receives notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Android, Firebase Cloud Messaging.
    Fcm,
    /// iPhone, Apple Push Notification service (App Store and TestFlight builds).
    Apns,
    /// iPhone, development builds (Apple's sandbox).
    ApnsSandbox,
}

impl Platform {
    pub fn name(self) -> &'static str {
        match self {
            Platform::Fcm => "fcm",
            Platform::Apns => "apns",
            Platform::ApnsSandbox => "apns-sandbox",
        }
    }

    pub fn from_name(name: &str) -> Platform {
        match name {
            "apns" => Platform::Apns,
            "apns-sandbox" => Platform::ApnsSandbox,
            _ => Platform::Fcm,
        }
    }
}

/// A phone's push address: its platform and the token it got there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub platform: Platform,
    pub token: String,
}

/// What a phone is told about an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// New unread mail: the phone shows a notification.
    NewMail,
    /// Unread mail was read (or removed) elsewhere: the phone silently takes
    /// back its notifications and updates the number on the app icon.
    Read,
}

impl Event {
    /// The `type` field of the message.
    pub fn name(self) -> &'static str {
        match self {
            Event::NewMail => "new_mail",
            Event::Read => "read",
        }
    }
}

pub trait Notifier: Send + Sync {
    fn notify(&self, target: &Target, account_id: &str, event: Event) -> Result<Delivery>;
}

/// For development without Firebase/APNs credentials: only logs.
pub struct LogNotifier;

impl Notifier for LogNotifier {
    fn notify(&self, target: &Target, account_id: &str, event: Event) -> Result<Delivery> {
        eprintln!(
            "push (log only, {}): {} for account {account_id}",
            target.platform.name(),
            event.name()
        );
        Ok(Delivery::Sent)
    }
}

/// Sends each phone's notification through its platform; platforms without
/// credentials only log.
pub struct Push {
    pub fcm: Option<Box<dyn Notifier>>,
    pub apns: Option<Box<dyn Notifier>>,
}

impl Notifier for Push {
    fn notify(&self, target: &Target, account_id: &str, event: Event) -> Result<Delivery> {
        let service = match target.platform {
            Platform::Fcm => &self.fcm,
            Platform::Apns | Platform::ApnsSandbox => &self.apns,
        };
        match service {
            Some(n) => n.notify(target, account_id, event),
            None => LogNotifier.notify(target, account_id, event),
        }
    }
}

/// Fields used from a Firebase service account JSON key.
#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: String,
    token_uri: String,
}

pub struct Fcm {
    project_id: String,
    client_email: String,
    key: jsonwebtoken::EncodingKey,
    token_uri: String,
    /// OAuth access token and when it expires.
    access: Mutex<Option<(String, Instant)>>,
}

const SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";

impl Fcm {
    /// From the service account key file downloaded from the Firebase console.
    pub fn from_file(path: &str) -> Result<Fcm> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
        let sa: ServiceAccount = serde_json::from_str(&text).context("not a service account key")?;
        Ok(Fcm {
            key: jsonwebtoken::EncodingKey::from_rsa_pem(sa.private_key.as_bytes())
                .context("service account private key")?,
            project_id: sa.project_id,
            client_email: sa.client_email,
            token_uri: sa.token_uri,
            access: Mutex::new(None),
        })
    }

    /// OAuth access token for FCM, cached until shortly before it expires.
    fn access_token(&self) -> Result<String> {
        let mut cached = self.access.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((token, until)) = cached.as_ref()
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let claims = serde_json::json!({
            "iss": self.client_email,
            "scope": SCOPE,
            "aud": self.token_uri,
            "iat": now,
            "exp": now + 3600,
        });
        let jwt = jsonwebtoken::encode(
            &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
            &claims,
            &self.key,
        )?;
        #[derive(Deserialize)]
        struct TokenReply {
            access_token: String,
            expires_in: u64,
        }
        let reply: TokenReply = ureq::post(&self.token_uri)
            .send_form([
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", jwt.as_str()),
            ])?
            .body_mut()
            .read_json()?;
        let until = Instant::now() + Duration::from_secs(reply.expires_in.saturating_sub(300));
        *cached = Some((reply.access_token.clone(), until));
        Ok(reply.access_token)
    }
}

/// The FCM message: data-only, so the app decides what to show.
fn message(token: &str, account_id: &str, event: Event) -> serde_json::Value {
    // New mail: high priority wakes the app even in Doze. Taking back a
    // notification can wait (and Android lowers apps whose high-priority
    // messages show nothing).
    let priority = match event {
        Event::NewMail => "HIGH",
        Event::Read => "NORMAL",
    };
    serde_json::json!({
        "message": {
            "token": token,
            "data": { "type": event.name(), "account": account_id },
            "android": { "priority": priority, "ttl": "3600s" },
        }
    })
}

impl Notifier for Fcm {
    fn notify(&self, target: &Target, account_id: &str, event: Event) -> Result<Delivery> {
        let fcm_token = target.token.as_str();
        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );
        let body = message(fcm_token, account_id, event);
        let result = ureq::post(&url)
            .header("Authorization", &format!("Bearer {}", self.access_token()?))
            .config()
            .http_status_as_error(false)
            .build()
            .send_json(&body)?;
        match result.status().as_u16() {
            200 => Ok(Delivery::Sent),
            // UNREGISTERED / INVALID_ARGUMENT for a dead token.
            404 => Ok(Delivery::Unregistered),
            code => Err(anyhow!("FCM answered {code}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_carry_only_the_account_and_the_event() {
        let m = message("tok", "acc1", Event::Read);
        assert_eq!(m["message"]["data"]["type"], "read");
        assert_eq!(m["message"]["data"]["account"], "acc1");
        assert_eq!(m["message"]["android"]["priority"], "NORMAL");
        let m = message("tok", "acc1", Event::NewMail);
        assert_eq!(m["message"]["data"]["type"], "new_mail");
        assert_eq!(m["message"]["android"]["priority"], "HIGH");
    }
}
