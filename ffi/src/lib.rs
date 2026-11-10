//! Bindings of `ferrowl-core` for the mobile apps, generated with UniFFI.
//!
//! The apps see one object, [`Engine`], with `suspend` (Kotlin) / `async`
//! (Swift) methods, and plain data records. These records mirror the core
//! types on purpose: the core can change freely while this surface, which
//! the Kotlin and Swift code is written against, stays stable.
//!
//! The apps own account settings and secrets (Android Keystore / iOS
//! Keychain) and pass a complete [`Account`] to [`Engine::connect`]; the
//! engine never reads a config file on mobile.

use ferrowl_core::backend::Backend;
use ferrowl_core::{bridge, config, events, i18n, invite, mail, oauth, paths, providers};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

uniffi::setup_scaffolding!();

// ---------------------------------------------------------------- errors

/// Every failure reaches the app as a message ready to show to the user
/// (already translated into the engine's language).
#[derive(Debug, uniffi::Error)]
#[uniffi(flat_error)]
pub enum FerrowlError {
    Failed(String),
}

impl std::fmt::Display for FerrowlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FerrowlError::Failed(msg) => f.write_str(msg),
        }
    }
}

impl From<String> for FerrowlError {
    /// Messages reach the app in words for the user (already-explained
    /// ones pass through unchanged).
    fn from(msg: String) -> Self {
        FerrowlError::Failed(ferrowl_core::errors::humanize(&msg))
    }
}

type Result<T> = std::result::Result<T, FerrowlError>;

// ---------------------------------------------------------------- records

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Protocol {
    Imap,
    Pop3,
}

/// An account as the app stores it. For POP3, `incoming_host`/`incoming_port`
/// are the POP3 server.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Account {
    pub display_name: String,
    pub email: String,
    pub username: String,
    pub password: String,
    pub incoming_host: String,
    pub incoming_port: u16,
    pub smtp_host: String,
    /// 465 = TLS, 587 = STARTTLS.
    pub smtp_port: u16,
    pub protocol: Protocol,
    /// POP3 only: days a message stays on the server (`None` = always).
    pub pop3_keep_days: Option<u32>,
    /// Passphrase of the account's OpenPGP key (empty = none).
    pub pgp_passphrase: String,
    /// Contacts server (CardDAV); empty = found from the address's domain.
    #[uniffi(default = "")]
    pub carddav_url: String,
    /// Calendar server (CalDAV); empty = found from the address's domain.
    #[uniffi(default = "")]
    pub caldav_url: String,
    /// Signed in with Microsoft/Google instead of a password.
    #[uniffi(default = None)]
    pub oauth: Option<OAuthProvider>,
}

/// Providers whose accounts sign in in the browser (OAuth 2.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum OAuthProvider {
    Microsoft,
    Google,
}

impl From<OAuthProvider> for oauth::Provider {
    fn from(p: OAuthProvider) -> Self {
        match p {
            OAuthProvider::Microsoft => oauth::Provider::Microsoft,
            OAuthProvider::Google => oauth::Provider::Google,
        }
    }
}

impl From<oauth::Provider> for OAuthProvider {
    fn from(p: oauth::Provider) -> Self {
        match p {
            oauth::Provider::Microsoft => OAuthProvider::Microsoft,
            oauth::Provider::Google => OAuthProvider::Google,
        }
    }
}

