//! IMAP access (reading) and SMTP (sending). All functions are blocking:
//! the UI calls them from `backend`, inside `spawn_blocking`.

use crate::config::Account;
use crate::i18n::{t, tf};
use anyhow::{Context, Result, anyhow, bail};
use mail_parser::{AddressList, MessageParser};
use std::collections::HashMap;
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// TLS over TCP. A newtype so it can implement imap's `SetReadTimeout`
/// (needed by IDLE, which waits with a timeout).
pub struct TlsStream(rustls::StreamOwned<rustls::ClientConnection, TcpStream>);

impl std::io::Read for TlsStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl std::io::Write for TlsStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

/// Read/write timeout of IMAP commands: a dead connection fails instead of
/// hanging the caller forever.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

impl imap::extensions::idle::SetReadTimeout for TlsStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> imap::Result<()> {
        // After IDLE the imap crate clears the timeout (None): the commands
        // that follow (SELECT, SEARCH...) get the usual one back instead.
        self.0
            .sock
            .set_read_timeout(timeout.or(Some(COMMAND_TIMEOUT)))
            .map_err(imap::Error::Io)
    }
}
pub type ImapSession = imap::Session<TlsStream>;

/// How many messages (the most recent) to load per folder.
pub const PAGE_SIZE: u32 = 200;
/// Bytes of text downloaded for the list preview.
const PREVIEW_BYTES: u32 = 2048;

// ---------------------------------------------------------------- folders

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FolderKind {
    Inbox,
    Sent,
    Drafts,
    Archive,
    Spam,
    Trash,
    Other,
}

impl FolderKind {
    pub fn label(self) -> &'static str {
        match self {
            FolderKind::Inbox => t("folder_inbox"),
            FolderKind::Sent => t("folder_sent"),
            FolderKind::Drafts => t("folder_drafts"),
            FolderKind::Archive => t("folder_archive"),
            FolderKind::Spam => t("folder_spam"),
            FolderKind::Trash => t("folder_trash"),
            FolderKind::Other => "",
        }
    }

    /// Order in the sidebar.
    pub const SPECIAL: [FolderKind; 6] = [
        FolderKind::Inbox,
        FolderKind::Sent,
        FolderKind::Drafts,
        FolderKind::Archive,
        FolderKind::Spam,
        FolderKind::Trash,
    ];
}

#[derive(Debug, Clone)]
pub struct Folder {
    /// Raw IMAP name (to use in commands).
    pub name: String,
    /// Path segments, decoded for display.
    pub path: Vec<String>,
    pub delimiter: Option<String>,
    pub kind: FolderKind,
    pub selectable: bool,
}

impl Folder {
    pub fn display(&self) -> String {
        match self.kind {
            FolderKind::Other => self.path.last().cloned().unwrap_or_else(|| self.name.clone()),
            k => k.label().to_string(),
        }
    }
}

/// Decodes the "modified UTF-7" IMAP uses for folder names (RFC 3501 §5.1.3).
pub fn decode_utf7_imap(s: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+,";
    let mut out = String::new();
    let mut chars = s.split('&');
    if let Some(first) = chars.next() {
        out.push_str(first);
    }
    for chunk in chars {
        let Some(end) = chunk.find('-') else {
            out.push('&');
            out.push_str(chunk);
            continue;
        };
        let (encoded, rest) = chunk.split_at(end);
        let rest = &rest[1..];
        if encoded.is_empty() {
            out.push('&');
        } else {
            let mut bits: u32 = 0;
            let mut nbits = 0;
            let mut units = Vec::new();
            for b in encoded.bytes() {
                let Some(v) = ALPHABET.iter().position(|&c| c == b) else {
                    continue;
                };
                bits = (bits << 6) | v as u32;
                nbits += 6;
                if nbits >= 16 {
                    nbits -= 16;
                    units.push(((bits >> nbits) & 0xFFFF) as u16);
                }
            }
            out.push_str(&String::from_utf16_lossy(&units));
        }
        out.push_str(rest);
    }
    out
}

fn classify(name: &str, segments: &[String], attrs: &[imap::types::NameAttribute]) -> FolderKind {
    if name.eq_ignore_ascii_case("INBOX") {
        return FolderKind::Inbox;
    }
    // RFC 6154 (SPECIAL-USE), when the server advertises it.
    for a in attrs {
        if let imap::types::NameAttribute::Custom(c) = a {
            match c.to_ascii_lowercase().as_str() {
                "\\sent" => return FolderKind::Sent,
                "\\drafts" => return FolderKind::Drafts,
                "\\trash" => return FolderKind::Trash,
                "\\junk" => return FolderKind::Spam,
                "\\archive" => return FolderKind::Archive,
                _ => {}
            }
        }
    }
    // Otherwise by name, top level only (or under INBOX).
    let top_level = segments.len() == 1 || (segments.len() == 2 && segments[0].eq_ignore_ascii_case("INBOX"));
    if !top_level {
        return FolderKind::Other;
    }
    match segments.last().map(|s| s.to_lowercase()).as_deref() {
        Some("sent" | "sent items" | "sent messages" | "inviati" | "inviata" | "posta inviata") => {
            FolderKind::Sent
        }
        Some("drafts" | "bozze") => FolderKind::Drafts,
        Some("trash" | "cestino" | "deleted items" | "deleted messages" | "posta eliminata") => {
            FolderKind::Trash
        }
        Some("spam" | "junk" | "junk e-mail" | "posta indesiderata") => FolderKind::Spam,
        Some("archive" | "archivio" | "archives") => FolderKind::Archive,
        _ => FolderKind::Other,
    }
}

// ---------------------------------------------------------------- message types

#[derive(Debug, Clone, Default)]
pub struct Header {
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub preview: String,
    pub timestamp: i64,
    pub seen: bool,
    pub has_attachment: bool,
    /// Start of the body as text (for the search index; empty from the cache).
    pub text: String,
    /// Message-ID without angle brackets (empty if the message has none).
    pub message_id: String,
    /// Ids this message replies to (References, then In-Reply-To), for
    /// grouping into conversations (see `threads`).
    pub parents: Vec<String>,
    /// Sender's bare address (for the address book).
    pub from_address: String,
    /// To and Cc as (name, address), for the address book.
    pub recipients: Vec<(String, String)>,
    /// Starred (the IMAP \\Flagged flag, shown as a star by every client).
    pub flagged: bool,
    /// Size of the whole message in bytes (0 = unknown).
    pub size: u32,
    /// Gmail labels (raw IMAP names, as the folders'; Gmail's own system
    /// labels like \\Inbox left out). Empty on other servers.
    pub labels: Vec<String>,
    /// Gmail inbox tab (Primary elsewhere).
    pub category: Category,
}

/// A slice of a folder's message list, by sequence number.
pub struct HeaderPage {
    pub uidvalidity: Option<u32>,
    /// Messages in the folder.
    pub exists: u32,
    /// Sequence number of the oldest message in this page (1 = none older).
    pub first_seq: u32,
    pub headers: Vec<Header>,
}

#[derive(Debug, Clone)]
pub struct Attachment {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct FullMessage {
    pub uid: u32,
    pub from: String,
    pub from_address: String,
    pub to: String,
    pub cc: String,
    /// Bare addresses of To and Cc (for "Reply all").
    pub to_addrs: Vec<String>,
    pub cc_addrs: Vec<String>,
    pub subject: String,
    pub timestamp: i64,
    pub message_id: Option<String>,
    pub references: Vec<String>,
    pub body: String,
    /// HTML body as sent (only when the message has an HTML part).
    pub html: Option<String>,
    /// Inline parts referenced as `cid:` from the HTML, by Content-ID.
    pub inline: HashMap<String, Vec<u8>>,
    pub attachments: Vec<Attachment>,
    /// DKIM result reported by the server (Authentication-Results header).
    pub dkim_pass: bool,
    /// OpenPGP status, if the message is encrypted or signed.
    pub pgp: Option<PgpStatus>,
    /// iCalendar object of an invitation or a reply to one (the message's
    /// text/calendar part), if any.
    pub calendar: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SigState {
    /// Signed with the key imported for the sender.
    Valid,
    /// Signature does not match the content or the sender's key.
    Invalid,
    /// Signed, but we have no key for the sender.
    UnknownKey,
    /// Signed with a key only known from Autocrypt headers: anyone could
    /// have sent it with this From address.
    Unverified,
    /// Signed with a key other than the one we hold for the sender (the
    /// one offered in the message's Autocrypt header).
    KeyChanged,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PgpStatus {
    pub encrypted: bool,
    pub signature: Option<SigState>,
    /// Decryption failed (reason).
    pub error: Option<String>,
}

type KeyLookup<'a> = &'a dyn Fn(&str) -> Option<pgp::composed::SignedPublicKey>;

/// Keys used to open a message: our secret key (+ passphrase) and lookups
/// of correspondents' public keys.
pub struct PgpKeys<'a> {
    pub own: Option<(pgp::composed::SignedSecretKey, String)>,
    /// Keys imported by hand (trusted).
    pub contact: KeyLookup<'a>,
    /// Keys learned from Autocrypt headers.
    pub autocrypt: KeyLookup<'a>,
    /// Stores a key offered in an Autocrypt header.
    pub learn: &'a dyn Fn(&str, &pgp::composed::SignedPublicKey),
}

impl PgpKeys<'static> {
    /// The keys stored on this device.
    pub fn stored(own: Option<(pgp::composed::SignedSecretKey, String)>) -> Self {
        PgpKeys {
            own,
            contact: &crate::crypto::contact_key,
            autocrypt: &crate::crypto::autocrypt_key,
            learn: &learn_stored,
        }
    }
}

fn learn_stored(from: &str, key: &pgp::composed::SignedPublicKey) {
    crate::crypto::learn_autocrypt(from, key);
}

/// Unwraps PGP/MIME (RFC 3156). Returns the entity to display instead of
/// the raw message (decrypted content, or the signed part without the
/// signature) and the status; `(None, None)` for ordinary mail. `offered`
/// is the key in the message's Autocrypt header, if any.
pub fn unwrap_pgp(
    raw: &[u8],
    from: &str,
    keys: &PgpKeys,
    offered: Option<&pgp::composed::SignedPublicKey>,
) -> (Option<Vec<u8>>, Option<PgpStatus>) {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return (None, None);
    };
    let Some(ct) = msg.content_type() else {
        return (None, None);
    };
    let sub = ct.subtype().unwrap_or_default().to_ascii_lowercase();
    if !ct.ctype().eq_ignore_ascii_case("multipart") {
        return (None, None);
    }
    match sub.as_str() {
        "encrypted" => {
            let mut status = PgpStatus {
                encrypted: true,
                ..Default::default()
            };
            let data = msg.parts().find(|p| {
                p.content_type().is_some_and(|c| {
                    c.ctype().eq_ignore_ascii_case("application")
                        && c.subtype()
                            .is_some_and(|s| s.eq_ignore_ascii_case("octet-stream"))
                })
            });
            let Some(data) = data.map(|p| p.decoded().into_owned()) else {
                status.error = Some(t("pgp_malformed").into());
                return (None, Some(status));
            };
            let Some((key, pass)) = &keys.own else {
                status.error = Some(t("err_pgp_no_own_key").into());
                return (None, Some(status));
            };
            match crate::crypto::decrypt(&data, key, pass) {
                Ok(plain) => {
                    // Signed then encrypted: check the inner signature.
                    let (inner, inner_status) = unwrap_pgp(&plain, from, keys, offered);
                    status.signature = inner_status.and_then(|s| s.signature);
                    (Some(inner.unwrap_or(plain)), Some(status))
                }
                Err(e) => {
                    status.error = Some(format!("{e:#}"));
                    (None, Some(status))
                }
            }
        }
        "signed" => {
            // First child: exactly the signed bytes (mail-parser's raw part
            // excludes the line break before the delimiter, as RFC 3156 wants).
            let mut parts = msg.parts().skip(1);
            let Some(signed) = parts.next().map(|p| p.raw().to_vec()) else {
                return (None, None);
            };
            let signature = msg.parts().find(|p| {
                p.content_type().is_some_and(|c| {
                    c.subtype()
                        .is_some_and(|s| s.eq_ignore_ascii_case("pgp-signature"))
                })
            });
            let Some(signature) = signature.map(|p| p.decoded().into_owned()) else {
                return (None, None);
            };
            let canonical = crlf(&signed);
            let from = from.to_lowercase();
            let signed_by = |key: &pgp::composed::SignedPublicKey| {
                crate::crypto::verify(&canonical, &signature, key)
            };
            let state = match ((keys.contact)(&from), (keys.autocrypt)(&from)) {
                (Some(key), _) if signed_by(&key) => SigState::Valid,
                (None, Some(key)) if signed_by(&key) => SigState::Unverified,
                // First message from this sender: its own header's key
                // is as good as any Autocrypt key, no better.
                (None, None) if offered.is_some_and(signed_by) => SigState::Unverified,
                (None, None) => SigState::UnknownKey,
                (Some(_), Some(key)) if signed_by(&key) => SigState::KeyChanged,
                _ if offered.is_some_and(signed_by) => SigState::KeyChanged,
                _ => SigState::Invalid,
            };
            (
                Some(signed),
                Some(PgpStatus {
                    signature: Some(state),
                    ..Default::default()
                }),
            )
        }
        _ => (None, None),
    }
}

