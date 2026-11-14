//! Which mail servers the relay connects to. Anyone can register a phone,
//! so without these checks an account could point the relay at its own
//! network (other services on the machine, the provider's internal
//! addresses) and learn what answers there.

use anyhow::{Context, Result};
use ferrowl_core::config::Account;
use ferrowl_core::mail;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

/// The account's mail server is not a public address (or port).
#[derive(Debug)]
pub struct NotAllowed;

impl std::fmt::Display for NotAllowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mail server address not allowed")
    }
}

impl std::error::Error for NotAllowed {}

/// Connects to the account's IMAP server, at the addresses just checked
/// (so a name that changes its answer cannot slip a private one in).
pub fn connect(account: &Account) -> Result<mail::ImapSession> {
    let addrs = imap_addrs(account)?;
    mail::connect_to(account, &addrs)
}

/// The account's IMAP server addresses, refused unless all are public.
fn imap_addrs(account: &Account) -> Result<Vec<SocketAddr>> {
    if !port_allowed(account.imap_port) {
        return Err(NotAllowed.into());
    }
    let addrs: Vec<SocketAddr> = (account.imap_host.as_str(), account.imap_port)
        .to_socket_addrs()
        .with_context(|| mail::connect_error(account))?
        .collect();
    // All of them: one private address among public ones still reaches inside.
    if addrs.is_empty() || !addrs.iter().all(|a| is_public(a.ip())) {
        return Err(NotAllowed.into());
    }
    Ok(addrs)
}

/// IMAP's ports (993, 143) and unprivileged ones some servers use; the
/// other privileged ports belong to other services (SSH, web, SMTP...).
fn port_allowed(port: u16) -> bool {
    port == 993 || port == 143 || port >= 1024
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let s = v6.segments();
            // NAT64 (64:ff9b::/96) leads to the IPv4 address inside.
            if s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                return public_v4(Ipv4Addr::from((u32::from(s[6]) << 16) | u32::from(s[7])));
            }
            !(v6.is_multicast()
                // ::, ::1 and the old IPv4-compatible ::a.b.c.d
                || s[..6] == [0; 6]
                // unique local fc00::/7, link-local fe80::/10, site-local fec0::/10
                || s[0] & 0xfe00 == 0xfc00
                || s[0] & 0xffc0 == 0xfe80
                || s[0] & 0xffc0 == 0xfec0
                // discard 100::/64, documentation 2001:db8::/32
                || s[..4] == [0x100, 0, 0, 0]
                || s[..2] == [0x2001, 0xdb8])
        }
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    !(o[0] == 0
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        // shared address space (carrier NAT) 100.64.0.0/10
        || (o[0] == 100 && o[1] & 0xc0 == 64)
        // IETF protocol assignments 192.0.0.0/24, benchmarking 198.18.0.0/15
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)
        || (o[0] == 198 && o[1] & 0xfe == 18)
        // reserved 240.0.0.0/4
        || o[0] >= 240)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public(text: &str) -> bool {
        is_public(text.parse().unwrap())
    }

    #[test]
    fn only_public_addresses() {
        for ok in [
            "8.8.8.8",
            "142.250.180.5",
            "100.128.0.1",
            "2a00:1450:4001:80b::200e",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
        ] {
            assert!(public(ok), "{ok}");
        }
        for no in [
            "127.0.0.1",
            "0.0.0.0",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "::",
            "::1",
            "::127.0.0.1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "fd00::1",
            "fe80::1",
            "ff02::1",
        ] {
            assert!(!public(no), "{no}");
        }
    }

    #[test]
    fn imap_ports_only() {
        assert!(port_allowed(993) && port_allowed(143) && port_allowed(10993));
        assert!(!port_allowed(22) && !port_allowed(80) && !port_allowed(25) && !port_allowed(1));
    }

    #[test]
    fn refuses_local_servers() {
        let account = Account {
            imap_host: "127.0.0.1".into(),
            imap_port: 993,
            ..Default::default()
        };
        let e = imap_addrs(&account).unwrap_err();
        assert!(e.downcast_ref::<NotAllowed>().is_some());
        let account = Account {
            imap_host: "localhost".into(),
            imap_port: 1993,
            ..Default::default()
        };
        assert!(imap_addrs(&account).is_err());
    }
}