impl From<Account> for config::Account {
    fn from(a: Account) -> Self {
        config::Account {
            display_name: a.display_name,
            email: a.email,
            username: a.username,
            password: a.password,
            imap_host: a.incoming_host,
            imap_port: a.incoming_port,
            smtp_host: a.smtp_host,
            smtp_port: a.smtp_port,
            pgp_passphrase: a.pgp_passphrase,
            protocol: match a.protocol {
                Protocol::Imap => config::Protocol::Imap,
                Protocol::Pop3 => config::Protocol::Pop3,
            },
            pop3_keep_days: a.pop3_keep_days,
            keyring: false,
            carddav_url: a.carddav_url,
            caldav_url: a.caldav_url,
            oauth: a.oauth.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FolderKind {
    Inbox,
    Sent,
    Drafts,
    Archive,
    Spam,
    Trash,
    Other,
}

impl From<mail::FolderKind> for FolderKind {
    fn from(k: mail::FolderKind) -> Self {
        match k {
            mail::FolderKind::Inbox => FolderKind::Inbox,
            mail::FolderKind::Sent => FolderKind::Sent,
            mail::FolderKind::Drafts => FolderKind::Drafts,
            mail::FolderKind::Archive => FolderKind::Archive,
            mail::FolderKind::Spam => FolderKind::Spam,
            mail::FolderKind::Trash => FolderKind::Trash,
            mail::FolderKind::Other => FolderKind::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Folder {
    /// Server name: pass it back to the engine as is.
    pub id: String,
    /// Name to show (translated for the standard folders).
    pub name: String,
    /// Path segments, for nesting.
    pub path: Vec<String>,
    pub kind: FolderKind,
    pub selectable: bool,
}

impl From<&mail::Folder> for Folder {
    fn from(f: &mail::Folder) -> Self {
        Folder {
            id: f.name.clone(),
            name: f.display(),
            path: f.path.clone(),
            kind: f.kind.into(),
            selectable: f.selectable,
        }
    }
}

/// A row of the message list.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Header {
    pub uid: u32,
    pub from: String,
    pub subject: String,
    pub preview: String,
    /// Unix time, seconds.
    pub timestamp: i64,
    pub seen: bool,
    pub has_attachment: bool,
    /// Message-ID without brackets (empty if none); used for conversations.
    pub message_id: String,
    /// Ids this message replies to.
    pub parents: Vec<String>,
    /// Starred (IMAP \\Flagged).
    #[uniffi(default = false)]
    pub flagged: bool,
    /// Size of the whole message in bytes (0 = unknown).
    #[uniffi(default = 0)]
    pub size: u32,
    /// Sender's bare address (for the sender lists).
    #[uniffi(default = "")]
    pub from_address: String,
    /// Gmail labels (raw folder names); empty on other servers.
    #[uniffi(default = [])]
    pub labels: Vec<String>,
    /// Gmail inbox tab: 0 Primary, 1 Promotions, 2 Social, 3 Updates,
    /// 4 Forums (0 on other servers).
    #[uniffi(default = 0)]
    pub category: u8,
}

impl From<mail::Header> for Header {
    fn from(h: mail::Header) -> Self {
        Header {
            uid: h.uid,
            from: h.from,
            subject: h.subject,
            preview: h.preview,
            timestamp: h.timestamp,
            seen: h.seen,
            has_attachment: h.has_attachment,
            message_id: h.message_id,
            parents: h.parents,
            flagged: h.flagged,
            size: h.size,
            from_address: h.from_address,
            labels: h.labels,
            category: h.category.code(),
        }
    }
}

impl From<&Header> for mail::Header {
    fn from(h: &Header) -> Self {
        mail::Header {
            uid: h.uid,
            from: h.from.clone(),
            subject: h.subject.clone(),
            preview: h.preview.clone(),
            timestamp: h.timestamp,
            seen: h.seen,
            has_attachment: h.has_attachment,
            message_id: h.message_id.clone(),
            parents: h.parents.clone(),
            flagged: h.flagged,
            size: h.size,
            from_address: h.from_address.clone(),
            labels: h.labels.clone(),
            category: mail::Category::from_code(h.category),
            ..Default::default()
        }
    }
}

/// A conversation in a message list: its newest message plus its members.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Conversation {
    /// Newest message, shown as the list row.
    pub latest: Header,
    /// Members' uids, oldest first (reading order).
    pub uids: Vec<u32>,
    pub unread: u32,
}

/// Groups a message list into conversations (see `ferrowl_core::threads`).
#[uniffi::export]
pub fn group_conversations(headers: Vec<Header>) -> Vec<Conversation> {
    let core: Vec<mail::Header> = headers.iter().map(mail::Header::from).collect();
    ferrowl_core::threads::group(&core)
        .into_iter()
        .map(|t| Conversation {
            latest: headers[t.latest()].clone(),
            uids: t.members.iter().rev().map(|&i| headers[i].uid).collect(),
            unread: t.unread as u32,
        })
        .collect()
}

/// Orders of message lists ("Sort by…").
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum SortOrder {
    Newest,
    Oldest,
    Largest,
    Smallest,
    SenderAz,
    SenderZa,
    SubjectAz,
}

impl SortOrder {
    fn core(self) -> ferrowl_core::sort::SortOrder {
        use ferrowl_core::sort::SortOrder as S;
        match self {
            SortOrder::Newest => S::Newest,
            SortOrder::Oldest => S::Oldest,
            SortOrder::Largest => S::Largest,
            SortOrder::Smallest => S::Smallest,
            SortOrder::SenderAz => S::SenderAz,
            SortOrder::SenderZa => S::SenderZa,
            SortOrder::SubjectAz => S::SubjectAz,
        }
    }
}

/// Sorts list rows (conversations, or one-message ones) by their newest
/// message.
#[uniffi::export]
pub fn sort_conversations(rows: Vec<Conversation>, order: SortOrder) -> Vec<Conversation> {
    let order = order.core();
    let mut rows = rows;
    rows.sort_by(|a, b| order.compare(&mail::Header::from(&a.latest), &mail::Header::from(&b.latest)));
    rows
}

/// The label's translation key of a sort order.
#[uniffi::export]
pub fn sort_label(order: SortOrder) -> String {
    order.core().key().to_string()
}

/// The newest messages of a folder. `more` is set when older messages are
/// still being indexed: call [`Engine::index_older`] with it.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct HeaderPage {
    pub headers: Vec<Header>,
    pub more: Option<u32>,
}

/// What a new-mail notification shows, from [`Engine::peek_inbox`].
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct InboxPeek {
    /// The newest unread messages above the given UID, newest first (sender
    /// and subject, no preview).
    pub fresh: Vec<Header>,
    /// How many unread messages are above the given UID in all.
    pub fresh_count: u32,
    /// All the unread messages' UIDs above the given UID, newest first
    /// (which notifications are still about unread mail).
    pub unread_uids: Vec<u32>,
    /// The newest message of the inbox, read or not.
    pub newest: Option<Header>,
    /// The inbox's unread count (Gmail: of the Primary tab), for the icon.
    pub unread: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Attachment {
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Signature {
    Valid,
    Invalid,
    UnknownKey,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PgpInfo {
    pub encrypted: bool,
    pub signature: Option<Signature>,
    /// Why decryption failed, if it did.
    pub error: Option<String>,
}

/// An opened message.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Message {
    pub uid: u32,
    pub from: String,
    pub from_address: String,
    pub to: String,
    pub cc: String,
    pub to_addresses: Vec<String>,
    pub cc_addresses: Vec<String>,
    pub subject: String,
    pub timestamp: i64,
    pub message_id: Option<String>,
    pub references: Vec<String>,
    /// Plain text body (always present).
    pub text: String,
    /// HTML body as sent. Show it in a WebView with JavaScript off.
    pub html: Option<String>,
    /// Inline images referenced as `cid:` from the HTML, by Content-ID.
    pub inline: HashMap<String, Vec<u8>>,
    pub attachments: Vec<Attachment>,
    pub dkim_pass: bool,
    pub pgp: Option<PgpInfo>,
    /// The calendar object of an invitation (or of a cancellation or
    /// reply), to pass back to `respond_invitation`.
    pub calendar: Option<String>,
    /// What that calendar object says, for the invitation card.
    pub invitation: Option<Invitation>,
}

/// An answer to an invitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum InviteReply {
    Accepted,
    Tentative,
    Declined,
}

impl From<InviteReply> for invite::Reply {
    fn from(r: InviteReply) -> Self {
        match r {
            InviteReply::Accepted => invite::Reply::Accepted,
            InviteReply::Tentative => invite::Reply::Tentative,
            InviteReply::Declined => invite::Reply::Declined,
        }
    }
}

impl From<invite::Reply> for InviteReply {
    fn from(r: invite::Reply) -> Self {
        match r {
            invite::Reply::Accepted => InviteReply::Accepted,
            invite::Reply::Tentative => InviteReply::Tentative,
            invite::Reply::Declined => InviteReply::Declined,
        }
    }
}

/// Someone's answer, in a reply to our invitation.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct InviteAnswer {
    pub name: String,
    pub email: String,
    /// ACCEPTED, TENTATIVE, DECLINED…
    pub status: String,
}

/// A meeting invitation, cancellation or reply found in a message.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Invitation {
    /// REQUEST (invitation), CANCEL (called off), REPLY (answer to ours).
    pub method: String,
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    /// All-day: first and last day (inclusive), "YYYY-MM-DD".
    pub first_day: Option<String>,
    pub last_day: Option<String>,
    pub recurring: bool,
    pub organizer: String,
    pub organizer_name: String,
    pub answers: Vec<InviteAnswer>,
}

impl From<invite::Invitation> for Invitation {
    fn from(i: invite::Invitation) -> Self {
        Invitation {
            method: i.method,
            uid: i.uid,
            summary: i.summary,
            location: i.location,
            start: i.when.start,
            end: i.when.end,
            all_day: i.when.all_day,
            first_day: i.when.first_day.map(|d| d.to_string()),
            last_day: i.when.last_day.map(|d| d.to_string()),
            recurring: i.when.recurring,
            organizer: i.organizer,
            organizer_name: i.organizer_name,
            answers: i
                .replies
                .into_iter()
                .map(|a| InviteAnswer {
                    name: a.name,
                    email: a.email,
                    status: a.partstat,
                })
                .collect(),
        }
    }
}

impl From<mail::FullMessage> for Message {
    fn from(m: mail::FullMessage) -> Self {
        Message {
            uid: m.uid,
            from: m.from,
            from_address: m.from_address,
            to: m.to,
            cc: m.cc,
            to_addresses: m.to_addrs,
            cc_addresses: m.cc_addrs,
            subject: m.subject,
            timestamp: m.timestamp,
            message_id: m.message_id,
            references: m.references,
            text: m.body,
            html: m.html,
            inline: m.inline,
            attachments: m
                .attachments
                .into_iter()
                .map(|a| Attachment {
                    name: a.name,
                    data: a.data,
                })
                .collect(),
            invitation: m.calendar.as_deref().and_then(invite::read).map(Invitation::from),
            calendar: m.calendar,
            dkim_pass: m.dkim_pass,
            pgp: m.pgp.map(|p| PgpInfo {
                encrypted: p.encrypted,
                signature: p.signature.map(|s| match s {
                    mail::SigState::Valid => Signature::Valid,
                    mail::SigState::Invalid => Signature::Invalid,
                    mail::SigState::UnknownKey => Signature::UnknownKey,
                }),
                error: p.error,
            }),
        }
    }
}

/// A message to send.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Outgoing {
    /// Comma-separated addresses.
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub subject: String,
    /// Plain text (or Markdown, see `html`).
    pub body: String,
    /// HTML alternative, e.g. from [`markdown_to_html`].
    pub html: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    /// Paths of files to attach (inside the app's sandbox).
    pub attachments: Vec<String>,
    pub sign: bool,
    pub encrypt: bool,
}

impl From<Outgoing> for mail::Outgoing {
    fn from(o: Outgoing) -> Self {
        mail::Outgoing {
            to: o.to,
            cc: o.cc,
            bcc: o.bcc,
            subject: o.subject,
            body: o.body,
            html: o.html,
            in_reply_to: o.in_reply_to,
            references: o.references,
            attachments: o.attachments.into_iter().map(PathBuf::from).collect(),
            sign: o.sign,
            encrypt: o.encrypt,
            calendar: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Login {
    /// Needs an app-specific password.
    AppPassword,
    /// Needs "Sign in with Microsoft".
    MicrosoftSignIn,
}

/// An address suggested while typing a recipient.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct AddressSuggestion {
    pub name: String,
    pub address: String,
}

/// A contact from the account's contacts server.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Contact {
    pub name: String,
    /// Preferred address first.
    pub emails: Vec<String>,
    pub phones: Vec<String>,
    pub organization: String,
    pub note: String,
}

/// One occurrence of a calendar event.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CalendarEvent {
    pub uid: String,
    pub summary: String,
    pub location: String,
    pub description: String,
    /// Start and end, seconds since the epoch.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    /// All-day events: first and last day (inclusive), "YYYY-MM-DD".
    pub first_day: Option<String>,
    pub last_day: Option<String>,
    /// Part of a repeating series.
    pub recurring: bool,
    pub organizer: String,
    /// Name and color ("#RRGGBB[AA]", may be empty) of its calendar.
    pub calendar: String,
    pub color: String,
    /// The stored event (to edit or delete it) and its calendar's URL.
    pub url: String,
    pub calendar_url: String,
}

/// How often an event repeats (the choices of the event form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Repeat {
    Never,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    /// A rule the form cannot show: kept as it is.
    Custom,
}