/// Normalizes line endings to CRLF (signatures are over canonical text).
fn crlf(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 40);
    let mut prev = 0u8;
    for &b in data {
        if b == b'\n' && prev != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        prev = b;
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct Outgoing {
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    pub body: String,
    /// HTML alternative of `body` (sent as multipart/alternative).
    pub html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub attachments: Vec<PathBuf>,
    /// OpenPGP: detached signature (PGP/MIME).
    pub sign: bool,
    /// OpenPGP: encrypt to all recipients (and to ourselves, for Sent).
    pub encrypt: bool,
    /// iCalendar object sent as a text/calendar alternative of the body
    /// (invitation replies, RFC 6047).
    pub calendar: Option<String>,
}

// ---------------------------------------------------------------- connection

pub(crate) fn tls_config() -> Arc<rustls::ClientConfig> {
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config =
        rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("supported TLS versions")
            .with_root_certificates(roots)
            .with_no_client_auth();
    Arc::new(config)
}

/// The server refused the login (wrong password, IMAP turned off...): the
/// message is the server's answer, already translated.
#[derive(Debug)]
pub struct LoginError(pub String);

impl std::fmt::Display for LoginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for LoginError {}

pub fn connect(account: &Account) -> Result<ImapSession> {
    use std::net::ToSocketAddrs;
    let addrs: Vec<SocketAddr> = (account.imap_host.as_str(), account.imap_port)
        .to_socket_addrs()
        .with_context(|| connect_error(account))?
        .collect();
    connect_to(account, &addrs)
}

/// "Cannot connect to host:port".
pub fn connect_error(account: &Account) -> String {
    tf(
        "err_connect",
        &[
            ("host", &account.imap_host),
            ("port", &account.imap_port.to_string()),
        ],
    )
}

/// Like `connect`, to addresses already resolved (the push server checks
/// them first). The host name is still the one TLS checks.
pub fn connect_to(account: &Account, addrs: &[SocketAddr]) -> Result<ImapSession> {
    let tcp = TcpStream::connect(addrs).with_context(|| connect_error(account))?;
    tcp.set_read_timeout(Some(COMMAND_TIMEOUT))?;
    tcp.set_write_timeout(Some(COMMAND_TIMEOUT))?;

    let server_name = rustls::pki_types::ServerName::try_from(account.imap_host.clone())
        .context("invalid IMAP host name")?;
    let conn = rustls::ClientConnection::new(tls_config(), server_name)?;
    let tls = TlsStream(rustls::StreamOwned::new(conn, tcp));

    let mut client = imap::Client::new(tls);
    client.read_greeting().context("IMAP greeting")?;
    let login_error = |e: imap::Error| {
        let msg = e.to_string();
        let text = if mailbox_refuses_apps(&msg) {
            t("err_outlook_imap_off").to_string()
        } else if account.oauth.is_none()
            && is_gmail_host(&account.imap_host)
            && crate::errors::kind(&msg) == crate::errors::Kind::WrongPassword
        {
            t("err_h_gmail_password").to_string()
        } else if msg.to_uppercase().contains("AUTHENTICATIONFAILED") {
            tf("err_auth", &[("msg", &msg)])
        } else {
            tf("err_login", &[("msg", &msg)])
        };
        anyhow::Error::new(LoginError(text))
    };
    match account.oauth {
        // Signed in with Google/Microsoft: XOAUTH2 with a fresh access token.
        Some(provider) => {
            let token = crate::oauth::access_token(provider, &account.email)?;
            let auth = crate::oauth::ImapXoauth2 {
                response: crate::oauth::xoauth2(&account.username, &token),
            };
            client
                .authenticate("XOAUTH2", &auth)
                .map_err(|(e, _)| login_error(e))
        }
        None => client
            .login(&account.username, &account.password)
            .map_err(|(e, _)| login_error(e)),
    }
}

/// Whether the server supports IMAP IDLE (push).
pub fn supports_idle(session: &mut ImapSession) -> Result<bool> {
    Ok(session.capabilities()?.has_str("IDLE"))
}

/// Waits (IDLE) until `folder` changes or `timeout` passes.
/// Returns `true` if the server reported a change.
pub fn idle_wait(session: &mut ImapSession, folder: &str, timeout: Duration) -> Result<bool> {
    session.select(folder)?;
    idle_wait_selected(session, timeout)
}

/// Like `idle_wait`, on the folder already selected. Selecting it again
/// would tell the server the client has seen every change so far, so mail
/// arrived since the caller last looked would not be reported by IDLE.
pub fn idle_wait_selected(session: &mut ImapSession, timeout: Duration) -> Result<bool> {
    let outcome = session.idle()?.wait_with_timeout(timeout)?;
    Ok(matches!(
        outcome,
        imap::extensions::idle::WaitOutcome::MailboxChanged
    ))
}

pub fn list_folders(session: &mut ImapSession) -> Result<Vec<Folder>> {
    let names = session.list(Some(""), Some("*"))?;
    let has_attr = |n: &imap::types::Name, attr: &str| {
        n.attributes()
            .iter()
            .any(|a| matches!(a, imap::types::NameAttribute::Custom(c) if c.eq_ignore_ascii_case(attr)))
    };
    // No \Archive folder but an \All one (Gmail's "All Mail"): archiving
    // there is how Gmail archives (the message only loses its Inbox label),
    // so it is the Archive, ahead of any folder merely named "Archive".
    let all_mail = (!names.iter().any(|n| has_attr(n, "\\Archive")))
        .then(|| {
            names
                .iter()
                .find(|n| has_attr(n, "\\All"))
                .map(|n| n.name().to_string())
        })
        .flatten();
    let mut folders: Vec<Folder> = names
        .iter()
        .map(|n| {
            let delimiter = n.delimiter().map(str::to_string);
            let decoded = decode_utf7_imap(n.name());
            let path: Vec<String> = match &delimiter {
                Some(d) if !d.is_empty() => decoded.split(d.as_str()).map(str::to_string).collect(),
                _ => vec![decoded],
            };
            let kind = match classify(n.name(), &path, n.attributes()) {
                _ if all_mail.as_deref() == Some(n.name()) => FolderKind::Archive,
                FolderKind::Archive if all_mail.is_some() => FolderKind::Other,
                kind => kind,
            };
            Folder {
                name: n.name().to_string(),
                selectable: !n.attributes().contains(&imap::types::NameAttribute::NoSelect),
                path,
                delimiter,
                kind,
            }
        })
        .collect();
    // One folder per special kind: the others go back to "normal".
    let mut seen = std::collections::HashSet::new();
    for f in folders.iter_mut() {
        if f.kind != FolderKind::Other && !seen.insert(f.kind) {
            f.kind = FolderKind::Other;
        }
    }
    folders.sort_by_key(|f| f.path.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>());
    Ok(folders)
}

/// Unread messages per folder, and whether the session is still in step
/// with the server afterwards (false: start the next command on a new
/// connection).
///
/// The STATUS answer is read here rather than by the IMAP library, whose
/// parser refuses Outlook's ("Unable to parse status response").
pub fn unseen_counts(session: &mut ImapSession, folders: &[String]) -> (HashMap<String, u32>, bool) {
    let mut out = HashMap::new();
    for f in folders {
        match session.run_command_and_read_response(format!("STATUS {} (UNSEEN)", quote_mailbox(f))) {
            Ok(answer) => {
                if let Some(n) = unseen_in(&answer) {
                    out.insert(f.clone(), n);
                }
            }
            // Refused for this folder: the session is still in step.
            Err(imap::Error::No(_) | imap::Error::Bad(_)) => continue,
            Err(e) => {
                if cfg!(debug_assertions) {
                    eprintln!("imap: STATUS {f:?} failed, stopping the counts: {e}");
                }
                return (out, false);
            }
        }
    }
    (out, true)
}

/// A mailbox name as an IMAP quoted string.
fn quote_mailbox(name: &str) -> String {
    format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The UNSEEN count in a STATUS answer ("* STATUS INBOX (UNSEEN 3)").
fn unseen_in(answer: &[u8]) -> Option<u32> {
    let text = String::from_utf8_lossy(answer).to_ascii_uppercase();
    let at = text.find("UNSEEN ")? + "UNSEEN ".len();
    text[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

// ---------------------------------------------------------------- reading

fn first_addr(addr: Option<AddressList>) -> (String, String) {
    let Some(first) = addr.and_then(|a| a.first()) else {
        return (String::new(), String::new());
    };
    let email = first.address().unwrap_or_default().to_string();
    let shown = match first.name() {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => email.clone(),
    };
    (shown, email)
}

fn addr_list(addr: Option<AddressList>) -> String {
    let Some(addr) = addr else {
        return String::new();
    };
    addr.mailboxes()
        .map(|a| match (a.name(), a.address()) {
            (Some(n), Some(e)) if !n.is_empty() => format!("{n} <{e}>"),
            (_, Some(e)) => e.to_string(),
            (Some(n), None) => n.to_string(),
            _ => String::new(),
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn bare_list(addr: Option<AddressList>) -> Vec<String> {
    addr.map(|a| {
        a.mailboxes()
            .filter_map(|m| m.address().map(str::to_string))
            .collect()
    })
    .unwrap_or_default()
}

fn one_line(s: &str, max: usize) -> String {
    let mut out = String::new();
    for word in s.split_whitespace() {
        if word.starts_with('>') {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
        if out.chars().count() >= max {
            break;
        }
    }
    out.chars().take(max).collect()
}

/// Selects the folder and downloads headers and previews of the most recent messages.
/// The most recent `PAGE_SIZE` messages of a folder.
pub fn fetch_headers(session: &mut ImapSession, folder: &str) -> Result<HeaderPage> {
    let mailbox = session.select(folder)?;
    let last = mailbox.exists;
    fetch_range(session, mailbox, last.saturating_sub(PAGE_SIZE - 1).max(1), last)
}

/// What changed in a folder since it was cached: messages newer than the
/// highest cached UID, and the read/starred state of the cached ones.
pub struct Delta {
    pub uidvalidity: Option<u32>,
    pub exists: u32,
    /// Messages with a UID above `known_max`, newest first.
    pub new: Vec<Header>,
    /// (uid, seen, flagged) of the messages from `window_min` to `known_max`
    /// still in the folder; the cached ones missing here were deleted.
    pub flags: Vec<(u32, bool, bool)>,
}

/// A cheap refresh: only messages above `known_max` are downloaded; for the
/// cached window (`window_min..=known_max`) just UIDs and flags. Gives up
/// (`None`) when there are more new messages than a page, so the caller can
/// fetch the page instead.
pub fn fetch_delta(
    session: &mut ImapSession,
    folder: &str,
    known_max: u32,
    window_min: u32,
) -> Result<Option<Delta>> {
    let mailbox = session.select(folder)?;
    let mut delta = Delta {
        uidvalidity: mailbox.uid_validity,
        exists: mailbox.exists,
        new: Vec::new(),
        flags: Vec::new(),
    };
    if mailbox.exists == 0 {
        return Ok(Some(delta));
    }
    let has_new = mailbox.uid_next.is_none_or(|next| next > known_max + 1);
    if has_new {
        let query = format!(
            "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER] BODY.PEEK[TEXT]<0.{PREVIEW_BYTES}>)"
        );
        // "n:*" always returns at least the last message, even when older.
        let fetches = session.uid_fetch(format!("{}:*", known_max + 1), query)?;
        let fresh: Vec<_> = fetches
            .iter()
            .filter(|f| f.uid.is_some_and(|u| u > known_max))
            .collect();
        if fresh.len() as u32 >= PAGE_SIZE {
            return Ok(None);
        }
        for f in fresh {
            let uid = f.uid.unwrap_or_default();
            let mut raw = f.header().unwrap_or_default().to_vec();
            raw.extend_from_slice(f.text().unwrap_or_default());
            let mut h = header_from_raw(uid, &raw, f.flags().contains(&imap::types::Flag::Seen));
            h.flagged = f.flags().contains(&imap::types::Flag::Flagged);
            h.size = f.size.unwrap_or(0);
            fill_missing_date(&mut h.timestamp, f);
            delta.new.push(h);
        }
        delta
            .new
            .sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(b.uid.cmp(&a.uid)));
    }
    if window_min > 0 && window_min <= known_max {
        for f in session
            .uid_fetch(format!("{window_min}:{known_max}"), "(UID FLAGS)")?
            .iter()
        {
            if let Some(uid) = f.uid
                && (window_min..=known_max).contains(&uid)
            {
                let flags = f.flags();
                delta.flags.push((
                    uid,
                    flags.contains(&imap::types::Flag::Seen),
                    flags.contains(&imap::types::Flag::Flagged),
                ));
            }
        }
    }
    Ok(Some(delta))
}

/// Every UID of a folder (UIDs only: a few bytes per message), to find
/// cached messages deleted by other clients below the refreshed window.
pub fn all_uids(session: &mut ImapSession, folder: &str) -> Result<Vec<u32>> {
    session.select(folder)?;
    Ok(session.uid_search("ALL")?.into_iter().collect())
}

/// What a new-mail notification needs from the inbox. See [`peek_inbox`].
#[derive(Debug, Clone, Default)]
pub struct Peek {
    /// The newest unread messages with a UID above `after`, newest first
    /// (at most `limit`; sender and subject, no preview).
    pub fresh: Vec<Header>,
    /// How many unread messages have a UID above `after` in all.
    pub fresh_count: u32,
    /// All the unread messages' UIDs above `after`, newest first.
    pub unread_uids: Vec<u32>,
    /// The newest message of the inbox, read or not (`None`: empty inbox).
    pub newest: Option<Header>,
    /// The inbox's unread count; for Gmail, of the Primary tab among the
    /// newest `PAGE_SIZE` messages (as the app's icon number counts them).
    pub unread: u32,
}

/// Reads only what a new-mail notification shows, for the phone's
/// notification handlers, which have little time and memory: no folder
/// list, no cache, no previews, a few small commands. The inbox is opened
/// read-only (EXAMINE), so nothing gets marked as read.
pub fn peek_inbox(session: &mut ImapSession, after: u32, limit: u32) -> Result<Peek> {
    let gmail = has_gmail_labels(session);
    let mailbox = session.examine("INBOX")?;
    let mut peek = Peek::default();
    if mailbox.exists == 0 {
        return Ok(peek);
    }
    // UIDs and flags of the newest page: the newest message, and for Gmail
    // the window its unread count is taken from.
    let first = mailbox.exists.saturating_sub(PAGE_SIZE - 1).max(1);
    let window: Vec<(u32, bool)> = session
        .fetch(format!("{first}:{}", mailbox.exists), "(UID FLAGS)")?
        .iter()
        .filter_map(|f| Some((f.uid?, f.flags().contains(&imap::types::Flag::Seen))))
        .collect();
    let newest_uid = window.iter().map(|(u, _)| *u).max();

    let unseen: Vec<u32> = session.uid_search("UNSEEN")?.into_iter().collect();
    peek.unread = if gmail {
        let unread_window: Vec<u32> = window.iter().filter(|(_, seen)| !seen).map(|(u, _)| *u).collect();
        gmail_search(
            session,
            &unread_window,
            &format!("X-GM-RAW {}", imap_quote("category:primary")),
        )
        .map(|s| s.len() as u32)
        .unwrap_or(unread_window.len() as u32)
    } else {
        unseen.len() as u32
    };

    let mut fresh: Vec<u32> = unseen.into_iter().filter(|u| *u > after).collect();
    fresh.sort_unstable_by(|a, b| b.cmp(a));
    peek.fresh_count = fresh.len() as u32;
    peek.unread_uids = fresh.clone();
    fresh.truncate(limit as usize);

    let mut wanted = fresh.clone();
    if let Some(n) = newest_uid
        && !wanted.contains(&n)
    {
        wanted.push(n);
    }
    if wanted.is_empty() {
        return Ok(peek);
    }
    let mut headers: HashMap<u32, Header> = session
        .uid_fetch(uid_set(&wanted), "(UID FLAGS INTERNALDATE BODY.PEEK[HEADER])")?
        .iter()
        .filter_map(|f| {
            let uid = f.uid?;
            let mut h = header_from_raw(
                uid,
                f.header().unwrap_or_default(),
                f.flags().contains(&imap::types::Flag::Seen),
            );
            fill_missing_date(&mut h.timestamp, f);
            Some((uid, h))
        })
        .collect();
    peek.newest = newest_uid.and_then(|n| headers.get(&n).cloned());
    peek.fresh = fresh.iter().filter_map(|u| headers.remove(u)).collect();
    Ok(peek)
}

/// The unread messages with a UID of at least `from` in the selected
/// folder (the server's push watcher: whether new mail is worth a
/// notification).
pub fn unseen_from(session: &mut ImapSession, from: u32) -> Result<Vec<u32>> {
    // "n:*" always matches the last message, even below n: filtered out.
    Ok(session
        .uid_search(format!("UID {}:* UNSEEN", from.max(1)))?
        .into_iter()
        .filter(|u| *u >= from)
        .collect())
}

/// Up to `count` messages older than sequence number `before` (for indexing).
pub fn fetch_older(session: &mut ImapSession, folder: &str, before: u32, count: u32) -> Result<HeaderPage> {
    let mailbox = session.select(folder)?;
    let last = before.saturating_sub(1).min(mailbox.exists);
    fetch_range(session, mailbox, last.saturating_sub(count - 1).max(1), last)
}

fn fetch_range(
    session: &mut ImapSession,
    mailbox: imap::types::Mailbox,
    first: u32,
    last: u32,
) -> Result<HeaderPage> {
    let mut page = HeaderPage {
        uidvalidity: mailbox.uid_validity,
        exists: mailbox.exists,
        first_seq: first,
        headers: Vec::new(),
    };
    if mailbox.exists == 0 || last == 0 {
        page.first_seq = 1;
        return Ok(page);
    }
    let range = format!("{first}:{last}");
    let query =
        format!("(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER] BODY.PEEK[TEXT]<0.{PREVIEW_BYTES}>)");
    let fetches = session.fetch(range, query)?;

    let mut headers: Vec<Header> = fetches
        .iter()
        .filter_map(|f| {
            let uid = f.uid?;
            // Headers + start of the body: enough to build the preview.
            let mut raw = f.header().unwrap_or_default().to_vec();
            raw.extend_from_slice(f.text().unwrap_or_default());
            let mut h = header_from_raw(uid, &raw, f.flags().contains(&imap::types::Flag::Seen));
            h.flagged = f.flags().contains(&imap::types::Flag::Flagged);
            h.size = f.size.unwrap_or(0);
            fill_missing_date(&mut h.timestamp, f);
            Some(h)
        })
        .collect();
    headers.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then(b.uid.cmp(&a.uid)));
    page.headers = headers;
    Ok(page)
}

/// List entry (sender, subject, preview, attachments) from a raw message,
/// or from its headers plus the start of the body.
pub fn header_from_raw(uid: u32, raw: &[u8], seen: bool) -> Header {
    let parsed = MessageParser::default().parse(raw);
    let msg = parsed.as_ref();
    let (from, from_address) = first_addr(msg.and_then(|m| m.from()));
    let mut recipients = Vec::new();
    for list in [msg.and_then(|m| m.to()), msg.and_then(|m| m.cc())]
        .into_iter()
        .flatten()
    {
        for a in list.mailboxes() {
            if let Some(addr) = a.address() {
                recipients.push((a.name().unwrap_or_default().to_string(), addr.to_string()));
            }
        }
    }
    let content_type = msg
        .and_then(|m| m.content_type())
        .map(|ct| format!("{}/{}", ct.ctype(), ct.subtype().unwrap_or_default()))
        .unwrap_or_default()
        .to_lowercase();
    let has_attachment =
        content_type == "multipart/mixed" || msg.map(|m| m.attachments().len() > 0).unwrap_or(false);
    let text = msg
        .and_then(|m| m.body_text(0))
        .map(|t| t.into_owned())
        .unwrap_or_default();
    Header {
        uid,
        from,
        subject: msg
            .and_then(|m| m.subject())
            .unwrap_or(t("no_subject"))
            .to_string(),
        preview: one_line(&text, 140),
        timestamp: msg.and_then(|m| m.date()).map(|d| d.to_timestamp()).unwrap_or(0),
        seen,
        has_attachment,
        text: text.chars().take(4000).collect(),
        message_id: msg.and_then(|m| m.message_id()).map(bare_id).unwrap_or_default(),
        parents: thread_parents(raw),
        from_address,
        recipients,
        flagged: false,
        // The whole message (POP3, local mailbox); IMAP lists set the
        // server's size instead.
        size: u32::try_from(raw.len()).unwrap_or(u32::MAX),
        labels: Vec::new(),
        category: Category::Primary,
    }
}

/// The addresses in a To/Cc field, trimmed, empty ones left out. Commas
/// and semicolons separate them only outside quotes and angle brackets:
/// "Rossi, Mario" <m@x.it> is one address.
pub fn split_addresses(field: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut quoted, mut angle, mut escaped) = (0, false, false, false);
    for (i, c) in field.char_indices() {
        match c {
            _ if escaped => escaped = false,
            '\\' if quoted => escaped = true,
            '"' if !angle => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                parts.push(&field[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&field[start..]);
    parts
        .into_iter()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

/// Addresses typed in a To/Cc field ("Ada <ada@x.org>, bob@y.org") as
/// (name, address) pairs.
pub fn parse_recipients(field: &str) -> Vec<(String, String)> {
    split_addresses(field)
        .into_iter()
        .filter(|s| s.contains('@'))
        .map(|s| match (s.rfind('<'), s.rfind('>')) {
            (Some(a), Some(b)) if a < b => (
                s[..a].trim().trim_matches('"').replace("\\\"", "\""),
                s[a + 1..b].trim().to_string(),
            ),
            _ => (String::new(), s.to_string()),
        })
        .collect()
}

/// A message id without spaces and angle brackets.
pub fn bare_id(id: &str) -> String {
    id.trim().trim_matches(['<', '>']).to_string()
}

/// References and In-Reply-To ids, in order, without duplicates.
fn thread_parents(raw: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in ["references", "in-reply-to"] {
        for value in raw_header_values(raw, name) {
            for id in value.split_whitespace().map(bare_id).filter(|s| !s.is_empty()) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }
    out
}

/// Downloads the full message (marks it as read on the server).
pub fn fetch_message(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
    keys: &PgpKeys,
) -> Result<FullMessage> {
    session.select(folder)?;
    fetch_full(session, uid, keys, "(INTERNALDATE BODY[])")
}

/// Downloads the full message without marking it read (for other apps
/// reading it on the user's behalf: nothing changes on the server).
pub fn peek_message(
    session: &mut ImapSession,
    folder: &str,
    uid: u32,
    keys: &PgpKeys,
) -> Result<FullMessage> {
    session.examine(folder)?;
    fetch_full(session, uid, keys, "(INTERNALDATE BODY.PEEK[])")
}

fn fetch_full(session: &mut ImapSession, uid: u32, keys: &PgpKeys, items: &str) -> Result<FullMessage> {
    let fetches = session.uid_fetch(uid.to_string(), items)?;
    let f = fetches
        .iter()
        .find(|f| f.body().is_some())
        .context(t("err_not_found"))?;
    let mut full = parse_full(uid, f.body().unwrap_or_default(), keys)?;
    fill_missing_date(&mut full.timestamp, f);
    Ok(full)
}

/// Messages without a usable Date header (some scripts and alert senders
/// omit it) take the server's arrival time instead of sorting as 1970.
fn fill_missing_date(timestamp: &mut i64, f: &imap::types::Fetch) {
    if *timestamp == 0
        && let Some(d) = f.internal_date()
    {
        *timestamp = d.timestamp();
    }
}

/// Builds the reader's view of a raw message (OpenPGP unwrapped).
pub fn parse_full(uid: u32, raw: &[u8], keys: &PgpKeys) -> Result<FullMessage> {
    let outer = MessageParser::default().parse(raw).context(t("err_unreadable"))?;
    let (from, from_address) = first_addr(outer.from());

    // Autocrypt: the key the sender offers. Signatures are checked against
    // the keys held before this message; the offered key is stored only
    // afterwards, and only if we hold none for the sender.
    let offered = raw_header_values(raw, "autocrypt")
        .iter()
        .find_map(|value| crate::crypto::autocrypt_offer(&from_address, value));
    let (inner, mut pgp) = unwrap_pgp(raw, &from_address, keys, offered.as_ref());
    if let Some(key) = &offered {
        (keys.learn)(&from_address, key);
    }
    let inner_msg = inner
        .as_deref()
        .and_then(|bytes| MessageParser::default().parse(bytes));
    // Body, HTML and attachments come from the unwrapped content if any;
    // headers always from the outer message.
    let msg = inner_msg.as_ref().unwrap_or(&outer);

    let mut body = msg
        .body_text(0)
        .map(|b| b.into_owned())
        .unwrap_or_else(|| t("no_text").into());
    if let Some(status) = pgp.as_ref().and_then(|p| p.error.clone()) {
        body = format!("{}\n\n{status}", t("pgp_cannot_decrypt"));
    }
    // Inline PGP (armored block in a plain-text body).
    if pgp.is_none()
        && body.contains("-----BEGIN PGP MESSAGE-----")
        && let Some((key, pass)) = &keys.own
    {
        let mut status = PgpStatus {
            encrypted: true,
            ..Default::default()
        };
        match crate::crypto::decrypt(body.as_bytes(), key, pass) {
            Ok(plain) => body = String::from_utf8_lossy(&plain).into_owned(),
            Err(e) => status.error = Some(format!("{e:#}")),
        }
        pgp = Some(status);
    }
    let html: Vec<String> = msg
        .html_body()
        .filter(|part| matches!(part.kind(), mail_parser::PartKind::Html))
        .filter_map(|part| part.text().map(|t| t.into_owned()))
        .collect();
    let html = (!html.is_empty()).then(|| html.join("\n"));
    let inline: HashMap<String, Vec<u8>> = msg
        .parts()
        .filter_map(|part| {
            let id = part.content_id()?.trim().trim_matches(['<', '>']).to_string();
            (!id.is_empty()).then(|| (id, part.decoded().into_owned()))
        })
        .collect();
    let attachments = msg
        .attachments()
        // An unnamed calendar part is the invitation itself, shown as a
        // card: not listed as "attachment-1".
        .filter(|part| {
            part.attachment_name().is_some()
                || !part.content_type().is_some_and(|ct| {
                    ct.ctype().eq_ignore_ascii_case("text")
                        && ct.subtype().is_some_and(|s| s.eq_ignore_ascii_case("calendar"))
                })
        })
        .enumerate()
        .map(|(i, part)| Attachment {
            name: part
                .attachment_name()
                .map(str::to_string)
                .unwrap_or_else(|| format!("attachment-{}", i + 1)),
            data: part.decoded().into_owned(),
        })
        .collect();
    // Invitations: the first text/calendar part (inline or attached).
    let calendar = msg
        .parts()
        .find(|part| {
            part.content_type().is_some_and(|ct| {
                ct.ctype().eq_ignore_ascii_case("text")
                    && ct.subtype().is_some_and(|s| s.eq_ignore_ascii_case("calendar"))
            })
        })
        .map(|part| String::from_utf8_lossy(&part.decoded()).into_owned());
    let dkim_pass = raw_header_values(raw, "authentication-results")
        .iter()
        .any(|v| v.to_lowercase().contains("dkim=pass"));
    let references = raw_header_values(raw, "references")
        .iter()
        .flat_map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .collect();

    Ok(FullMessage {
        uid,
        from,
        from_address,
        to: addr_list(outer.to()),
        cc: addr_list(outer.cc()),
        to_addrs: bare_list(outer.to()),
        cc_addrs: bare_list(outer.cc()),
        subject: outer.subject().unwrap_or(t("no_subject")).to_string(),
        timestamp: outer.date().map(|d| d.to_timestamp()).unwrap_or(0),
        message_id: outer.message_id().map(str::to_string),
        references,
        body,
        html,
        inline,
        attachments,
        dkim_pass,
        pgp,
        calendar,
    })
}

/// Raw values of a header (continuation lines joined).
fn raw_header_values(raw: &[u8], name: &str) -> Vec<String> {
    let text = String::from_utf8_lossy(raw);
    let head = text.split("\r\n\r\n").next().unwrap_or_default();
    let head = head.split("\n\n").next().unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    for line in head.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(c) = current.as_mut() {
                c.push(' ');
                c.push_str(line.trim());
            }
            continue;
        }
        if let Some(c) = current.take() {
            out.push(c);
        }
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case(name)
        {
            current = Some(v.trim().to_string());
        }
    }
    if let Some(c) = current {
        out.push(c);
    }
    out
}

// ---------------------------------------------------------------- changes

/// Moves a message to another folder (MOVE if available, otherwise COPY + delete).
/// IMAP UID set for one command: "12,15,40".
fn uid_set(uids: &[u32]) -> String {
    uids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
}

/// Moves messages to another folder (MOVE, or COPY + delete + EXPUNGE).
pub fn move_message(session: &mut ImapSession, from: &str, uids: &[u32], to: &str) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    session.select(from)?;
    let set = uid_set(uids);
    let has_move = session.capabilities().map(|c| c.has_str("MOVE")).unwrap_or(false);
    if has_move {
        session.uid_mv(&set, to)?;
    } else {
        session.uid_copy(&set, to)?;
        session.uid_store(&set, "+FLAGS (\\Deleted)")?;
        expunge_uids(session, &set)?;
    }
    Ok(())
}

/// Removes the given messages (already flagged \Deleted). With UIDPLUS only
/// those; without it EXPUNGE is the only way, and it also removes messages
/// other clients flagged \Deleted in the folder.
fn expunge_uids(session: &mut ImapSession, set: &str) -> Result<()> {
    let uidplus = session.capabilities().is_ok_and(|c| c.has_str("UIDPLUS"));
    if uidplus {
        session.uid_expunge(set)?;
    } else {
        session.expunge()?;
    }
    Ok(())
}

/// Deletes permanently (used only inside Trash).
pub fn delete_permanently(session: &mut ImapSession, folder: &str, uids: &[u32]) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    session.select(folder)?;
    let set = uid_set(uids);
    session.uid_store(&set, "+FLAGS (\\Deleted)")?;
    expunge_uids(session, &set)?;
    Ok(())
}

pub fn set_seen(session: &mut ImapSession, folder: &str, uids: &[u32], seen: bool) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    session.select(folder)?;
    let op = if seen {
        "+FLAGS (\\Seen)"
    } else {
        "-FLAGS (\\Seen)"
    };
    session.uid_store(uid_set(uids), op)?;
    Ok(())
}

/// Adds and removes keywords such as `$Junk` / `$NotJunk`, which servers
/// and other clients use to learn what is spam. Best effort: servers that
/// do not accept new keywords refuse it, and that is fine.
pub fn set_keywords(session: &mut ImapSession, folder: &str, uids: &[u32], add: &[&str], remove: &[&str]) {
    if uids.is_empty() || session.select(folder).is_err() {
        return;
    }
    if !add.is_empty() {
        let _ = session.uid_store(uid_set(uids), format!("+FLAGS ({})", add.join(" ")));
    }
    if !remove.is_empty() {
        let _ = session.uid_store(uid_set(uids), format!("-FLAGS ({})", remove.join(" ")));
    }
}

/// Whether the server is Gmail (labels through the X-GM-EXT-1 extension).
pub fn has_gmail_labels(session: &mut ImapSession) -> bool {
    session
        .capabilities()
        .map(|c| c.has_str("X-GM-EXT-1"))
        .unwrap_or(false)
}

/// Quotes a string for an IMAP command.
fn imap_quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The UIDs among `uids` that a Gmail search matches (e.g. `X-GM-LABELS
/// "Work"` or `X-GM-RAW "category:social"`). Searches are used because the
/// IMAP library cannot read Gmail's own FETCH items; SEARCH replies it can.
fn gmail_search(
    session: &mut ImapSession,
    uids: &[u32],
    criterion: &str,
) -> Result<std::collections::HashSet<u32>> {
    let (Some(min), Some(max)) = (uids.iter().min(), uids.iter().max()) else {
        return Ok(Default::default());
    };
    Ok(session.uid_search(format!("UID {min}:{max} {criterion}"))?)
}

/// Gmail labels of messages in `folder`, by UID: one search per label
/// (`labels` = the raw names of the user's labels).
pub fn fetch_labels(
    session: &mut ImapSession,
    folder: &str,
    uids: &[u32],
    labels: &[String],
) -> Result<HashMap<u32, Vec<String>>> {
    let mut out: HashMap<u32, Vec<String>> = HashMap::new();
    if uids.is_empty() {
        return Ok(out);
    }
    session.select(folder)?;
    for label in labels.iter().filter(|l| l.as_str() != folder) {
        for uid in gmail_search(session, uids, &format!("X-GM-LABELS {}", imap_quote(label)))? {
            out.entry(uid).or_default().push(label.clone());
        }
    }
    Ok(out)
}

/// Gmail inbox tabs. Primary is what no other tab takes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Category {
    #[default]
    Primary,
    Promotions,
    Social,
    Updates,
    Forums,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Primary,
        Category::Promotions,
        Category::Social,
        Category::Updates,
        Category::Forums,
    ];

    /// Stored in the cache as a number.
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(n: u8) -> Category {
        Category::ALL.get(n as usize).copied().unwrap_or_default()
    }

    /// The label's translation key.
    pub fn key(self) -> &'static str {
        match self {
            Category::Primary => "gmail_primary",
            Category::Promotions => "gmail_promotions",
            Category::Social => "gmail_social",
            Category::Updates => "gmail_updates",
            Category::Forums => "gmail_forums",
        }
    }

    fn search(self) -> Option<&'static str> {
        match self {
            Category::Primary => None,
            Category::Promotions => Some("category:promotions"),
            Category::Social => Some("category:social"),
            Category::Updates => Some("category:updates"),
            Category::Forums => Some("category:forums"),
        }
    }
}

/// The Gmail inbox tab of each message (Primary when no other).
pub fn fetch_categories(
    session: &mut ImapSession,
    folder: &str,
    uids: &[u32],
) -> Result<HashMap<u32, Category>> {
    let mut out = HashMap::new();
    if uids.is_empty() {
        return Ok(out);
    }
    session.select(folder)?;
    // Gmail can give a message more than one tab (often Updates and
    // Primary); its web inbox then shows it in Primary, so Primary wins.
    let primary: std::collections::HashSet<u32> = gmail_search(
        session,
        uids,
        &format!("X-GM-RAW {}", imap_quote("category:primary")),
    )?
    .into_iter()
    .collect();
    for cat in Category::ALL {
        let Some(raw) = cat.search() else { continue };
        for uid in gmail_search(session, uids, &format!("X-GM-RAW {}", imap_quote(raw)))? {
            if !primary.contains(&uid) {
                out.insert(uid, cat);
            }
        }
    }
    Ok(out)
}

/// Adds or removes a Gmail label (its raw IMAP name) on messages. `.SILENT`
/// keeps Gmail from answering with label lists the library cannot read.
pub fn set_gmail_label(
    session: &mut ImapSession,
    folder: &str,
    uids: &[u32],
    label: &str,
    on: bool,
) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    session.select(folder)?;
    let op = if on { '+' } else { '-' };
    session.uid_store(
        uid_set(uids),
        format!("{op}X-GM-LABELS.SILENT ({})", imap_quote(label)),
    )?;
    Ok(())
}

