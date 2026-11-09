//! Wakes iPhones through the Apple Push Notification service (HTTP/2 API,
//! token authentication with the team's `.p8` key).
//!
//! As with Firebase the message carries no sender or subject: only the
//! account id, and a "New mail" text the phone translates itself
//! (`loc-key`, from the app's `Localizable.strings`). The app then fetches
//! the mail straight from the mail server.

use crate::fcm::{Delivery, Event, Notifier, Platform, Target};
use anyhow::{Context, Result, anyhow};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Apns {
    team_id: String,
    key_id: String,
    key: jsonwebtoken::EncodingKey,
    /// The app's bundle id.
    topic: String,
    http: reqwest::blocking::Client,
    /// Signed provider token and when to make a new one (Apple accepts it
    /// for an hour and refuses new ones more often than every 20 minutes).
    jwt: Mutex<Option<(String, Instant)>>,
}

impl Apns {
    /// `key_file`: the `.p8` key downloaded from the Apple Developer site
    /// (Certificates, Identifiers & Profiles → Keys), with its Key ID and
    /// the Team ID of the account.
    pub fn new(key_file: &str, key_id: &str, team_id: &str, topic: &str) -> Result<Apns> {
        let pem = std::fs::read(key_file).with_context(|| format!("reading {key_file}"))?;
        Ok(Apns {
            key: jsonwebtoken::EncodingKey::from_ec_pem(&pem).context("APNs key (.p8)")?,
            key_id: key_id.to_string(),
            team_id: team_id.to_string(),
            topic: topic.to_string(),
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .context("HTTP client")?,
            jwt: Mutex::new(None),
        })
    }

    fn provider_token(&self) -> Result<String> {
        let mut cached = self.jwt.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((token, until)) = cached.as_ref()
            && Instant::now() < *until
        {
            return Ok(token.clone());
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();
        let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        let claims = serde_json::json!({ "iss": self.team_id, "iat": now });
        let token = jsonwebtoken::encode(&header, &claims, &self.key)?;
        *cached = Some((token.clone(), Instant::now() + Duration::from_secs(45 * 60)));
        Ok(token)
    }
}

/// The notification: "New mail" in the phone's language, one per account
/// (a newer one replaces it), with the account id for the app.
pub fn payload(account_id: &str) -> serde_json::Value {
    serde_json::json!({
        "aps": {
            "alert": { "loc-key": "notif_new_mail" },
            "sound": "default",
            "thread-id": account_id,
            // Lets a notification extension of the app show sender and
            // subject instead (it reads the mail on the phone).
            "mutable-content": 1,
        },
        "type": "new_mail",
        "account": account_id,
    })
}

/// Mail read elsewhere: a background push, shown nowhere, that wakes the
/// app to take back the notifications of messages no longer unread.
pub fn read_payload(account_id: &str) -> serde_json::Value {
    serde_json::json!({
        "aps": { "content-available": 1 },
        "type": "read",
        "account": account_id,
    })
}

impl Notifier for Apns {
    fn notify(&self, target: &Target, account_id: &str, event: Event) -> Result<Delivery> {
        let host = match target.platform {
            Platform::ApnsSandbox => "api.sandbox.push.apple.com",
            _ => "api.push.apple.com",
        };
        let expires = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
            + 3600;
        let request = self
            .http
            .post(format!("https://{host}/3/device/{}", target.token))
            .header("authorization", format!("bearer {}", self.provider_token()?))
            .header("apns-topic", &self.topic)
            .header("apns-expiration", expires.to_string());
        let reply = match event {
            Event::NewMail => request
                .header("apns-push-type", "alert")
                .header("apns-priority", "10")
                .header("apns-collapse-id", account_id)
                .json(&payload(account_id)),
            // Apple requires priority 5 for background pushes.
            Event::Read => request
                .header("apns-push-type", "background")
                .header("apns-priority", "5")
                .json(&read_payload(account_id)),
        }
        .send()?;
        let status = reply.status().as_u16();
        if status == 200 {
            return Ok(Delivery::Sent);
        }
        let reason = reply
            .json::<serde_json::Value>()
            .ok()
            .and_then(|v| v["reason"].as_str().map(str::to_owned))
            .unwrap_or_default();
        match (status, reason.as_str()) {
            // The app was removed, or the token is from the other environment.
            (410, _) | (400, "BadDeviceToken") | (400, "DeviceTokenNotForTopic") => {
                Ok(Delivery::Unregistered)
            }
            _ => Err(anyhow!("APNs answered {status} {reason}")),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn payload_carries_only_the_account() {
        let p = super::payload("acc1");
        assert_eq!(p["account"], "acc1");
        assert_eq!(p["aps"]["alert"]["loc-key"], "notif_new_mail");
        assert_eq!(p["aps"]["thread-id"], "acc1");
        assert!(p["aps"]["alert"].get("body").is_none(), "no message content");
    }

    #[test]
    fn read_payload_is_silent() {
        let p = super::read_payload("acc1");
        assert_eq!(p["type"], "read");
        assert_eq!(p["account"], "acc1");
        assert_eq!(p["aps"]["content-available"], 1);
        assert!(p["aps"].get("alert").is_none() && p["aps"].get("sound").is_none());
    }
}