/// The event form. Times are wall-clock times on the phone,
/// "YYYY-MM-DDTHH:MM"; for all-day events only the dates count and `end`
/// is the last day.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct EventDraft {
    pub summary: String,
    pub location: String,
    pub description: String,
    pub all_day: bool,
    pub start: String,
    pub end: String,
    pub repeat: Repeat,
}

/// A calendar of the account.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CalendarInfo {
    pub url: String,
    pub name: String,
    pub color: String,
}

impl From<events::Draft> for EventDraft {
    fn from(d: events::Draft) -> Self {
        EventDraft {
            summary: d.summary,
            location: d.location,
            description: d.description,
            all_day: d.all_day,
            start: d.start.format("%Y-%m-%dT%H:%M").to_string(),
            end: d.end.format("%Y-%m-%dT%H:%M").to_string(),
            repeat: match d.repeat {
                events::Repeat::Never => Repeat::Never,
                events::Repeat::Daily => Repeat::Daily,
                events::Repeat::Weekly => Repeat::Weekly,
                events::Repeat::Monthly => Repeat::Monthly,
                events::Repeat::Yearly => Repeat::Yearly,
                events::Repeat::Custom => Repeat::Custom,
            },
        }
    }
}

impl EventDraft {
    fn into_core(self) -> Option<events::Draft> {
        let at = |s: &str| chrono::NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%dT%H:%M").ok();
        Some(events::Draft {
            summary: self.summary,
            location: self.location,
            description: self.description,
            all_day: self.all_day,
            start: at(&self.start)?,
            end: at(&self.end)?,
            repeat: match self.repeat {
                Repeat::Never => events::Repeat::Never,
                Repeat::Daily => events::Repeat::Daily,
                Repeat::Weekly => events::Repeat::Weekly,
                Repeat::Monthly => events::Repeat::Monthly,
                Repeat::Yearly => events::Repeat::Yearly,
                Repeat::Custom => events::Repeat::Custom,
            },
        })
    }
}

/// A one-hour event on `day` ("YYYY-MM-DD"), the form's starting point.
#[uniffi::export]
pub fn new_event_draft(day: String) -> EventDraft {
    let day =
        chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d").unwrap_or_else(|_| ferrowl_core::ical::today());
    events::Draft::new_on(day).into()
}

/// A known mail provider, for the "add account" screen.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Provider {
    pub name: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub login: Login,
    /// Setup guide on ferrowl.com, in the engine's language.
    pub guide_url: Option<String>,
}

// ---------------------------------------------------------------- free functions

/// Providers shown when adding an account, in display order.
#[uniffi::export]
pub fn known_providers() -> Vec<Provider> {
    let lang = i18n::current().code;
    providers::PROVIDERS
        .iter()
        .map(|p| Provider {
            name: p.name.into(),
            imap_host: p.imap.0.into(),
            imap_port: p.imap.1,
            smtp_host: p.smtp.0.into(),
            smtp_port: p.smtp.1,
            login: match p.login {
                providers::Login::AppPassword => Login::AppPassword,
                providers::Login::MicrosoftSignIn => Login::MicrosoftSignIn,
            },
            guide_url: p.guide.map(|g| providers::guide_url(g, lang)),
        })
        .collect()
}