/// Stars or unstars messages (the \\Flagged flag).
pub fn set_flagged(session: &mut ImapSession, folder: &str, uids: &[u32], flagged: bool) -> Result<()> {
    if uids.is_empty() {
        return Ok(());
    }
    session.select(folder)?;
    let op = if flagged {
        "+FLAGS (\\Flagged)"
    } else {
        "-FLAGS (\\Flagged)"
    };
    session.uid_store(uid_set(uids), op)?;
    Ok(())
}

/// Creates a special folder (e.g. Archive) next to the inbox.
pub fn create_folder(session: &mut ImapSession, like: &[Folder], leaf: &str) -> Result<String> {
    // If folders live under INBOX (e.g. Aruba: INBOX.Sent), create INBOX.<leaf>.
    let nested = like
        .iter()
        .find(|f| f.kind != FolderKind::Inbox && f.kind != FolderKind::Other)
        .filter(|f| f.path.len() == 2 && f.path[0].eq_ignore_ascii_case("INBOX"));
    let name = match nested.and_then(|f| f.delimiter.clone()) {
        Some(d) => format!("INBOX{d}{leaf}"),
        None => leaf.to_string(),
    };
    session.create(&name)?;
    Ok(name)
}

// ---------------------------------------------------------------- sending

/// A unique Message-ID in the sender's domain: `<random@domain>`.
fn new_message_id(sender: &str) -> String {
    use rand::Rng;
    let domain = sender
        .rsplit_once('@')
        .map(|(_, d)| d.trim())
        .filter(|d| !d.is_empty())
        .unwrap_or("ferrowl.invalid");
    let id: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(24)
        .map(char::from)
        .collect();
    format!("<{id}.ferrowl@{domain}>")
}

