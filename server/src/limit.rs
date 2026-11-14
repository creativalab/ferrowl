//! Limits against abuse: registrations per client address, and how many
//! phones and watched accounts the server takes in all.
//!
//! The server listens on 127.0.0.1 behind Caddy, so every connection
//! comes from the proxy; the client's address is then the last entry of
//! `X-Forwarded-For`, which Caddy sets (it replaces the header of clients
//! it does not trust, so the last entry is the address it saw). The header
//! counts only on connections from the loopback address: anyone reaching
//! the port directly could write anything in it.

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Registrations (POST /v1/devices) per client address in `window`.
    pub registrations: u32,
    pub window: Duration,
    /// Phones registered in all.
    pub max_devices: usize,
    /// Accounts watched in all (each is an IMAP connection held open).
    pub max_accounts: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            registrations: 20,
            window: Duration::from_secs(3600),
            max_devices: 50_000,
            max_accounts: 20_000,
        }
    }
}

/// The client's address, as far as it can be known (see the module
/// comment); `None` when unknown (tests, odd proxies), which is limited
/// as one shared client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientIp(pub Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        let forwarded = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok());
        Ok(ClientIp(client_ip(peer, forwarded)))
    }
}

fn client_ip(peer: Option<IpAddr>, forwarded: Option<&str>) -> Option<IpAddr> {
    match peer {
        Some(p) if p.is_loopback() => forwarded
            .and_then(|f| f.rsplit(',').next())
            .and_then(|last| last.trim().parse().ok())
            .or(Some(p)),
        other => other,
    }
}

/// Counts requests per client address in fixed windows.
#[derive(Default)]
pub struct RateLimiter {
    seen: Mutex<HashMap<Option<IpAddr>, (Instant, u32)>>,
}

/// Addresses remembered before expired windows are dropped.
const MAX_TRACKED: usize = 100_000;

impl RateLimiter {
    /// Counts one request; false when the address used up its window.
    pub fn allow(&self, ip: ClientIp, max: u32, window: Duration) -> bool {
        self.allow_at(ip, max, window, Instant::now())
    }

    fn allow_at(&self, ip: ClientIp, max: u32, window: Duration, now: Instant) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(|p| p.into_inner());
        if seen.len() >= MAX_TRACKED && !seen.contains_key(&ip.0) {
            seen.retain(|_, (start, _)| now.duration_since(*start) < window);
            if seen.len() >= MAX_TRACKED {
                // Flooded from too many addresses to tell apart: refuse
                // newcomers until windows expire.
                return false;
            }
        }
        let entry = seen.entry(ip.0).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        if entry.1 >= max {
            return false;
        }
        entry.1 += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwarded_address_counts_only_from_the_proxy() {
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        let outside: IpAddr = "203.0.113.9".parse().unwrap();
        assert_eq!(
            client_ip(Some(local), Some("1.2.3.4, 198.51.100.7")),
            Some("198.51.100.7".parse().unwrap())
        );
        assert_eq!(client_ip(Some(local), None), Some(local));
        assert_eq!(client_ip(Some(local), Some("junk")), Some(local));
        // Straight to the port: the header is ignored.
        assert_eq!(client_ip(Some(outside), Some("1.2.3.4")), Some(outside));
        assert_eq!(client_ip(None, Some("1.2.3.4")), None);
    }

    #[test]
    fn limits_per_address_and_window() {
        let l = RateLimiter::default();
        let a = ClientIp(Some("198.51.100.7".parse().unwrap()));
        let b = ClientIp(Some("198.51.100.8".parse().unwrap()));
        let w = Duration::from_secs(60);
        let t = Instant::now();
        assert!(l.allow_at(a, 2, w, t));
        assert!(l.allow_at(a, 2, w, t));
        assert!(!l.allow_at(a, 2, w, t));
        assert!(l.allow_at(b, 2, w, t), "others are not affected");
        assert!(l.allow_at(a, 2, w, t + w), "a new window");
    }
}