/// Index into [`known_providers`] of the provider an address belongs to.
#[uniffi::export]
pub fn provider_for_email(email: String) -> Option<u32> {
    providers::for_email(&email).map(|i| i as u32)
}

// ---------------------------------------------------------------- sign-in

/// Secure storage the app offers for sign-in (refresh) tokens: Android
/// Keystore, iOS Keychain. Called from background threads.
#[uniffi::export(with_foreign)]
pub trait TokenVault: Send + Sync {
    fn get(&self, key: String) -> Option<String>;
    /// True when saved.
    fn set(&self, key: String, secret: String) -> bool;
    fn delete(&self, key: String);
}

struct Vault(Arc<dyn TokenVault>);

impl ferrowl_core::secrets::Store for Vault {
    fn get(&self, user: &str) -> Option<String> {
        self.0.get(user.to_string())
    }
    fn set(&self, user: &str, secret: &str) -> bool {
        self.0.set(user.to_string(), secret.to_string())
    }
    fn delete(&self, user: &str) {
        self.0.delete(user.to_string())
    }
}

/// Hands token storage to the engine. Call once at start-up, before any
/// account that signs in with Microsoft/Google connects.
#[uniffi::export]
pub fn set_token_vault(vault: Arc<dyn TokenVault>) {
    oauth::set_vault(Box::new(Vault(vault)));
}

/// Access tokens the app obtains itself (Android: Google accounts through
/// Google Identity Services). Called from background threads; may block.
#[uniffi::export(with_foreign)]
pub trait AccessTokenSource: Send + Sync {
    /// A valid token, or `None` when this source does not handle the
    /// provider or cannot get one without the user.
    fn access_token(&self, provider: OAuthProvider, email: String) -> Option<String>;
}

/// Hands token retrieval to the app. Call once at start-up.
#[uniffi::export]
pub fn set_access_token_source(source: Arc<dyn AccessTokenSource>) {
    oauth::set_token_source(Box::new(move |provider, email| {
        source.access_token(provider.into(), email.to_string())
    }));
}

/// Where Google sends the iOS app back after signing in (pass it to
/// [oauth_begin]).
#[uniffi::export]
pub fn google_ios_redirect() -> String {
    oauth::GOOGLE_IOS_REDIRECT.to_string()
}

/// Whether this build can sign in with `provider` (client registered).
#[uniffi::export]
pub fn oauth_available(provider: OAuthProvider) -> bool {
    oauth::Provider::from(provider).available()
}

/// The provider whose sign-in an address needs, if any.
#[uniffi::export]
pub fn oauth_provider_for_email(email: String) -> Option<OAuthProvider> {
    oauth::Provider::for_email(&email).map(Into::into)
}

/// A sign-in in progress. The app opens `url`, keeps the rest (it can be
/// stopped by the system meanwhile) and passes it back to [`oauth_finish`].
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct OAuthRequest {
    pub url: String,
    pub state: String,
    pub verifier: String,
    pub redirect: String,
}

impl From<OAuthRequest> for oauth::Pending {
    fn from(r: OAuthRequest) -> Self {
        oauth::Pending {
            url: r.url,
            state: r.state,
            verifier: r.verifier,
            redirect: r.redirect,
        }
    }
}

/// Starts a sign-in whose answer comes back to `redirect` (on Android the
/// app's `msauth://` address, which reopens the app).
#[uniffi::export]
pub fn oauth_begin(provider: OAuthProvider, login_hint: String, redirect: String) -> Result<OAuthRequest> {
    let p = oauth::begin(provider.into(), &login_hint, &redirect)
        .map_err(|e| FerrowlError::Failed(ferrowl_core::errors::humanize(&format!("{e:#}"))))?;
    Ok(OAuthRequest {
        url: p.url,
        state: p.state,
        verifier: p.verifier,
        redirect: p.redirect,
    })
}

/// Finishes a sign-in from the address the provider sent the browser back
/// to: checks it, exchanges the code, stores the tokens in the
/// [`TokenVault`] and returns the signed-in address.
#[uniffi::export(async_runtime = "tokio")]
pub async fn oauth_finish(
    provider: OAuthProvider,
    request: OAuthRequest,
    answer: String,
    login_hint: String,
) -> Result<String> {
    let provider: oauth::Provider = provider.into();
    let pending: oauth::Pending = request.into();
    tokio::task::spawn_blocking(move || {
        let code = oauth::code_from_redirect(&pending, &answer)?;
        oauth::finish(provider, &pending, &code, &login_hint)
    })
    .await
    .map_err(|e| FerrowlError::Failed(e.to_string()))?
    .map_err(|e| FerrowlError::Failed(ferrowl_core::errors::humanize(&format!("{e:#}"))))
}

/// A complete account for an address signed in with `provider` (servers
/// filled in; no password).
#[uniffi::export]
pub fn oauth_account(provider: OAuthProvider, email: String, display_name: String) -> Account {
    let ((imap_host, imap_port), (smtp_host, smtp_port)) = oauth::Provider::from(provider).servers(&email);
    Account {
        display_name,
        email: email.clone(),
        username: email,
        password: String::new(),
        incoming_host: imap_host.into(),
        incoming_port: imap_port,
        smtp_host: smtp_host.into(),
        smtp_port,
        protocol: Protocol::Imap,
        pop3_keep_days: None,
        pgp_passphrase: String::new(),
        carddav_url: String::new(),
        caldav_url: String::new(),
        oauth: Some(provider),
    }
}

/// Forgets the sign-in of an account (when it is removed).
#[uniffi::export]
pub fn oauth_forget(provider: OAuthProvider, email: String) {
    oauth::forget(provider.into(), &email);
}

/// HTML to send alongside the text when the user wrote Markdown formatting
/// (`None` for plain text, which is then sent as text only).
/// Initials for an avatar: "Giulia Ferri" -> "GF", "anna@x.it" -> "A".
#[uniffi::export]
pub fn initials(name: String) -> String {
    ferrowl_core::initials(&name)
}

/// The account as the push server stores it (the engine's settings
/// format, JSON). Sent over HTTPS to push.ferrowl.com, which seals it with
/// the phone's key.
#[uniffi::export]
pub fn push_account_json(account: Account) -> String {
    let account: config::Account = account.into();
    serde_json::to_string(&account).unwrap_or_default()
}

/// The sign-in to send the push server with an account that signs in with
/// Microsoft/Google (JSON: refresh token and client), so it can watch it
/// without a password. `None` for password accounts, or when this device
/// has no refresh token (Android Google accounts: Google Identity Services).
#[uniffi::export]
pub fn push_oauth_json(account: Account) -> Option<String> {
    let provider = account.oauth?;
    let grant = oauth::grant(provider.into(), &account.email)?;
    serde_json::to_string(&grant).ok()
}