/// Sends via SMTP and returns the formatted message (to save it in Sent).
pub fn send(account: &Account, mail: &Outgoing) -> Result<Vec<u8>> {
    use lettre::message::Mailbox;
    use lettre::transport::smtp::authentication::Credentials;
    use lettre::{Message, SmtpTransport, Transport};

    let from: Mailbox = format!("{} <{}>", account.display_name, account.email)
        .parse()
        .or_else(|_| account.email.parse())
        .context(t("err_bad_sender"))?;

    // A Message-ID lets replies thread to this message and is expected by
    // spam filters; lettre adds none by itself.
    let mut builder = Message::builder()
        .from(from)
        .subject(&mail.subject)
        .message_id(Some(new_message_id(&account.email)));
    let split = |s: &str| {
        split_addresses(s)
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    let to = split(&mail.to);
    if to.is_empty() {
        return Err(anyhow!(t("err_no_recipient")));
    }
    for a in to {
        builder = builder.to(a
            .parse()
            .with_context(|| tf("err_bad_address", &[("addr", &a)]))?);
    }
    for a in split(&mail.cc) {
        builder = builder.cc(a
            .parse()
            .with_context(|| tf("err_bad_address", &[("addr", &a)]))?);
    }
    for a in split(&mail.bcc) {
        builder = builder.bcc(
            a.parse()
                .with_context(|| tf("err_bad_address", &[("addr", &a)]))?,
        );
    }
    if let Some(id) = &mail.in_reply_to {
        let id = format!("<{}>", id.trim_matches(['<', '>']));
        builder = builder.in_reply_to(id.clone());
        let mut refs = mail.references.clone();
        if !refs.contains(&id) {
            refs.push(id);
        }
        builder = builder.references(refs.join(" "));
    }

    let content = build_content(mail)?;
    let content = protect(account, mail, content)?;
    if let Some(value) = crate::crypto::autocrypt_header(&account.email) {
        builder = builder.header(Autocrypt(value));
    }
    let email = match content {
        Part::Single(p) => builder.singlepart(p)?,
        Part::Multi(m) => builder.multipart(m)?,
    };

    let relay = if account.smtp_port == 465 {
        SmtpTransport::relay(&account.smtp_host)?
    } else {
        SmtpTransport::starttls_relay(&account.smtp_host)?
    };
    use lettre::transport::smtp::authentication::Mechanism;
    // Password, or (signed in with Google/Microsoft) a fresh access token.
    let (secret, mechanisms) = match account.oauth {
        Some(provider) => (
            crate::oauth::access_token(provider, &account.email)?,
            vec![Mechanism::Xoauth2],
        ),
        None => (account.password.clone(), vec![Mechanism::Plain, Mechanism::Login]),
    };
    let mailer = relay
        .port(account.smtp_port)
        .credentials(Credentials::new(account.username.clone(), secret))
        .authentication(mechanisms)
        .timeout(Some(Duration::from_secs(60)))
        .build();
    if let Err(e) = mailer.send(&email) {
        if mailbox_refuses_apps(&e.to_string()) {
            bail!(t("err_outlook_imap_off"));
        }
        if account.oauth.is_none()
            && is_gmail_host(&account.smtp_host)
            && crate::errors::kind(&e.to_string()) == crate::errors::Kind::WrongPassword
        {
            bail!(t("err_h_gmail_password"));
        }
        return Err(anyhow::Error::new(e).context(t("err_smtp")));
    }
    Ok(email.formatted())
}

/// Gmail's servers (also for Google Workspace domains, which use them too).
fn is_gmail_host(host: &str) -> bool {
    let h = host.trim().to_ascii_lowercase();
    h.ends_with("gmail.com") || h.ends_with("googlemail.com")
}

/// Outlook.com / Microsoft 365 answers like this when the sign-in worked
/// but the mailbox does not allow IMAP or SMTP from apps (off by default
/// on many Outlook.com mailboxes, or disabled by the administrator).
fn mailbox_refuses_apps(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("authenticated but not connected")
        || m.contains("smtpclientauthentication is disabled")
        || m.contains("5.7.139")
}

/// Saves a copy of the sent message to the Sent folder.
pub fn append_sent(session: &mut ImapSession, folder: &str, raw: &[u8]) -> Result<()> {
    session.append_with_flags(folder, raw, &[imap::types::Flag::Seen])?;
    Ok(())
}

fn guess_mime(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or_default().to_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "zip" => "application/zip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xml" => "application/xml",
        "ics" => "text/calendar",
        "odt" => "application/vnd.oasis.opendocument.text",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        _ => "application/octet-stream",
    }
}