#[uniffi::export]
pub fn markdown_to_html(markdown: String) -> Option<String> {
    ferrowl_core::markdown::has_formatting(&markdown)
        .then(|| ferrowl_core::markdown::to_html_document(&markdown))
}

// MARK: Ferrowl Bridge (other apps, Shortcuts/Siri, intents, links)

/// What a `ferrowl://` or `mailto:` link asks for (see `ferrowl_core::bridge`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum FerrowlLink {
    /// Open a message of an account (the inbox unless `folder` says otherwise).
    Open {
        account: String,
        folder: String,
        uid: u32,
    },
    /// Show an account's inbox (the current account when `None`).
    Inbox { account: Option<String> },
    /// Write a message, shown to the user before anything is sent.
    Compose {
        to: String,
        cc: String,
        bcc: String,
        subject: String,
        body: String,
    },
}

impl From<bridge::Link> for FerrowlLink {
    fn from(l: bridge::Link) -> Self {
        match l {
            bridge::Link::Open { account, folder, uid } => FerrowlLink::Open { account, folder, uid },
            bridge::Link::Inbox { account } => FerrowlLink::Inbox { account },
            bridge::Link::Compose {
                to,
                cc,
                bcc,
                subject,
                body,
            } => FerrowlLink::Compose {
                to,
                cc,
                bcc,
                subject,
                body,
            },
        }
    }
}

/// Reads a `ferrowl://` or `mailto:` link (`None`: not one, or incomplete).
#[uniffi::export]
pub fn parse_link(url: String) -> Option<FerrowlLink> {
    bridge::parse_link(&url).map(FerrowlLink::from)
}

/// The `ferrowl://open` link of a message, for other apps to keep.
#[uniffi::export]
pub fn message_link(account: String, folder: String, uid: u32) -> String {
    bridge::open_link(&account, &folder, uid)
}

/// What another app or a shortcut may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Capability {
    Read,
    Write,
    Send,
    Contacts,
    Calendar,
    Open,
}

impl From<Capability> for bridge::Capability {
    fn from(c: Capability) -> Self {
        match c {
            Capability::Read => bridge::Capability::Read,
            Capability::Write => bridge::Capability::Write,
            Capability::Send => bridge::Capability::Send,
            Capability::Contacts => bridge::Capability::Contacts,
            Capability::Calendar => bridge::Capability::Calendar,
            Capability::Open => bridge::Capability::Open,
        }
    }
}

/// Whether other apps need Ferrowl Pro for this.
#[uniffi::export]
pub fn capability_needs_pro(capability: Capability) -> bool {
    bridge::needs_pro(capability.into())
}

/// Whether the user confirms this each time (sending, changing mail).
#[uniffi::export]
pub fn capability_needs_confirmation(capability: Capability) -> bool {
    bridge::needs_confirmation(capability.into())
}

/// Translated UI string from `core/locales`, for texts shared with desktop.
#[uniffi::export]
pub fn translate(key: String) -> String {
    i18n::t(&key).to_string()
}

// ---------------------------------------------------------------- drafts

/// Full date in the engine's language, e.g. "2 novembre 2026, 09:42".
fn full_date(ts: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(d) = Local.timestamp_opt(ts, 0).single() else {
        return String::new();
    };
    i18n::tf(
        "date_other",
        &[
            ("day", &d.day().to_string()),
            ("month", i18n::t(&format!("month_{}", d.month()))),
            ("year", &d.year().to_string()),
            ("time", &d.format("%H:%M").to_string()),
        ],
    )
}

fn blank_draft() -> Outgoing {
    Outgoing {
        to: String::new(),
        cc: String::new(),
        bcc: String::new(),
        subject: String::new(),
        body: String::new(),
        html: None,
        in_reply_to: None,
        references: Vec::new(),
        attachments: Vec::new(),
        sign: false,
        encrypt: false,
    }
}

/// A reply, as the desktop writes it: "Re:" subject, quoted text with a
/// translated "On <date>, <name> wrote:" line, threading headers.
/// `reply_all` adds the other recipients (never `own_address`).
#[uniffi::export]
pub fn reply_draft(message: Message, reply_all: bool, own_address: String) -> Outgoing {
    let own = own_address.trim().to_lowercase();
    let not_me = |a: &&String| a.trim().to_lowercase() != own;
    let mut to = vec![message.from_address.clone()];
    let mut cc = Vec::new();
    if reply_all {
        to.extend(message.to_addresses.iter().filter(not_me).cloned());
        cc.extend(message.cc_addresses.iter().filter(not_me).cloned());
    }
    to.dedup();
    let subject = if message.subject.to_lowercase().starts_with("re:") {
        message.subject.clone()
    } else {
        format!("Re: {}", message.subject)
    };
    let quoted: String = message.text.lines().map(|l| format!("> {l}\n")).collect();
    let header = i18n::tf(
        "quote_header",
        &[("date", &full_date(message.timestamp)), ("name", &message.from)],
    );
    // The engine adds the replied-to id to References when sending.
    let references = message.references.clone();
    Outgoing {
        to: to.join(", "),
        cc: cc.join(", "),
        subject,
        body: format!("\n\n{header}\n{quoted}"),
        in_reply_to: message.message_id.clone(),
        references,
        encrypt: message.pgp.as_ref().is_some_and(|p| p.encrypted),
        ..blank_draft()
    }
}

/// A forward: "Fwd:" subject and the original headers and text. Attachments
/// are not carried over yet.
#[uniffi::export]
pub fn forward_draft(message: Message) -> Outgoing {
    let body = format!(
        "\n\n{}\n{}: {} <{}>\n{}: {}\n{}: {}\n{}: {}\n\n{}",
        i18n::t("forward_header"),
        i18n::t("from"),
        message.from,
        message.from_address,
        i18n::t("date"),
        full_date(message.timestamp),
        i18n::t("subject"),
        message.subject,
        i18n::t("to"),
        message.to,
        message.text
    );
    Outgoing {
        subject: format!("Fwd: {}", message.subject),
        body,
        ..blank_draft()
    }
}

// ---------------------------------------------------------------- engine

/// The engine: one per app process.
#[derive(uniffi::Object)]
pub struct Engine {
    backend: Backend,
    account: Mutex<Option<config::Account>>,
    folders: Mutex<Vec<mail::Folder>>,
}