/// A MIME body: one part or a multipart tree.
enum Part {
    Single(lettre::message::SinglePart),
    Multi(lettre::message::MultiPart),
}

impl Part {
    fn formatted(&self) -> Vec<u8> {
        match self {
            Part::Single(p) => p.formatted(),
            Part::Multi(m) => m.formatted(),
        }
    }

    fn into_multi(self, outer: lettre::message::MultiPartBuilder) -> lettre::message::MultiPart {
        match self {
            Part::Single(p) => outer.singlepart(p),
            Part::Multi(m) => outer.multipart(m),
        }
    }
}

/// Text (+ HTML alternative) (+ attachments).
fn build_content(mail: &Outgoing) -> Result<Part> {
    use lettre::message::{Attachment as LAttachment, MultiPart, SinglePart, header};
    let text = SinglePart::builder()
        .header(header::ContentType::TEXT_PLAIN)
        .body(mail.body.clone());
    let alternative = match &mail.calendar {
        // Invitation reply: plain text and the calendar object, which
        // calendar programs read (RFC 6047). Its METHOD goes in the type.
        Some(ics) => {
            let method = crate::ical::parse(ics).method;
            let method = if method.is_empty() {
                "PUBLISH".into()
            } else {
                method
            };
            let ct = header::ContentType::parse(&format!("text/calendar; charset=utf-8; method={method}"))
                .map_err(|e| anyhow!("{e}"))?;
            Some(
                MultiPart::alternative()
                    .singlepart(text.clone())
                    .singlepart(SinglePart::builder().header(ct).body(ics.clone())),
            )
        }
        None => mail
            .html
            .as_ref()
            .map(|html| MultiPart::alternative_plain_html(mail.body.clone(), html.clone())),
    };
    if mail.attachments.is_empty() {
        return Ok(match alternative {
            Some(alt) => Part::Multi(alt),
            None => Part::Single(text),
        });
    }
    let mut multi = match alternative {
        Some(alt) => MultiPart::mixed().multipart(alt),
        None => MultiPart::mixed().singlepart(text),
    };
    for path in &mail.attachments {
        let data = std::fs::read(path)
            .with_context(|| tf("err_attach_read", &[("path", &path.display().to_string())]))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "attachment".into());
        let ct = header::ContentType::parse(guess_mime(&name))
            .unwrap_or(header::ContentType::parse("application/octet-stream").unwrap());
        multi = multi.singlepart(LAttachment::new(name).body(data, ct));
    }
    Ok(Part::Multi(multi))
}