#[uniffi::export(async_runtime = "tokio")]
impl Engine {
    /// `data_dir`: app-private directory for the cache, the POP3 mailbox and
    /// OpenPGP keys. `language`: a code such as "it", or "auto".
    #[uniffi::constructor]
    pub fn new(data_dir: String, language: String) -> Arc<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let data = PathBuf::from(data_dir);
        paths::set(data.clone(), data.join("config"));
        i18n::set(Some(&language));
        Arc::new(Engine {
            backend: Backend::default(),
            account: Mutex::new(None),
            folders: Mutex::new(Vec::new()),
        })
    }

    /// Checks the settings without changing the connected account.
    /// Returns a short confirmation to show.
    pub async fn test(&self, account: Account) -> Result<String> {
        Ok(Backend::test(account.into()).await?)
    }

    /// Connects an account (replacing the previous one) and lists its folders.
    pub async fn connect(&self, account: Account) -> Result<Vec<Folder>> {
        let account: config::Account = account.into();
        let folders = self.backend.connect(account.clone()).await?;
        let out = folders.iter().map(Folder::from).collect();
        *lock(&self.account) = Some(account);
        *lock(&self.folders) = folders;
        Ok(out)
    }

    /// Deletes what the device keeps for a removed account (cached mail,
    /// search index, contacts, calendars, sender rules).
    pub fn forget_account(&self, account: Account) {
        let account: config::Account = account.into();
        ferrowl_core::cache::forget_account(&account);
    }

    /// Messages already stored on the device, instantly (before `headers`).
    /// The inbox as saved on the device for `account`, before connecting:
    /// the app shows it at once while the connection is made.
    pub fn cached_inbox(&self, account: Account) -> Vec<Header> {
        let account: config::Account = account.into();
        Backend::cached_headers(&account, "INBOX")
            .into_iter()
            .map(Header::from)
            .collect()
    }

    pub fn cached_headers(&self, folder_id: String) -> Vec<Header> {
        match lock(&self.account).as_ref() {
            Some(a) => Backend::cached_headers(a, &folder_id)
                .into_iter()
                .map(Header::from)
                .collect(),
            None => Vec::new(),
        }
    }

    /// Newest messages from the server (POP3: downloads new mail first).
    pub async fn headers(&self, folder_id: String) -> Result<HeaderPage> {
        let (headers, first_seq) = self.backend.headers(folder_id).await?;
        Ok(HeaderPage {
            headers: headers.into_iter().map(Header::from).collect(),
            more: (first_seq > 1).then_some(first_seq),
        })
    }

    /// For new-mail notifications, which have little time and memory: the
    /// unread messages of `account`'s inbox above UID `after` (at most
    /// `limit`), its newest message and its unread count, on a connection
    /// of its own (the connected account and the cache are left alone).
    /// Fails for POP3 accounts.
    pub async fn peek_inbox(&self, account: Account, after: u32, limit: u32) -> Result<InboxPeek> {
        let p = Backend::peek(account.into(), after, limit).await?;
        Ok(InboxPeek {
            fresh: p.fresh.into_iter().map(Header::from).collect(),
            fresh_count: p.fresh_count,
            unread_uids: p.unread_uids,
            newest: p.newest.map(Header::from),
            unread: p.unread,
        })
    }

    /// Indexes older messages for search, in the background. Returns the
    /// value for the next call, or `None` when done.
    pub async fn index_older(&self, folder_id: String, more: u32) -> Result<Option<u32>> {
        Ok(self.backend.index_older(folder_id, more, 150).await?)
    }

    /// Search in the messages stored on the device: free words and the
    /// operators `from:`, `has:attachment`, `is:unread`, `is:read`,
    /// `is:starred` (see `ferrowl_core::query`).
    pub fn search(&self, folder_id: String, query: String) -> Vec<Header> {
        match lock(&self.account).as_ref() {
            Some(a) => Backend::search_query(a, &folder_id, &query)
                .into_iter()
                .map(Header::from)
                .collect(),
            None => Vec::new(),
        }
    }

    /// Search in what is stored on the device for any account (not only
    /// the connected one; nothing is fetched): newest first, at most `limit`.
    /// For Shortcuts/Siri and other apps.
    pub fn search_account(
        &self,
        account: Account,
        folder_id: String,
        query: String,
        limit: u32,
    ) -> Vec<Header> {
        let account: config::Account = account.into();
        bridge::search(&account, &folder_id, &query, limit as usize)
            .into_iter()
            .map(Header::from)
            .collect()
    }

    /// Recipients matching what is being typed, from the account's mail
    /// (people we write to first).
    pub fn suggest_addresses(&self, query: String) -> Vec<AddressSuggestion> {
        match lock(&self.account).as_ref() {
            Some(a) => Backend::suggest(a, &query)
                .into_iter()
                .map(|(name, address)| AddressSuggestion { name, address })
                .collect(),
            None => Vec::new(),
        }
    }

    /// Saved contacts whose name, address, organization or phone contains
    /// `query` (all when empty), by name. Reads the local copy only.
    pub fn contacts(&self, query: String) -> Vec<Contact> {
        match lock(&self.account).as_ref() {
            Some(a) => contacts_of(a, &query),
            None => Vec::new(),
        }
    }

    /// Brings the local copy of the contacts up to date with the account's
    /// CardDAV server. Returns how many contacts are saved.
    pub async fn sync_contacts(&self) -> Result<u32> {
        // No account yet (the app connects first): nothing to sync.
        let Some(account) = lock(&self.account).clone() else {
            return Ok(0);
        };
        Ok(Backend::sync_contacts(account).await? as u32)
    }

    /// Event occurrences overlapping `[from, to)` (seconds since the
    /// epoch), sorted by start. Reads the local copy only.
    pub fn agenda(&self, from: i64, to: i64) -> Vec<CalendarEvent> {
        let Some(a) = lock(&self.account).clone() else {
            return Vec::new();
        };
        agenda_of(&a, from, to)
    }

    /// The agenda of any account on the device (not only the connected
    /// one), from what was synced: for Shortcuts/Siri and other apps.
    pub fn agenda_for(&self, account: Account, from: i64, to: i64) -> Vec<CalendarEvent> {
        agenda_of(&account.into(), from, to)
    }

    /// The address book of any account on the device (see `agenda_for`).
    pub fn contacts_for(&self, account: Account, query: String) -> Vec<Contact> {
        contacts_of(&account.into(), &query)
    }

    /// The calendars of any account on the device (see `agenda_for`).
    pub fn calendars_for(&self, account: Account) -> Vec<CalendarInfo> {
        Backend::calendars(&account.into())
            .into_iter()
            .map(|c| CalendarInfo {
                url: c.url,
                name: c.name,
                color: c.color,
            })
            .collect()
    }

    /// Adds an event to a calendar of any account (see `agenda_for`).
    pub async fn save_event_for(
        &self,
        account: Account,
        calendar_url: String,
        draft: EventDraft,
    ) -> Result<String> {
        let draft = draft
            .into_core()
            .ok_or_else(|| FerrowlError::from(i18n::t("event_bad_time").to_string()))?;
        Ok(Backend::save_event(account.into(), calendar_url, None, draft).await?)
    }
}