/// Applies OpenPGP (RFC 3156): sign → multipart/signed; encrypt →
/// multipart/encrypted (signed first when both).
fn protect(account: &Account, mail: &Outgoing, content: Part) -> Result<Part> {
    if !mail.sign && !mail.encrypt {
        return Ok(content);
    }
    let own = crate::crypto::own_key(&account.email).ok_or_else(|| anyhow!(t("err_pgp_no_own_key")))?;
    protect_with(
        mail,
        content,
        &own,
        &account.pgp_passphrase,
        &crate::crypto::encryption_key,
    )
}

fn protect_with(
    mail: &Outgoing,
    content: Part,
    own: &pgp::composed::SignedSecretKey,
    passphrase: &str,
    contact: &dyn Fn(&str) -> Option<pgp::composed::SignedPublicKey>,
) -> Result<Part> {
    use lettre::message::{MultiPart, SinglePart, header::ContentType};
    let mut content = content;
    if mail.sign {
        // RFC 3156: the line break before the next delimiter belongs to the
        // delimiter, not to the signed part.
        let mut signed_bytes = content.formatted();
        if signed_bytes.ends_with(b"\r\n") {
            signed_bytes.truncate(signed_bytes.len() - 2);
        }
        let signature = crate::crypto::sign(&signed_bytes, own, passphrase)?;
        let sig_part = SinglePart::builder()
            .header(ContentType::parse("application/pgp-signature; name=\"signature.asc\"").unwrap())
            .body(signature);
        let outer = MultiPart::signed("application/pgp-signature".into(), "pgp-sha256".into());
        content = Part::Multi(content.into_multi(outer).singlepart(sig_part));
    }
    if mail.encrypt {
        let mut keys = vec![own.to_public_key()];
        let all = [&mail.to, &mail.cc, &mail.bcc]
            .iter()
            .flat_map(|s| split_addresses(s))
            .map(bare_address)
            .filter(|a| !a.is_empty())
            .collect::<Vec<_>>();
        for address in all {
            let key =
                contact(&address).ok_or_else(|| anyhow!(tf("err_pgp_no_key", &[("addr", &address)])))?;
            keys.push(key);
        }
        let armored = crate::crypto::encrypt(&content.formatted(), &keys)?;
        let version = SinglePart::builder()
            .header(ContentType::parse("application/pgp-encrypted").unwrap())
            .body(String::from("Version: 1\r\n"));
        let data = SinglePart::builder()
            .header(ContentType::parse("application/octet-stream; name=\"encrypted.asc\"").unwrap())
            .body(armored);
        content = Part::Multi(
            MultiPart::encrypted("application/pgp-encrypted".into())
                .singlepart(version)
                .singlepart(data),
        );
    }
    Ok(content)
}

/// "Name <a@b.it>" → "a@b.it".
pub fn bare_address(s: &str) -> String {
    let s = s.trim();
    match (s.rfind('<'), s.rfind('>')) {
        (Some(a), Some(b)) if a < b => s[a + 1..b].trim().to_lowercase(),
        _ => s.to_lowercase(),
    }
}

/// `Autocrypt:` header (lettre has no type for it).
#[derive(Clone)]
struct Autocrypt(String);

impl lettre::message::header::Header for Autocrypt {
    fn name() -> lettre::message::header::HeaderName {
        lettre::message::header::HeaderName::new_from_ascii_str("Autocrypt")
    }
    fn parse(s: &str) -> std::result::Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Autocrypt(s.to_string()))
    }
    fn display(&self) -> lettre::message::header::HeaderValue {
        lettre::message::header::HeaderValue::new(Self::name(), self.0.clone())
    }
}

#[cfg(test)]
mod status_tests {
    #[test]
    fn unseen_counts_are_read_from_status_answers() {
        assert_eq!(super::unseen_in(b"* STATUS INBOX (UNSEEN 3)\r\n"), Some(3));
        assert_eq!(
            super::unseen_in(b"* STATUS \"Posta in arrivo\" (MESSAGES 9 UNSEEN 12)\r\n"),
            Some(12)
        );
        assert_eq!(super::unseen_in(b"* STATUS Archivio ()\r\n"), None);
        assert_eq!(super::quote_mailbox("A \"b\""), "\"A \\\"b\\\"\"");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlook_mailbox_without_imap_is_recognised() {
        assert!(mailbox_refuses_apps("User is authenticated but not connected."));
        assert!(mailbox_refuses_apps(
            "permanent error (535): 5.7.139 Authentication unsuccessful, SmtpClientAuthentication is disabled for the Mailbox."
        ));
        assert!(!mailbox_refuses_apps("AUTHENTICATIONFAILED Invalid credentials"));
    }

    #[test]
    fn invitation_reply_carries_the_calendar_part() {
        let mail = Outgoing {
            to: "giulia@example.com".into(),
            body: "Fabio has accepted the invitation.".into(),
            calendar: Some("BEGIN:VCALENDAR\r\nMETHOD:REPLY\r\nEND:VCALENDAR\r\n".into()),
            ..Default::default()
        };
        let Part::Multi(m) = build_content(&mail).unwrap() else {
            panic!("expected multipart/alternative");
        };
        let raw = String::from_utf8_lossy(&m.formatted()).into_owned();
        assert!(raw.contains("multipart/alternative"));
        assert!(
            raw.contains("text/calendar; charset=utf-8; method=REPLY"),
            "{raw}"
        );
        assert!(raw.contains("METHOD:REPLY"));
    }

    #[test]
    fn imap_quoting_and_categories() {
        assert_eq!(imap_quote(r#"Say "hi" \ bye"#), r#""Say \"hi\" \\ bye""#);
        for c in Category::ALL {
            assert_eq!(Category::from_code(c.code()), c);
        }
    }

    #[test]
    fn utf7_imap() {
        assert_eq!(decode_utf7_imap("INBOX"), "INBOX");
        assert_eq!(decode_utf7_imap("Posta &AOg- qui"), "Posta è qui");
        assert_eq!(decode_utf7_imap("A&-B"), "A&B");
        assert_eq!(decode_utf7_imap("&ZeVnLIqe-"), "日本語");
    }

    #[test]
    fn classify_aruba_style() {
        let seg = |s: &str| s.split('.').map(str::to_string).collect::<Vec<_>>();
        assert_eq!(classify("INBOX.Sent", &seg("INBOX.Sent"), &[]), FolderKind::Sent);
        assert_eq!(
            classify("INBOX.Trash", &seg("INBOX.Trash"), &[]),
            FolderKind::Trash
        );
        assert_eq!(
            classify("INBOX.Sent.2004.04-apr", &seg("INBOX.Sent.2004.04-apr"), &[]),
            FolderKind::Other
        );
        assert_eq!(classify("Junk", &seg("Junk"), &[]), FolderKind::Spam);
    }

    #[test]
    fn header_values() {
        let raw = b"Subject: x\r\nAuthentication-Results: mx.aruba.it;\r\n dkim=pass header.d=x\r\n\r\nbody";
        let v = raw_header_values(raw, "authentication-results");
        assert_eq!(v.len(), 1);
        assert!(v[0].contains("dkim=pass"));
    }

    /// Requires network: `cargo test -- --ignored`
    /// Sends a message to the account itself and checks its headers:
    /// `FERROWL_TEST_CONFIG=<config.toml with [account]> cargo test -p ferrowl-core live_send_has_message_id -- --ignored`
    #[test]
    #[ignore]
    fn live_send_has_message_id() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let path = std::env::var("FERROWL_TEST_CONFIG").expect("FERROWL_TEST_CONFIG");
        let table: toml::Table = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let account: Account = table["account"].clone().try_into().unwrap();
        let raw = send(
            &account,
            &Outgoing {
                to: account.email.clone(),
                cc: String::new(),
                bcc: String::new(),
                subject: "Ferrowl Message-ID test".into(),
                body: "Safe to delete.".into(),
                html: None,
                in_reply_to: Some("<parent@example.org>".into()),
                references: Vec::new(),
                attachments: Vec::new(),
                sign: false,
                encrypt: false,
                calendar: None,
            },
        )
        .unwrap();
        let head = String::from_utf8_lossy(&raw);
        assert!(head.contains("Message-ID: <"), "no Message-ID");
        assert!(head.contains("In-Reply-To: <parent@example.org>"));
    }

    /// Read-only: what a new-mail notification reads, against the inbox
    /// of the `[account]` of a config. Prints counts only.
    /// `FERROWL_TEST_CONFIG=<config.toml> cargo test -p ferrowl-core live_peek_inbox -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_peek_inbox() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let path = std::env::var("FERROWL_TEST_CONFIG").expect("FERROWL_TEST_CONFIG");
        let table: toml::Table = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let account: Account = table["account"].clone().try_into().unwrap();
        let mut s = connect(&account).unwrap();
        let all = peek_inbox(&mut s, 0, 3).unwrap();
        let newest = all.newest.as_ref().expect("an empty inbox tells nothing");
        assert!(all.fresh.len() <= 3);
        assert!(all.fresh.windows(2).all(|w| w[0].uid > w[1].uid), "newest first");
        assert!(all.fresh.iter().all(|h| !h.seen));
        assert!(!newest.subject.is_empty() || !newest.from_address.is_empty());
        // Nothing above the newest message.
        let none = peek_inbox(&mut s, newest.uid, 3).unwrap();
        assert_eq!(none.fresh_count, 0);
        assert_eq!(none.newest.map(|h| h.uid), Some(newest.uid));
        assert!(unseen_from(&mut s, newest.uid + 1).unwrap().is_empty());
        println!(
            "unread {}, fresh {}, newest has subject: {}",
            all.unread,
            all.fresh_count,
            !newest.subject.is_empty()
        );
        let _ = s.logout();
    }

    /// Bulk commands on messages whose subject starts with "Ferrowl bulk test":
    /// unread, read, archive, then permanent delete from the archive.
    /// Read-only checks on a Gmail account of a profile's config (index in
    /// FERROWL_TEST_ACCOUNT): folders, list, labels, unread counts and the
    /// inbox categories. Prints counts only.
    /// `FERROWL_TEST_CONFIG=… FERROWL_TEST_ACCOUNT=1 XDG_DATA_HOME=… FERROWL_NO_KEYCHAIN=1 cargo test -p ferrowl-core live_gmail -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_gmail() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let path = std::env::var("FERROWL_TEST_CONFIG").expect("FERROWL_TEST_CONFIG");
        let index: usize = std::env::var("FERROWL_TEST_ACCOUNT")
            .unwrap_or_default()
            .parse()
            .unwrap_or(0);
        let table: toml::Table = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let account: Account = table["accounts"].as_array().unwrap()[index]
            .clone()
            .try_into()
            .unwrap();
        let mut s = connect(&account).unwrap();
        println!("gmail extension: {}", has_gmail_labels(&mut s));
        let folders = list_folders(&mut s).unwrap();
        println!("folders: {}", folders.len());
        let page = fetch_headers(&mut s, "INBOX").unwrap();
        println!("inbox page: {} messages", page.headers.len());
        let uids: Vec<u32> = page.headers.iter().map(|h| h.uid).collect();
        let names: Vec<String> = folders
            .iter()
            .filter(|f| f.kind == FolderKind::Other && f.selectable && !f.name.starts_with("[Gmail]"))
            .map(|f| f.name.clone())
            .collect();
        let labels = fetch_labels(&mut s, "INBOX", &uids, &names).unwrap();
        println!("labels: {} | labelled messages: {}", names.len(), labels.len());
        let cats = fetch_categories(&mut s, "INBOX", &uids).unwrap();
        for c in Category::ALL {
            println!("category {:?}: {}", c, cats.values().filter(|x| **x == c).count());
        }
        for f in folders.iter().filter(|f| f.selectable) {
            match s.status(&f.name, "(UNSEEN)") {
                Ok(_) => {}
                Err(e) => println!("STATUS {} failed: {e}", f.name),
            }
        }
        let _ = s.logout();
    }