fn agenda_of(a: &config::Account, from: i64, to: i64) -> Vec<CalendarEvent> {
    Backend::agenda(a, from, to)
        .into_iter()
        .map(|e| {
            let o = e.occurrence;
            CalendarEvent {
                uid: o.uid,
                summary: o.summary,
                location: o.location,
                description: o.description,
                start: o.start,
                end: o.end,
                all_day: o.all_day,
                first_day: o.first_day.map(|d| d.to_string()),
                last_day: o.last_day.map(|d| d.to_string()),
                recurring: o.recurring,
                organizer: o.organizer,
                calendar: e.calendar,
                color: e.color,
                url: e.url,
                calendar_url: e.calendar_url,
            }
        })
        .collect()
}

fn contacts_of(a: &config::Account, query: &str) -> Vec<Contact> {
    Backend::contacts(a, query)
        .into_iter()
        .map(|c| Contact {
            name: c.name,
            emails: c.emails,
            phones: c.phones,
            organization: c.organization,
            note: c.note,
        })
        .collect()
}

#[uniffi::export(async_runtime = "tokio")]
impl Engine {
    /// The account's calendars.
    pub fn calendars(&self) -> Vec<CalendarInfo> {
        let Some(a) = lock(&self.account).clone() else {
            return Vec::new();
        };
        Backend::calendars(&a)
            .into_iter()
            .map(|c| CalendarInfo {
                url: c.url,
                name: c.name,
                color: c.color,
            })
            .collect()
    }

    /// The form for a stored event (by its `url`).
    pub fn event_draft(&self, url: String) -> Option<EventDraft> {
        let a = lock(&self.account).clone()?;
        Backend::event_draft(&a, &url).map(EventDraft::from)
    }

    /// Saves an event: changes the stored one at `url`, or creates a new
    /// one in `calendar_url` (empty = the first calendar). Returns the text
    /// to show.
    pub async fn save_event(
        &self,
        calendar_url: String,
        url: Option<String>,
        draft: EventDraft,
    ) -> Result<String> {
        let Some(account) = lock(&self.account).clone() else {
            return Err(i18n::t("err_no_account").to_string().into());
        };
        let draft = draft
            .into_core()
            .ok_or_else(|| FerrowlError::from(i18n::t("event_bad_time").to_string()))?;
        Ok(Backend::save_event(account, calendar_url, url, draft).await?)
    }

    /// Deletes a stored event (a repeating one with all its occurrences).
    pub async fn delete_event(&self, url: String) -> Result<String> {
        let Some(account) = lock(&self.account).clone() else {
            return Err(i18n::t("err_no_account").to_string().into());
        };
        Ok(Backend::delete_event(account, url).await?)
    }

    /// Brings the local copy of the calendars up to date with the
    /// account's CalDAV server. Returns how many calendars there are.
    pub async fn sync_calendar(&self) -> Result<u32> {
        // No account yet (the app connects first): nothing to sync.
        let Some(account) = lock(&self.account).clone() else {
            return Ok(0);
        };
        Ok(Backend::sync_calendar(account).await? as u32)
    }

    /// Unread counts per folder id.
    pub async fn unread(&self, folder_ids: Vec<String>) -> Result<HashMap<String, u32>> {
        Ok(self.backend.unseen(folder_ids).await?)
    }

    /// Opens a message (marks it as read).
    pub async fn message(&self, folder_id: String, uid: u32) -> Result<Message> {
        Ok(self.backend.message(folder_id, uid).await?.into())
    }

    pub async fn set_seen(&self, folder_id: String, uid: u32, seen: bool) -> Result<()> {
        Ok(self.backend.set_seen(folder_id, uid, seen).await?)
    }

    pub async fn move_to(&self, folder_id: String, uid: u32, target_id: String) -> Result<()> {
        Ok(self.backend.move_to(folder_id, uid, target_id).await?)
    }

    /// Moves to the Archive folder (created if missing). Returns its id.
    pub async fn archive(&self, folder_id: String, uid: u32) -> Result<String> {
        let folders = lock(&self.folders).clone();
        Ok(self.backend.archive(folder_id, uid, folders).await?)
    }

    pub async fn delete_forever(&self, folder_id: String, uid: u32) -> Result<()> {
        Ok(self.backend.delete_forever(folder_id, uid).await?)
    }

    /// Marks several messages read or unread with one server command.
    /// Stars or unstars messages (IMAP \\Flagged).
    pub async fn set_flagged_many(&self, folder_id: String, uids: Vec<u32>, flagged: bool) -> Result<()> {
        Ok(self.backend.set_flagged_many(folder_id, uids, flagged).await?)
    }

    pub async fn set_seen_many(&self, folder_id: String, uids: Vec<u32>, seen: bool) -> Result<()> {
        Ok(self.backend.set_seen_many(folder_id, uids, seen).await?)
    }

    pub async fn move_many(&self, folder_id: String, uids: Vec<u32>, target_id: String) -> Result<()> {
        Ok(self.backend.move_many(folder_id, uids, target_id).await?)
    }

    /// Moves several messages to the Archive (created if missing).
    pub async fn archive_many(&self, folder_id: String, uids: Vec<u32>) -> Result<String> {
        let folders = lock(&self.folders).clone();
        Ok(self.backend.archive_many(folder_id, uids, folders).await?)
    }

    /// Whether the account is Gmail (messages have labels).
    pub fn is_gmail(&self) -> bool {
        self.backend.is_gmail()
    }

    /// Adds or removes a Gmail label (a folder's raw name) on messages.
    pub async fn set_label_many(
        &self,
        folder_id: String,
        uids: Vec<u32>,
        label: String,
        on: bool,
    ) -> Result<()> {
        Ok(self.backend.set_label_many(folder_id, uids, label, on).await?)
    }

    /// Moves messages to Spam (marked $Junk for the server's filter).
    pub async fn report_spam(&self, folder_id: String, uids: Vec<u32>) -> Result<String> {
        let folders = lock(&self.folders).clone();
        Ok(self.backend.report_spam(folder_id, uids, folders).await?)
    }

    /// Moves messages from Spam back to the inbox (marked $NotJunk).
    pub async fn not_spam(&self, folder_id: String, uids: Vec<u32>) -> Result<String> {
        let folders = lock(&self.folders).clone();
        Ok(self.backend.not_spam(folder_id, uids, folders).await?)
    }

    /// Blocks (`allow` false) or allows a sender: an address or "@domain".
    pub fn set_sender_rule(&self, address: String, allow: bool) -> Result<()> {
        let Some(a) = lock(&self.account).clone() else {
            return Ok(());
        };
        Ok(Backend::set_sender_rule(&a, &address, allow)?)
    }

    pub fn remove_sender_rule(&self, address: String) {
        if let Some(a) = lock(&self.account).clone() {
            Backend::remove_sender_rule(&a, &address);
        }
    }

    /// The blocked (`allow` false) or allowed senders.
    pub fn sender_rules(&self, allow: bool) -> Vec<String> {
        lock(&self.account)
            .clone()
            .map(|a| Backend::sender_rules(&a, allow))
            .unwrap_or_default()
    }

    /// Applies the sender lists to a loaded folder (inbox: blocked senders
    /// to Spam; Spam: allowed senders back). Returns the moved uids.
    pub async fn apply_sender_rules(&self, folder_id: String, headers: Vec<Header>) -> Result<Vec<u32>> {
        let folders = lock(&self.folders).clone();
        let core: Vec<mail::Header> = headers.iter().map(mail::Header::from).collect();
        Ok(self.backend.apply_sender_rules(folder_id, core, folders).await?)
    }

    pub async fn delete_forever_many(&self, folder_id: String, uids: Vec<u32>) -> Result<()> {
        Ok(self.backend.delete_forever_many(folder_id, uids).await?)
    }

    /// Sends and stores a copy in Sent. Returns a warning if the message
    /// went out but the copy could not be saved.
    /// Answers an invitation (`calendar` from the message): mails the reply
    /// to the organizer and updates the calendar if the account has one.
    /// Returns the text to show.
    pub async fn respond_invitation(&self, calendar: String, reply: InviteReply) -> Result<String> {
        let sent = lock(&self.folders)
            .iter()
            .find(|f| f.kind == mail::FolderKind::Sent)
            .map(|f| f.name.clone());
        Ok(self
            .backend
            .respond_invitation(calendar, reply.into(), sent)
            .await?)
    }

    /// Our earlier answer to the invitation with this event UID, if any.
    pub fn invitation_reply(&self, uid: String) -> Option<InviteReply> {
        let a = lock(&self.account).clone()?;
        Backend::invitation_reply(&a, &uid).map(InviteReply::from)
    }

    /// Whether the event is in the calendar (to offer removing it when
    /// cancelled).
    pub fn in_calendar(&self, uid: String) -> bool {
        lock(&self.account)
            .as_ref()
            .is_some_and(|a| Backend::in_calendar(a, &uid))
    }

    /// Removes a cancelled event from the calendar. Returns the text to show.
    pub async fn remove_cancelled(&self, uid: String) -> Result<String> {
        let Some(account) = lock(&self.account).clone() else {
            return Ok(String::new());
        };
        Ok(Backend::remove_cancelled(account, uid).await?)
    }

    pub async fn send(&self, message: Outgoing) -> Result<Option<String>> {
        let sent = lock(&self.folders)
            .iter()
            .find(|f| f.kind == mail::FolderKind::Sent)
            .map(|f| f.name.clone());
        Ok(self.backend.send(message.into(), sent).await?)
    }
}

/// Poisoned locks only mean another call panicked; the data is still usable.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_and_detection() {
        let list = known_providers();
        assert!(list.iter().any(|p| p.name == "Google"));
        let i = provider_for_email("someone@gmail.com".into()).expect("gmail") as usize;
        assert_eq!(list[i].imap_host, "imap.gmail.com");
    }

    fn sample() -> Message {
        Message {
            uid: 1,
            from: "Ada".into(),
            from_address: "ada@example.org".into(),
            to: "me, Bob".into(),
            cc: String::new(),
            to_addresses: vec!["me@example.org".into(), "bob@example.org".into()],
            cc_addresses: vec!["carl@example.org".into()],
            subject: "Engines".into(),
            timestamp: 1_790_000_000,
            message_id: Some("<1@x>".into()),
            references: vec!["<0@x>".into()],
            text: "line 1\nline 2".into(),
            html: None,
            inline: HashMap::new(),
            attachments: Vec::new(),
            dkim_pass: false,
            pgp: None,
            calendar: None,
            invitation: None,
        }
    }

    #[test]
    fn conversations_group_replies() {
        let h = |uid: u32, id: &str, parent: &[&str]| Header {
            uid,
            from: String::new(),
            subject: String::new(),
            preview: String::new(),
            timestamp: uid as i64,
            seen: uid != 2,
            has_attachment: false,
            message_id: id.into(),
            parents: parent.iter().map(|p| p.to_string()).collect(),
            flagged: false,
            size: 0,
            from_address: String::new(),
            labels: Vec::new(),
            category: 0,
        };
        let c = group_conversations(vec![h(1, "a", &[]), h(2, "b", &["a"]), h(3, "x", &[])]);
        assert_eq!(c.len(), 2);
        assert_eq!(c[1].uids, vec![1, 2]);
        assert_eq!(c[1].latest.uid, 2);
        assert_eq!(c[1].unread, 1);
    }

    #[test]
    fn reply_and_forward_drafts() {
        let r = reply_draft(sample(), false, "me@example.org".into());
        assert_eq!(r.to, "ada@example.org");
        assert_eq!(r.subject, "Re: Engines");
        assert!(r.body.contains("> line 1\n> line 2"));
        assert_eq!(r.in_reply_to.as_deref(), Some("<1@x>"));
        assert_eq!(r.references, vec!["<0@x>".to_string()]);

        let all = reply_draft(sample(), true, "ME@example.org".into());
        assert_eq!(all.to, "ada@example.org, bob@example.org");
        assert_eq!(all.cc, "carl@example.org");

        let f = forward_draft(sample());
        assert_eq!(f.subject, "Fwd: Engines");
        assert!(f.to.is_empty() && f.body.contains("line 2"));
    }

    #[test]
    fn push_json_uses_the_engine_format() {
        let json = push_account_json(Account {
            display_name: "Ada".into(),
            email: "ada@example.org".into(),
            username: "ada".into(),
            password: "pw".into(),
            incoming_host: "imap.example.org".into(),
            incoming_port: 993,
            smtp_host: "smtp.example.org".into(),
            smtp_port: 465,
            protocol: Protocol::Imap,
            pop3_keep_days: None,
            pgp_passphrase: String::new(),
            carddav_url: String::new(),
            caldav_url: String::new(),
            oauth: None,
        });
        let back: config::Account = serde_json::from_str(&json).unwrap();
        assert_eq!(back.imap_host, "imap.example.org");
        assert_eq!(back.password, "pw");
    }

    #[test]
    fn account_round_trip() {
        let a = Account {
            display_name: "Ada".into(),
            email: "ada@example.org".into(),
            username: "ada".into(),
            password: "pw".into(),
            incoming_host: "pop.example.org".into(),
            incoming_port: 995,
            smtp_host: "smtp.example.org".into(),
            smtp_port: 465,
            protocol: Protocol::Pop3,
            pop3_keep_days: Some(7),
            pgp_passphrase: String::new(),
            carddav_url: String::new(),
            caldav_url: String::new(),
            oauth: None,
        };
        let core: config::Account = a.into();
        assert!(core.is_pop3());
        assert_eq!(core.imap_host, "pop.example.org");
        assert_eq!(core.pop3_keep_days, Some(7));
    }
}