    /// Spam and back: a message whose subject starts with "Ferrowl spam
    /// test" goes to Spam marked $Junk, then back to the inbox marked
    /// $NotJunk, and is finally deleted.
    /// `FERROWL_TEST_CONFIG=<config.toml with [account]> cargo test -p ferrowl-core live_spam -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_spam() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let path = std::env::var("FERROWL_TEST_CONFIG").expect("FERROWL_TEST_CONFIG");
        let table: toml::Table = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let account: Account = table["account"].clone().try_into().unwrap();
        let mut s = connect(&account).unwrap();
        let folders = list_folders(&mut s).unwrap();
        let spam = folders
            .iter()
            .find(|f| f.kind == FolderKind::Spam)
            .map(|f| f.name.clone())
            .expect("spam folder");
        let find = |s: &mut ImapSession, folder: &str| -> Vec<u32> {
            fetch_headers(s, folder)
                .unwrap()
                .headers
                .iter()
                .filter(|h| h.subject.starts_with("Ferrowl spam test"))
                .map(|h| h.uid)
                .collect()
        };
        let uids = find(&mut s, "INBOX");
        assert_eq!(uids.len(), 1, "send one test message first");
        set_keywords(&mut s, "INBOX", &uids, &["$Junk"], &["$NotJunk"]);
        move_message(&mut s, "INBOX", &uids, &spam).unwrap();
        let in_spam = find(&mut s, &spam);
        assert_eq!(in_spam.len(), 1, "moved to {spam}");
        println!("in {spam}: uid {:?}", in_spam);
        set_keywords(&mut s, &spam, &in_spam, &["$NotJunk"], &["$Junk"]);
        move_message(&mut s, &spam, &in_spam, "INBOX").unwrap();
        let back = find(&mut s, "INBOX");
        assert_eq!(back.len(), 1, "back in the inbox");
        assert!(find(&mut s, &spam).is_empty());
        delete_permanently(&mut s, "INBOX", &back).unwrap();
        let _ = s.logout();
    }

    /// `FERROWL_TEST_CONFIG=<config.toml with [account]> cargo test -p ferrowl-core live_bulk_ops -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_bulk_ops() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let path = std::env::var("FERROWL_TEST_CONFIG").expect("FERROWL_TEST_CONFIG");
        let table: toml::Table = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let account: Account = table["account"].clone().try_into().unwrap();
        let mut s = connect(&account).unwrap();
        let folders = list_folders(&mut s).unwrap();
        let page = fetch_headers(&mut s, "INBOX").unwrap();
        let uids: Vec<u32> = page
            .headers
            .iter()
            .filter(|h| h.subject.starts_with("Ferrowl bulk test"))
            .map(|h| h.uid)
            .collect();
        assert_eq!(uids.len(), 2, "send two test messages first");
        let seen = |s: &mut ImapSession| {
            fetch_headers(s, "INBOX")
                .unwrap()
                .headers
                .iter()
                .filter(|h| uids.contains(&h.uid))
                .map(|h| h.seen)
                .collect::<Vec<_>>()
        };
        set_seen(&mut s, "INBOX", &uids, false).unwrap();
        assert_eq!(seen(&mut s), vec![false, false]);
        set_seen(&mut s, "INBOX", &uids, true).unwrap();
        assert_eq!(seen(&mut s), vec![true, true]);

        let archive = folders
            .iter()
            .find(|f| f.kind == FolderKind::Archive)
            .map(|f| f.name.clone())
            .expect("archive folder");
        move_message(&mut s, "INBOX", &uids, &archive).unwrap();
        let left = fetch_headers(&mut s, "INBOX").unwrap();
        assert!(
            !left
                .headers
                .iter()
                .any(|h| h.subject.starts_with("Ferrowl bulk test"))
        );
        let moved: Vec<u32> = fetch_headers(&mut s, &archive)
            .unwrap()
            .headers
            .iter()
            .filter(|h| h.subject.starts_with("Ferrowl bulk test"))
            .map(|h| h.uid)
            .collect();
        assert_eq!(moved.len(), 2);
        delete_permanently(&mut s, &archive, &moved).unwrap();
        let gone = fetch_headers(&mut s, &archive).unwrap();
        assert!(
            !gone
                .headers
                .iter()
                .any(|h| h.subject.starts_with("Ferrowl bulk test"))
        );
        println!("bulk unread/read/archive/delete ok on {} messages", uids.len());
    }

    #[test]
    fn recipients_are_parsed() {
        assert_eq!(
            parse_recipients("Ada <ada@x.org>, bob@y.org; \"Carl C\" <carl@z.org>, nobody"),
            vec![
                ("Ada".to_string(), "ada@x.org".to_string()),
                (String::new(), "bob@y.org".to_string()),
                ("Carl C".to_string(), "carl@z.org".to_string()),
            ]
        );
    }

    #[test]
    fn sending_splits_addresses_outside_quotes() {
        let field = "\"Rossi, Mario\" <m@x.it>; ugo@y.it,  ,";
        let parts = split_addresses(field);
        assert_eq!(parts, ["\"Rossi, Mario\" <m@x.it>", "ugo@y.it"]);
        // Each part is one address for lettre (what send builds the
        // message from) and for the PGP key lookup.
        let first: lettre::message::Mailbox = parts[0].parse().unwrap();
        assert_eq!(first.name.as_deref(), Some("Rossi, Mario"));
        assert_eq!(first.email.to_string(), "m@x.it");
        let bare: Vec<String> = parts.iter().map(|p| bare_address(p)).collect();
        assert_eq!(bare, ["m@x.it", "ugo@y.it"]);
    }

    #[test]
    fn recipients_keep_separators_inside_quotes_and_brackets() {
        assert_eq!(
            parse_recipients("\"Rossi, Mario\" <m@x.it>; \"Bianchi; Anna\" <a@x.it>,ugo@y.it"),
            vec![
                ("Rossi, Mario".to_string(), "m@x.it".to_string()),
                ("Bianchi; Anna".to_string(), "a@x.it".to_string()),
                (String::new(), "ugo@y.it".to_string()),
            ]
        );
        // An escaped quote doesn't end the name; a comma in the brackets
        // doesn't split the address.
        assert_eq!(
            parse_recipients(r#""Ada \"A, B\" L" <ada@x.org>, Bob <bob,x@y.org>"#),
            vec![
                (r#"Ada "A, B" L"#.to_string(), "ada@x.org".to_string()),
                ("Bob".to_string(), "bob,x@y.org".to_string()),
            ]
        );
    }

    #[test]
    fn message_ids_are_unique_and_in_sender_domain() {
        let a = new_message_id("ada@example.org");
        let b = new_message_id("ada@example.org");
        assert!(a.starts_with('<') && a.ends_with(".ferrowl@example.org>"));
        assert_ne!(a, b);
        assert!(new_message_id("broken").ends_with("@ferrowl.invalid>"));
    }

    #[test]
    #[ignore]
    fn tls_handshake_and_login_rejected() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        crate::i18n::set(Some("en"));
        let acc = Account {
            username: "nonexistent-user@gmail.com".into(),
            password: "wrong".into(),
            imap_host: "imap.gmail.com".into(),
            ..Default::default()
        };
        let err = connect(&acc).err().expect("login must fail");
        assert!(format!("{err:#}").contains("IMAP login failed"), "{err:#}");
    }

    /// Uses the configured account: `cargo test live_account -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_account() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let acc = crate::config::Config::load()
            .account()
            .cloned()
            .expect("no account");
        let mut s = connect(&acc).unwrap_or_else(|e| panic!("IMAP ERROR: {e:#}"));
        let folders = list_folders(&mut s).unwrap();
        for f in folders.iter().filter(|f| f.kind != FolderKind::Other) {
            println!("special: {:?} -> {}", f.kind, f.name);
        }
        let names: Vec<String> = folders
            .iter()
            .filter(|f| f.kind != FolderKind::Other)
            .map(|f| f.name.clone())
            .collect();
        println!("unread: {:?}", unseen_counts(&mut s, &names));
        let h = fetch_headers(&mut s, "INBOX").unwrap().headers;
        println!("INBOX: {} messages", h.len());
        for m in h.iter().take(3) {
            println!(
                "- [{}{}] {} | {} | {}",
                if m.seen { " " } else { "●" },
                if m.has_attachment { "📎" } else { " " },
                m.from,
                m.subject,
                m.preview
            );
        }
    }

    #[test]
    #[ignore = "uses the configured account"]
    fn live_idle() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let acc = crate::config::Config::load()
            .account()
            .cloned()
            .expect("no account");
        let mut s = connect(&acc).unwrap();
        assert!(supports_idle(&mut s).unwrap(), "server without IDLE");
        let t = std::time::Instant::now();
        let changed = idle_wait(&mut s, "INBOX", Duration::from_secs(5)).unwrap();
        println!("idle: changed={changed} after {:?}", t.elapsed());
        // The session is still usable after IDLE.
        assert!(s.select("INBOX").unwrap().exists > 0);
        let _ = s.logout();
    }

    #[test]
    fn pgp_mime_roundtrip() {
        use crate::crypto;
        let alice = crypto::generate("Alice", "alice@example.com", "a").unwrap();
        let bob = crypto::generate("Bob", "bob@example.com", "").unwrap();
        let (alice_pub, bob_pub) = (alice.to_public_key(), bob.to_public_key());
        let mut out = Outgoing {
            to: "Bob <bob@example.com>".into(),
            subject: "s".into(),
            body: "Ciao Bob, **prova** con àccénti.".into(),
            html: Some("<p>Ciao Bob, <b>prova</b> con àccénti.</p>".into()),
            sign: true,
            ..Default::default()
        };
        let dir = std::env::temp_dir().join("ferrowl-pgp-test");
        std::fs::create_dir_all(&dir).unwrap();
        let att = dir.join("note.txt");
        std::fs::write(&att, b"allegato").unwrap();
        out.attachments = vec![att];

        let build = |out: &Outgoing| -> Vec<u8> {
            let content = build_content(out).unwrap();
            let contact = |a: &str| (a == "bob@example.com").then(|| bob_pub.clone());
            let content = protect_with(out, content, &alice, "a", &contact).unwrap();
            let b = lettre::Message::builder()
                .from("Alice <alice@example.com>".parse().unwrap())
                .to("Bob <bob@example.com>".parse().unwrap())
                .subject("s");
            match content {
                Part::Single(p) => b.singlepart(p).unwrap(),
                Part::Multi(m) => b.multipart(m).unwrap(),
            }
            .formatted()
        };
        let knows_alice = |a: &str| (a == "alice@example.com").then(|| alice_pub.clone());
        let knows_nobody = |_: &str| None;
        let learns_nothing = |_: &str, _: &pgp::composed::SignedPublicKey| {};
        let bob_keys = PgpKeys {
            own: Some((bob.clone(), String::new())),
            contact: &knows_alice,
            autocrypt: &knows_nobody,
            learn: &learns_nothing,
        };

        // Signed only.
        let raw = build(&out);
        let m = parse_full(1, &raw, &bob_keys).unwrap();
        let pgp = m.pgp.clone().unwrap();
        assert_eq!(pgp.signature, Some(SigState::Valid), "signed: {pgp:?}");
        assert!(!pgp.encrypted);
        assert!(m.body.contains("àccénti"));
        assert_eq!(m.attachments.len(), 1, "signature.asc must not be listed");
        let unknown = PgpKeys {
            own: None,
            contact: &knows_nobody,
            autocrypt: &knows_nobody,
            learn: &learns_nothing,
        };
        assert_eq!(
            parse_full(1, &raw, &unknown).unwrap().pgp.unwrap().signature,
            Some(SigState::UnknownKey)
        );
        // Tampered content → invalid.
        let tampered = String::from_utf8_lossy(&raw)
            .replace("prova", "PROVA")
            .into_bytes();
        assert_eq!(
            parse_full(1, &tampered, &bob_keys)
                .unwrap()
                .pgp
                .unwrap()
                .signature,
            Some(SigState::Invalid)
        );

        // Signed and encrypted.
        out.encrypt = true;
        let raw = build(&out);
        assert!(
            !String::from_utf8_lossy(&raw).contains("prova"),
            "plaintext leaked"
        );
        let m = parse_full(1, &raw, &bob_keys).unwrap();
        let pgp = m.pgp.clone().unwrap();
        assert!(pgp.encrypted && pgp.error.is_none(), "{pgp:?}");
        assert_eq!(pgp.signature, Some(SigState::Valid));
        assert!(m.body.contains("àccénti") && m.html.is_some());
        assert_eq!(m.attachments.len(), 1);
        // Someone without the key cannot read it.
        let eve = crypto::generate("Eve", "eve@example.com", "").unwrap();
        let eve_keys = PgpKeys {
            own: Some((eve, String::new())),
            contact: &knows_nobody,
            autocrypt: &knows_nobody,
            learn: &learns_nothing,
        };
        let m = parse_full(1, &raw, &eve_keys).unwrap();
        assert!(m.pgp.unwrap().error.is_some());
        assert!(!m.body.contains("prova"));
        // Alice's key only known from Autocrypt: signed, but unverified.
        out.encrypt = false;
        out.to = "Bob <bob@example.com>".into();
        let raw = build(&out);
        let autocrypt_alice = PgpKeys {
            own: None,
            contact: &knows_nobody,
            autocrypt: &knows_alice,
            learn: &learns_nothing,
        };
        assert_eq!(
            parse_full(1, &raw, &autocrypt_alice).unwrap().pgp.unwrap().signature,
            Some(SigState::Unverified)
        );
        // Eve writes as Alice, signs with her own key and offers it in an
        // Autocrypt header: never valid, never learned over Alice's key.
        let content = build_content(&out).unwrap();
        let contact = |a: &str| (a == "bob@example.com").then(|| bob_pub.clone());
        // Her key names Alice's address, so the header passes as Alice's.
        let fake = crypto::generate("Alice", "alice@example.com", "").unwrap();
        let content = protect_with(&out, content, &fake, "", &contact).unwrap();
        let eve_pub = fake.to_public_key();
        let mut keydata = Vec::new();
        pgp::ser::Serialize::to_writer(&eve_pub, &mut keydata).unwrap();
        use base64::Engine;
        let header = format!(
            "addr=alice@example.com; keydata={}",
            base64::engine::general_purpose::STANDARD.encode(keydata)
        );
        let b = lettre::Message::builder()
            .from("Alice <alice@example.com>".parse().unwrap())
            .to("Bob <bob@example.com>".parse().unwrap())
            .header(Autocrypt(header))
            .subject("s");
        let spoofed = match content {
            Part::Single(p) => b.singlepart(p).unwrap(),
            Part::Multi(m) => b.multipart(m).unwrap(),
        }
        .formatted();
        let learned = std::cell::RefCell::new(Vec::new());
        let remember = |a: &str, k: &pgp::composed::SignedPublicKey| {
            learned.borrow_mut().push((a.to_string(), crate::crypto::fingerprint(k)))
        };
        let watching = PgpKeys {
            own: None,
            contact: &knows_alice,
            autocrypt: &knows_nobody,
            learn: &remember,
        };
        let m = parse_full(1, &spoofed, &watching).unwrap();
        assert_eq!(m.pgp.unwrap().signature, Some(SigState::KeyChanged));
        let first_contact = PgpKeys {
            own: None,
            contact: &knows_nobody,
            autocrypt: &knows_nobody,
            learn: &remember,
        };
        let m = parse_full(1, &spoofed, &first_contact).unwrap();
        assert_eq!(m.pgp.unwrap().signature, Some(SigState::Unverified));
        // parse_full offers the key; crypto::learn_autocrypt keeps keys it
        // already holds (see crypto tests).
        assert_eq!(learned.borrow().len(), 2);
        // Encrypting to someone without a key fails before sending.
        out.encrypt = true;
        out.to = "carol@example.com".into();
        let content = build_content(&out).unwrap();
        let contact = |_: &str| None;
        assert!(protect_with(&out, content, &alice, "a", &contact).is_err());
    }

    /// Sends a test message from the configured account to itself and saves
    /// the copy in Sent, exactly like the app does.
    #[test]
    #[ignore = "sends a real email from the configured account"]
    fn live_send_to_self() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let acc = crate::config::Config::load()
            .account()
            .cloned()
            .expect("no account");
        let dir = std::env::temp_dir().join("ferrowl-live-send");
        std::fs::create_dir_all(&dir).unwrap();
        let att = dir.join("ferrowl-test.txt");
        std::fs::write(&att, "Allegato di prova inviato da Ferrowl.\n").unwrap();
        let body = "Ciao,\n\nquesta è una **mail di prova** inviata da _Ferrowl_.\n\n- testo formattato (HTML + testo)\n- un allegato\n- copia salvata in Inviati\n\nFerrowl";
        let out = Outgoing {
            to: acc.email.clone(),
            subject: format!("Ferrowl: mail di prova {}", chrono::Local::now().format("%H:%M")),
            body: body.into(),
            html: Some(crate::markdown::to_html_document(body)),
            attachments: vec![att],
            ..Default::default()
        };
        let raw = send(&acc, &out).expect("SMTP send");
        println!("sent: {} bytes", raw.len());
        let mut s = connect(&acc).unwrap();
        let sent = list_folders(&mut s)
            .unwrap()
            .into_iter()
            .find(|f| f.kind == FolderKind::Sent)
            .expect("no Sent folder");
        append_sent(&mut s, &sent.name, &raw).expect("append to Sent");
        println!("copy saved in {}", sent.name);
        let _ = s.logout();
    }
}
