//! Application state and logic (Elm architecture: update / view / subscription).

use crate::backend::{Backend, Res};
use crate::config::{Account, Config, Density};
use crate::i18n::{self, t, tf};
use crate::mail::{Folder, FolderKind, FullMessage, Header, Outgoing};
use crate::theme::{self, Palette};
use crate::ui;
use iced::keyboard::{self, Key, key::Named};
use iced::widget::{operation, text_editor};
use iced::{Element, Subscription, Task, Theme};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const SEARCH_ID: &str = "search";
/// Background indexing: wait before the first chunk and between chunks.
const INDEX_FIRST_PAUSE: Duration = Duration::from_secs(3);
const INDEX_PAUSE: Duration = Duration::from_millis(1500);
/// Scrollable of the reading pane.
pub const READER_ID: &str = "reader";
/// Scrollable of the message list.
pub const LIST_ID: &str = "message-list";
const AUTO_REFRESH: Duration = Duration::from_secs(120);

// ---------------------------------------------------------------- state

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Mail,
    Settings(Section),
}

/// The invitation in the open message, with our answer so far.
#[derive(Debug, Clone)]
pub struct InviteCard {
    /// The message it belongs to.
    pub uid: u32,
    pub ics: String,
    pub invitation: ferrowl_core::invite::Invitation,
    pub reply: Option<ferrowl_core::invite::Reply>,
    /// Cancelled event still in our calendar (can be removed).
    pub in_calendar: bool,
    pub busy: bool,
}

/// The event form (new event, or changes to a stored one).
pub struct EventForm {
    /// The stored event being changed (`None` = new).
    pub url: Option<String>,
    /// Calendar for a new event (its URL).
    pub calendar: String,
    pub summary: String,
    pub location: String,
    pub notes: text_editor::Content,
    pub all_day: bool,
    /// As typed: "YYYY-MM-DD" and "HH:MM".
    pub start_date: String,
    pub start_time: String,
    pub end_date: String,
    pub end_time: String,
    pub repeat: ferrowl_core::events::Repeat,
    pub error: Option<String>,
    pub saving: bool,
    /// The open date picker: which date it sets and the month it shows.
    pub picker: Option<(EventField, chrono::NaiveDate)>,
}

impl EventForm {
    fn new(url: Option<String>, calendar: String, d: &ferrowl_core::events::Draft) -> EventForm {
        EventForm {
            url,
            calendar,
            summary: d.summary.clone(),
            location: d.location.clone(),
            notes: text_editor::Content::with_text(&d.description),
            all_day: d.all_day,
            start_date: d.start.format("%Y-%m-%d").to_string(),
            start_time: d.start.format("%H:%M").to_string(),
            end_date: d.end.format("%Y-%m-%d").to_string(),
            end_time: d.end.format("%H:%M").to_string(),
            repeat: d.repeat,
            error: None,
            saving: false,
            picker: None,
        }
    }

    /// The form as a draft, if dates and times are readable.
    pub fn draft(&self) -> Option<ferrowl_core::events::Draft> {
        let at = |date: &str, time: &str| {
            let d = chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").ok()?;
            let t = if self.all_day {
                chrono::NaiveTime::MIN
            } else {
                chrono::NaiveTime::parse_from_str(time.trim(), "%H:%M").ok()?
            };
            Some(d.and_time(t))
        };
        Some(ferrowl_core::events::Draft {
            summary: self.summary.clone(),
            location: self.location.clone(),
            description: self.notes.text().trim_end().to_string(),
            all_day: self.all_day,
            start: at(&self.start_date, &self.start_time)?,
            end: at(&self.end_date, &self.end_time)?,
            repeat: self.repeat,
        })
    }
}

/// The contact form (new contact, or changes to a stored one).
pub struct ContactForm {
    /// Where the contact is stored (empty = new).
    pub url: String,
    pub uid: String,
    pub name: String,
    pub organization: String,
    pub emails: Vec<String>,
    pub phones: Vec<String>,
    pub note: text_editor::Content,
    pub error: Option<String>,
    pub saving: bool,
}

impl ContactForm {
    fn new(c: &ferrowl_core::vcard::Contact) -> ContactForm {
        // Always one empty line to type in.
        let one = |v: &Vec<String>| {
            if v.is_empty() {
                vec![String::new()]
            } else {
                v.clone()
            }
        };
        ContactForm {
            url: c.url.clone(),
            uid: c.uid.clone(),
            name: c.name.clone(),
            organization: c.organization.clone(),
            emails: one(&c.emails),
            phones: one(&c.phones),
            note: text_editor::Content::with_text(&c.note),
            error: None,
            saving: false,
        }
    }

    /// The form as a contact, without the empty lines.
    pub fn contact(&self) -> ferrowl_core::vcard::Contact {
        let keep = |v: &Vec<String>| {
            v.iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        };
        ferrowl_core::vcard::Contact {
            url: self.url.clone(),
            uid: self.uid.clone(),
            name: self.name.trim().to_string(),
            emails: keep(&self.emails),
            phones: keep(&self.phones),
            organization: self.organization.trim().to_string(),
            note: self.note.text().trim_end().to_string(),
        }
    }

    /// Something to save: a name or an address.
    pub fn can_save(&self) -> bool {
        let c = self.contact();
        !self.saving && (!c.name.is_empty() || !c.emails.is_empty())
    }
}

/// Which field of the contact form changed.
#[derive(Debug, Clone, Copy)]
pub enum ContactField {
    Name,
    Organization,
    Email(usize),
    Phone(usize),
}

/// Which text field of the event form changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventField {
    Summary,
    Location,
    StartDate,
    StartTime,
    EndDate,
    EndTime,
}

/// The message list's drop-down menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Popup {
    /// Select all / read / unread / starred / none.
    Select,
    Sort,
    /// Move the checked messages.
    Move,
    /// Jump to a page of the list.
    Page,
    /// Gmail labels of the checked (or open) messages.
    Labels,
}

/// What the main window shows beside the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Mail,
    Contacts,
    Calendar,
}

/// A sync with the contacts or calendar server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncState {
    Idle,
    Running,
    Done,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Accounts,
    Security,
    Rules,
    Appearance,
}

/// "Select" menu above the list (Gmail-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Select {
    All,
    None,
    Read,
    Unread,
    Starred,
}

impl std::fmt::Display for Select {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(t(match self {
            Select::All => "select_all",
            Select::None => "select_none",
            Select::Read => "select_read",
            Select::Unread => "select_unread",
            Select::Starred => "select_starred",
        }))
    }
}

/// Actions on the checked messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bulk {
    Read,
    Unread,
    Star,
    /// Report spam, or (in Spam) not spam.
    Spam,
    Archive,
    Delete,
}

/// A folder in the "Move to" menu of the selection bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveTarget {
    pub name: String,
    pub label: String,
}

impl std::fmt::Display for MoveTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFilter {
    All,
    Unread,
    Starred,
    Attachments,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Conn {
    Offline,
    Connecting,
    Online,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeMode {
    New,
    Reply,
    Forward,
}

pub struct Compose {
    pub mode: ComposeMode,
    pub to: String,
    pub cc: String,
    pub bcc: String,
    pub show_cc: bool,
    pub subject: String,
    pub body: text_editor::Content,
    pub attachments: Vec<PathBuf>,
    pub in_reply_to: Option<String>,
    pub references: Vec<String>,
    pub sending: bool,
    pub error: Option<String>,
    /// Markdown formatting on: the message also gets an HTML part.
    pub rich: bool,
    /// OpenPGP options for this message.
    pub sign: bool,
    pub encrypt: bool,
    /// Rendered preview, shown instead of the editor while `Some`.
    pub preview: Option<Vec<iced::widget::markdown::Item>>,
    /// Address suggestions under a recipient field: (field, (name, address)).
    pub suggest: Option<(ComposeField, Vec<(String, String)>)>,
}

/// Actions of the message context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CtxAction {
    Open,
    Reply,
    ReplyAll,
    Forward,
    ToggleRead,
    Archive,
    Delete,
    DeleteForever,
    /// Show the folder list inside the menu.
    MoveMenu,
    Move(String),
}

/// Right-click menu on a message row.
#[derive(Debug, Clone)]
pub struct ContextMenu {
    pub uid: u32,
    pub at: iced::Point,
    pub folders: bool,
}

/// A message being dragged from the list to a folder.
#[derive(Debug, Clone)]
pub struct Drag {
    pub uid: u32,
    pub start: iced::Point,
    /// Became a drag (moved far enough), not just a click.
    pub active: bool,
    pub label: String,
}

/// Formatting actions of the compose toolbar (they insert Markdown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fmt {
    Bold,
    Italic,
    Strike,
    Code,
    Link,
    Bullets,
    Numbers,
    Quote,
    Heading,
}

impl Compose {
    fn new(mode: ComposeMode) -> Self {
        Self {
            mode,
            to: String::new(),
            cc: String::new(),
            bcc: String::new(),
            show_cc: false,
            subject: String::new(),
            body: text_editor::Content::new(),
            attachments: Vec::new(),
            in_reply_to: None,
            references: Vec::new(),
            sending: false,
            error: None,
            rich: true,
            sign: false,
            encrypt: false,
            preview: None,
            suggest: None,
        }
    }

    /// Applies a toolbar action to the editor: wraps the selection
    /// (or inserts an empty pair and puts the cursor inside it), or prefixes lines.
    pub fn format(&mut self, fmt: Fmt) {
        use iced::widget::text_editor::{Action, Edit, Motion};
        use std::sync::Arc;
        let paste =
            |c: &mut text_editor::Content, s: String| c.perform(Action::Edit(Edit::Paste(Arc::new(s))));
        let back = |c: &mut text_editor::Content, n: usize| {
            for _ in 0..n {
                c.perform(Action::Move(Motion::Left));
            }
        };
        let selection = self.body.selection().filter(|s| !s.is_empty());
        let wrap = |c: &mut text_editor::Content, l: &str, r: &str| match &selection {
            Some(sel) => paste(c, format!("{l}{sel}{r}")),
            None => {
                paste(c, format!("{l}{r}"));
                back(c, r.chars().count());
            }
        };
        let prefix = |c: &mut text_editor::Content, numbered: bool, p: &str| match &selection {
            Some(sel) => {
                let lines: Vec<String> = sel
                    .lines()
                    .enumerate()
                    .map(|(i, l)| {
                        if numbered {
                            format!("{}. {l}", i + 1)
                        } else {
                            format!("{p}{l}")
                        }
                    })
                    .collect();
                paste(c, lines.join("\n"))
            }
            None => {
                c.perform(Action::Move(Motion::Home));
                paste(c, if numbered { "1. ".into() } else { p.to_string() });
                c.perform(Action::Move(Motion::End));
            }
        };
        match fmt {
            Fmt::Bold => wrap(&mut self.body, "**", "**"),
            Fmt::Italic => wrap(&mut self.body, "_", "_"),
            Fmt::Strike => wrap(&mut self.body, "~~", "~~"),
            Fmt::Code => match &selection {
                Some(sel) if sel.contains('\n') => paste(&mut self.body, format!("```\n{sel}\n```")),
                _ => wrap(&mut self.body, "`", "`"),
            },
            Fmt::Link => match &selection {
                Some(sel) if sel.starts_with("http://") || sel.starts_with("https://") => {
                    paste(&mut self.body, format!("[{sel}]({sel})"))
                }
                Some(sel) => {
                    paste(&mut self.body, format!("[{sel}](https://)"));
                    back(&mut self.body, 1);
                }
                None => {
                    paste(&mut self.body, "[](https://)".into());
                    back(&mut self.body, 11);
                }
            },
            Fmt::Bullets => prefix(&mut self.body, false, "- "),
            Fmt::Numbers => prefix(&mut self.body, true, ""),
            Fmt::Quote => prefix(&mut self.body, false, "> "),
            Fmt::Heading => prefix(&mut self.body, false, "## "),
        }
    }
}

/// Draft of the account fields in settings (ports as text).
#[derive(Debug, Clone, Default)]
pub struct AccountDraft {
    pub display_name: String,
    pub email: String,
    pub username: String,
    pub password: String,
    pub imap_host: String,
    pub imap_port: String,
    pub smtp_host: String,
    pub smtp_port: String,
    /// Not shown in the form (edited under Security): carried through.
    pub pgp_passphrase: String,
    /// Provider picked for a new account (`None` = manual setup).
    pub preset: Option<usize>,
    pub protocol: crate::config::Protocol,
    pub pop3_keep_days: Option<u32>,
    /// Carried through (see `config::Account::keyring`).
    pub keyring: bool,
    /// Contacts server (empty = automatic).
    pub carddav_url: String,
    /// Calendar server (empty = automatic).
    pub caldav_url: String,
    /// Login for the contacts and calendar servers (empty = the mail login).
    pub dav_username: String,
    pub dav_password: String,
    /// Signed in with Google/Microsoft instead of a password.
    pub oauth: Option<ferrowl_core::oauth::Provider>,
}

impl AccountDraft {
    /// Fills in the servers of a known provider; the user name is the address.
    fn apply_preset(&mut self, i: usize) {
        let Some(p) = crate::providers::PROVIDERS.get(i) else {
            return;
        };
        self.preset = Some(i);
        // Providers in the list all use IMAP.
        self.protocol = crate::config::Protocol::Imap;
        self.imap_host = p.imap.0.into();
        self.imap_port = p.imap.1.to_string();
        self.smtp_host = p.smtp.0.into();
        self.smtp_port = p.smtp.1.to_string();
        self.username = self.email.trim().to_string();
    }

    pub fn blank() -> Self {
        Self {
            imap_port: "993".into(),
            smtp_port: "465".into(),
            ..Default::default()
        }
    }

    fn from(a: &Account) -> Self {
        Self {
            display_name: a.display_name.clone(),
            email: a.email.clone(),
            username: a.username.clone(),
            password: a.password.clone(),
            imap_host: a.imap_host.clone(),
            imap_port: a.imap_port.to_string(),
            smtp_host: a.smtp_host.clone(),
            smtp_port: a.smtp_port.to_string(),
            pgp_passphrase: a.pgp_passphrase.clone(),
            preset: None,
            protocol: a.protocol,
            pop3_keep_days: a.pop3_keep_days,
            keyring: a.keyring,
            carddav_url: a.carddav_url.clone(),
            caldav_url: a.caldav_url.clone(),
            dav_username: a.dav_username.clone(),
            dav_password: a.dav_password.clone(),
            oauth: a.oauth,
        }
    }

    pub fn to_account(&self) -> Option<Account> {
        let a = Account {
            display_name: self.display_name.trim().to_string(),
            email: self.email.trim().to_string(),
            username: self.username.trim().to_string(),
            password: self.password.clone(),
            imap_host: self.imap_host.trim().to_string(),
            imap_port: self.imap_port.trim().parse().ok()?,
            smtp_host: self.smtp_host.trim().to_string(),
            smtp_port: self.smtp_port.trim().parse().ok()?,
            pgp_passphrase: self.pgp_passphrase.clone(),
            protocol: self.protocol,
            pop3_keep_days: self.pop3_keep_days,
            keyring: self.keyring,
            carddav_url: self.carddav_url.trim().to_string(),
            caldav_url: self.caldav_url.trim().to_string(),
            dav_username: self.dav_username.trim().to_string(),
            dav_password: self.dav_password.clone(),
            oauth: self.oauth,
        };
        a.is_complete().then_some(a)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Field {
    DisplayName,
    Email,
    Username,
    Password,
    ImapHost,
    ImapPort,
    SmtpHost,
    SmtpPort,
    CardDav,
    DavUsername,
    DavPassword,
    CalDav,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComposeField {
    To,
    Cc,
    Bcc,
    Subject,
}

pub struct Toast {
    pub id: u64,
    pub text: String,
    pub error: bool,
}

pub struct App {
    pub config: Config,
    pub backend: Backend,
    pub screen: Screen,
    pub conn: Conn,
    pub folders: Vec<Folder>,
    pub unseen: HashMap<String, u32>,
    /// Unread count last shown on the dock icon.
    badge: Option<u32>,
    /// Unread mail in the other accounts' inboxes, by email.
    other_unread: HashMap<String, u32>,
    pub expanded: HashSet<String>,
    pub current_folder: Option<String>,
    pub headers: Vec<Header>,
    pub loading_list: bool,
    pub filter: ListFilter,
    pub search: String,
    /// Full-text matches from the local index while searching.
    pub search_results: Option<Vec<Header>>,
    /// Folders already indexed (or being indexed) in this session.
    pub indexed: HashSet<String>,
    /// Highest inbox UID seen so far: newer unseen messages are "new mail".
    pub inbox_max_uid: Option<u32>,
    /// Fingerprint of the active account's OpenPGP key, if any.
    pub pgp_fpr: Option<String>,
    pub pgp_pass_draft: String,
    pub pgp_contacts: usize,
    pub selected: Option<u32>,
    pub message: Option<FullMessage>,
    pub body: text_editor::Content,
    /// Hidden copy of `body` used to find the link under the mouse.
    pub body_probe: text_editor::Content,
    /// The mouse is over a link of the plain-text body (hand cursor).
    pub body_link_hover: bool,
    /// The open message's HTML, rendered into image tiles.
    pub html_render: Option<crate::html_view::Rendered>,
    pub html_rendering: bool,
    /// The open message's HTML could not be painted (its UID): it is shown
    /// as text. Cleared when a message loads.
    pub html_failed: Option<u32>,
    /// Width the reader actually gives the HTML message, once measured.
    pub reader_width: Option<f32>,
    /// Remote content loaded for this message despite the setting.
    pub html_force_remote: Option<u32>,
    /// Link under the pointer in the HTML view (shown at the bottom).
    pub hover_link: Option<String>,
    /// Last pointer position over the HTML view (logical, document coords).
    pub html_pointer: Option<(f32, f32)>,
    /// Where the hover link was last looked up (throttles hit tests).
    hover_probe: Option<(f32, f32)>,
    /// Mouse held down on the HTML message: where, and whether it moved
    /// (a drag selects text, a click opens links).
    html_press: Option<((f32, f32), bool)>,
    /// Last press on the HTML message, to tell double clicks.
    html_last_press: Option<(Instant, (f32, f32))>,
    /// A selection change is being painted; later ones wait here.
    html_select_busy: bool,
    html_select_queue: Vec<crate::html_view::Select>,
    pub window_width: f32,
    pub window_height: f32,
    pub scale_factor: f32,
    /// Thumbnails of the open message's attachments, by index.
    pub thumbs: HashMap<usize, iced::widget::image::Handle>,
    /// Attachment preview: index and content (None while loading).
    pub preview: Option<(usize, Option<crate::preview::Content>)>,
    pub cursor: iced::Point,
    /// Dragging the list's edge: cursor x and list width when it started.
    pub list_resize: Option<(f32, f32)>,
    pub menu: Option<ContextMenu>,
    pub drag: Option<Drag>,
    pub hover_row: Option<u32>,
    pub drop_folder: Option<String>,
    /// Action waiting for the message to load (reply/forward from the menu).
    pending_action: Option<(u32, CtxAction)>,
    pub loading_message: bool,
    pub confirm_delete: bool,
    /// Messages checked in the list (uids of the open folder).
    pub checked: std::collections::BTreeSet<u32>,
    /// Conversations opened in the list: a conversation is expanded when
    /// any of its messages is here (opening a message adds it).
    pub open_threads: std::collections::BTreeSet<u32>,
    /// The delete dialog is for the checked messages, not the open one.
    pub bulk_delete: bool,
    pub appearance_open: bool,
    pub compose: Option<Compose>,
    pub draft: AccountDraft,
    /// Password fields of the account form show what is typed (the eye).
    pub show_password: bool,
    /// Account being edited in settings (`None` = new account).
    pub editing: Option<usize>,
    pub confirm_remove: bool,
    /// Waiting for the browser sign-in (Google/Microsoft).
    pub oauth_busy: bool,
    /// Bumped on account switch, to drop results of the previous account.
    pub generation: u64,
    pub testing: bool,
    pub test_result: Option<Res<String>>,
    pub last_sync: Option<chrono::DateTime<chrono::Local>>,
    pub toast: Option<Toast>,
    toast_seq: u64,
    dev_shots: u32,
    /// Contacts or calendar in place of the message list and reader.
    pub pane: Pane,
    /// Contacts matching `contacts_query`, by name.
    pub contacts: Vec<ferrowl_core::vcard::Contact>,
    pub contacts_query: String,
    pub contact_selected: Option<usize>,
    pub contacts_sync: SyncState,
    /// First day of the month shown in the calendar.
    pub cal_month: chrono::NaiveDate,
    /// That month's events.
    pub agenda: Vec<ferrowl_core::backend::AgendaEntry>,
    pub agenda_selected: Option<usize>,
    pub calendar_sync: SyncState,
    pub invite: Option<InviteCard>,
    /// What the open message is about (shipment, code, bill), by its UID.
    pub insights: Option<(u32, Vec<ferrowl_core::insights::Insight>)>,
    /// A bill's due date is being added to the calendar (a second click
    /// would add it twice).
    pub due_adding: bool,
    pub event_form: Option<EventForm>,
    /// Keyboard modifiers held now (for Shift/Ctrl+click in the list).
    pub modifiers: keyboard::Modifiers,
    /// Row last clicked or checked: where a Shift+click range starts.
    pub anchor: Option<u32>,
    /// Open drop-down menu of the list and where it was opened.
    pub popup: Option<(Popup, iced::Point)>,
    /// What is being typed in the blocked / allowed senders boxes.
    pub sender_input: (String, String),
    /// The rule being written in Settings › Rules (`.1`: not saved yet).
    pub rule_edit: Option<(ferrowl_core::rules::Rule, bool)>,
    /// Inbox messages the rule being written would pick (sender, subject).
    pub rule_preview: Vec<(String, String)>,
    /// Page of the message list shown (0 = newest).
    pub page: usize,
    /// The account is Gmail: messages have labels.
    pub gmail: bool,
    /// Gmail inbox tab shown.
    pub category: ferrowl_core::mail::Category,
    /// Asking before deleting the open event.
    pub confirm_event_delete: bool,
    pub contact_form: Option<ContactForm>,
    pub confirm_contact_delete: bool,
}

// ---------------------------------------------------------------- messages

// FullMessage results are large, but messages are short-lived: boxing them
// would only add indirection to every update.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Message {
    /// Result of a backend operation started for account generation `.0`.
    Scoped(u64, Box<Message>),
    // connection and data
    Connected(Res<Vec<Folder>>),
    Unseen(Res<HashMap<String, u32>>),
    SelectFolder(String),
    ToggleNode(String),
    HeadersLoaded {
        folder: String,
        silent: bool,
        result: Res<(Vec<Header>, u32)>,
    },
    /// A background indexing step finished; `Some(seq)` = continue from there.
    Indexed {
        folder: String,
        result: Res<Option<u32>>,
    },
    /// The IDLE connection saw a change in the inbox (account generation).
    Push(u64),
    /// New unread mail in another account's inbox: (account address,
    /// (sender, subject) of each message).
    OtherMail(String, Vec<(String, String)>),
    /// Unread count in another account's inbox (by email), for the dock icon.
    OtherUnread(String, u32),
    /// Inbox checked after a push while another folder is open.
    PushChecked(Res<(Vec<Header>, u32)>),
    ToggleNotifications(bool),
    // OpenPGP
    PgpGenerate,
    PgpImportOwn,
    PgpImportOwnPicked(Option<PathBuf>),
    PgpExport,
    PgpImportContacts,
    PgpImportContactsPicked(Option<Vec<PathBuf>>),
    PgpPassphrase(String),
    PgpSavePassphrase,
    /// Result of a key operation: toast text.
    PgpDone(Res<String>),
    ComposeToggleSign,
    ComposeToggleEncrypt,
    Sync,
    Tick,
    // list
    Search(String),
    SetFilter(ListFilter),
    /// Checkbox of a list row: its uids (a whole conversation when grouped).
    Check(Vec<u32>, bool),
    /// Star (true) or unstar messages of the open folder.
    ToggleStar(Vec<u32>, bool),
    StarSaved(Res<()>),
    OpenPopup(Popup),
    ClosePopup,
    SetSort(ferrowl_core::sort::SortOrder),
    /// Report the open message as spam (or, in Spam, as not spam).
    SpamOpen,
    /// Block the open message's sender and move it to Spam.
    BlockSender,
    /// The sender lists moved some messages (count).
    RulesApplied(usize, Res<String>),
    /// Text typed in the blocked (false) or allowed (true) senders box.
    SenderInput(bool, String),
    /// Settings › Rules: start a new rule, or edit the one with this id.
    RuleNew,
    RuleEdit(String),
    RuleCondition(String),
    RuleName(String),
    /// Turn a mark / star / silence action on or off.
    RuleAction(ferrowl_core::rules::Action, bool),
    /// Where matching mail goes: `None` stays in the inbox.
    RuleDestination(Option<ferrowl_core::rules::Action>),
    RuleEnabled(bool),
    RuleSave,
    RuleCancel,
    RuleDelete(String),
    /// Turn a saved rule on or off from the list.
    RuleSwitch(String, bool),
    /// The rules acted on the inbox's new messages.
    MailRules(Res<ferrowl_core::rules::Plan>),
    AddSender(bool),
    RemoveSender(String),
    /// Next (+1) or previous (-1) page of the list.
    Page(i32),
    /// Jump to a page (0 = first).
    GoPage(usize),
    /// Gmail inbox tab.
    SetCategory(ferrowl_core::mail::Category),
    /// Add (true) or remove a Gmail label (raw name) on the checked or open
    /// messages.
    ToggleLabel(String, bool),
    LabelSaved(Res<()>),
    SetPageSize(u32),
    /// Close the open message (the list takes the whole width again).
    CloseReader,
    /// Print the open message (through the browser's print dialog).
    Print,
    SelectBy(Select),
    Bulk(Bulk),
    /// Moves the checked messages to a folder.
    BulkMove(MoveTarget),
    BulkDone {
        uids: Vec<u32>,
        note: &'static str,
        result: Res<()>,
    },
    BulkSeen(Res<()>),
    /// Conversations (true) or one row per message (false).
    SetThreaded(bool),
    /// Expands or collapses a conversation in the list (its messages).
    ToggleThread(Vec<u32>),
    Open(u32),
    Loaded(Res<FullMessage>),
    ToggleHtml,
    /// The painted HTML of message `uid`, or why there is none.
    HtmlRendered(
        u32,
        Result<crate::html_view::Rendered, crate::html_view::Unrendered>,
    ),
    LoadRemoteImages,
    ToggleRemoteImages(bool),
    /// Pointer moved over the HTML view (document coordinates).
    HtmlPointer(f32, f32),
    HoverLink(Option<String>),
    HtmlLeave,
    /// Mouse pressed on the HTML message: starts a selection (or a click).
    HtmlPress,
    /// The HTML message painted again after a selection change.
    HtmlSelected(Option<crate::html_view::Rendered>),
    /// Text to put on the clipboard (Ctrl+C on the HTML message).
    CopyText(Option<String>),
    WindowResized(iced::Size),
    /// The reader's measured width changed.
    ReaderWidth(f32),
    ScaleFactor(f32),
    // pointer, context menu, drag and drop
    Thumbs(u32, Vec<(usize, iced::widget::image::Handle)>),
    PreviewAttachment(usize),
    PreviewReady(u32, usize, crate::preview::Content),
    ClosePreview,
    CursorMoved(iced::Point),
    ListResizeStart,
    /// Window closed: exit right away.
    Quit,
    /// Another launch asked this window to come forward.
    Raise,
    MouseDown,
    MouseUp,
    /// The window lost the keyboard focus.
    Unfocused,
    RowEnter(u32),
    RowExit(u32),
    FolderEnter(String),
    FolderExit(String),
    OpenMenu(u32),
    CloseMenu,
    MenuAction(u32, CtxAction),
    OpenLink(String),
    BodyAction(text_editor::Action),
    /// The mouse moved over the plain-text body (position in it).
    BodyHover(iced::Point),
    BodyLeave,
    // message actions
    Reply,
    ReplyAll,
    Forward,
    Archive,
    Delete,
    ConfirmDelete(bool),
    ToggleRead,
    Done {
        uid: u32,
        note: &'static str,
        result: Res<()>,
    },
    Archived {
        uid: u32,
        result: Res<String>,
    },
    SeenChanged(Res<()>),
    SaveAttachment(usize),
    // appearance
    ToggleAppearance,
    CloseAppearance,
    SetTheme(&'static str),
    SetDensity(Density),
    TogglePreview(bool),
    SetLanguage(String),
    // settings
    OpenSettings(Section),
    CloseSettings,
    Draft(Field, String),
    UseEmailAsUsername,
    TestConnection,
    Tested(Res<String>),
    SaveAccount,
    SwitchAccount(usize),
    EditAccount(usize),
    NewAccount,
    /// New account: a known provider (index in `providers::PROVIDERS`) or manual setup.
    PickProvider(Option<usize>),
    SetProtocol(crate::config::Protocol),
    /// POP3: days to keep downloaded messages on the server (`None` = always).
    SetKeepDays(Option<u32>),
    /// From the sidebar: open settings on an empty account form.
    AddAccount,
    /// The eye next to a password field.
    TogglePassword,
    /// Settings of the account at this index (from the sidebar).
    OpenAccountSettings(usize),
    RemoveAccount(bool),
    /// Sign in with Google/Microsoft in the browser.
    OAuthSignIn(ferrowl_core::oauth::Provider),
    OAuthDone(Res<(ferrowl_core::oauth::Provider, String)>),
    /// Back to a password instead of the provider's sign-in.
    OAuthUsePassword,
    // contacts
    OpenContacts,
    ContactsQuery(String),
    ContactSelect(usize),
    SyncContacts,
    ContactsSynced(Res<usize>),
    NewContact,
    EditContact,
    ContactField(ContactField, String),
    ContactNote(text_editor::Action),
    /// Another empty address (true) or phone number (false).
    ContactAddLine(bool),
    SaveContact,
    ContactSaved(Res<String>),
    CancelContact,
    /// Delete the open contact (false = ask first).
    DeleteContact(bool),
    ContactDeleted(Res<()>),
    OpenCalendar,
    /// Moves the calendar by months (0 = back to this month).
    CalendarMonth(i32),
    AgendaSelect(usize),
    SyncCalendar,
    CalendarSynced(Res<usize>),
    /// Answer to the open message's invitation.
    InviteRespond(ferrowl_core::invite::Reply),
    InviteResponded(Res<String>),
    /// Remove the cancelled event from the calendar.
    InviteRemove,
    /// Copy a tracking number or code from an insight card.
    CopyInsight(String),
    /// Put a bill's due date (day, number, amount) in the calendar.
    AddDue(chrono::NaiveDate, String, String),
    DueAdded(Res<String>),
    InviteRemoved(Res<String>),
    NewEvent,
    /// Edit the open event.
    EditEvent,
    EventField(EventField, String),
    EventNotes(text_editor::Action),
    EventAllDay(bool),
    /// Open or close the date picker of the start or end date.
    EventPicker(EventField),
    /// Show the picker's previous (-1) or next (1) month.
    EventPickerMonth(i32),
    EventPickDate(chrono::NaiveDate),
    EventRepeat(ferrowl_core::events::Repeat),
    EventCalendar(String),
    SaveEvent,
    EventSaved(Res<String>),
    CancelEvent,
    /// Delete the open event (false = ask first).
    DeleteEvent(bool),
    EventDeleted(Res<String>),
    /// New message to (name, address).
    WriteTo(String, String),
    // compose
    ComposeNew,
    ComposeField(ComposeField, String),
    /// Chosen address suggestion: (field, name, address).
    ComposePick(ComposeField, String, String),
    /// Enter in a recipient field: take the first suggestion.
    ComposePickFirst(ComposeField),
    ComposeToggleCc,
    ComposeEdit(text_editor::Action),
    ComposeFormat(Fmt),
    ComposeToggleRich,
    ComposeTogglePreview,
    ComposeAttach,
    ComposeAttached(Vec<PathBuf>),
    ComposeRemove(usize),
    ComposeSend,
    ComposeSent(Res<Option<String>>),
    ComposeClose,
    // misc
    Key(keyboard::Event),
    DismissToast(u64),
    CopyDetails(String),
    // development: FERROWL_SCREENSHOT=<file.png> captures the window and quits
    DevScreenshot,
    DevShot(iced::window::Screenshot),
    Noop,
}

// ---------------------------------------------------------------- startup

impl App {
    pub fn boot() -> (Self, Task<Message>) {
        let mut config = Config::load();
        // Development overrides (not saved unless the user changes a setting).
        if let Ok(lang) = std::env::var("FERROWL_LANG") {
            config.ui.language = lang;
        }
        if let Ok(theme) = std::env::var("FERROWL_THEME") {
            config.ui.theme = theme;
        }
        i18n::set(Some(&config.ui.language));
        let draft = config
            .account()
            .map(AccountDraft::from)
            .unwrap_or_else(AccountDraft::blank);
        let editing = config.account().map(|_| config.active);
        let mut app = App {
            screen: if config.account().is_some() {
                Screen::Mail
            } else {
                Screen::Settings(Section::Accounts)
            },
            config,
            backend: Backend::default(),
            conn: Conn::Offline,
            folders: Vec::new(),
            unseen: HashMap::new(),
            badge: None,
            other_unread: HashMap::new(),
            expanded: HashSet::new(),
            current_folder: None,
            headers: Vec::new(),
            loading_list: false,
            filter: ListFilter::All,
            search: String::new(),
            search_results: None,
            indexed: HashSet::new(),
            inbox_max_uid: None,
            pgp_fpr: None,
            pgp_pass_draft: String::new(),
            pgp_contacts: 0,
            selected: None,
            message: None,
            body: text_editor::Content::new(),
            body_probe: text_editor::Content::new(),
            body_link_hover: false,
            html_render: None,
            html_rendering: false,
            html_failed: None,
            reader_width: None,
            html_force_remote: None,
            hover_link: None,
            html_pointer: None,
            hover_probe: None,
            html_press: None,
            html_last_press: None,
            html_select_busy: false,
            html_select_queue: Vec::new(),
            window_width: 1440.0,
            window_height: 900.0,
            scale_factor: 1.0,
            thumbs: HashMap::new(),
            preview: None,
            cursor: iced::Point::ORIGIN,
            list_resize: None,
            menu: None,
            drag: None,
            hover_row: None,
            drop_folder: None,
            pending_action: None,
            loading_message: false,
            confirm_delete: false,
            checked: Default::default(),
            open_threads: Default::default(),
            bulk_delete: false,
            appearance_open: false,
            compose: None,
            draft,
            show_password: false,
            editing,
            confirm_remove: false,
            oauth_busy: false,
            generation: 0,
            testing: false,
            test_result: None,
            last_sync: None,
            toast: None,
            toast_seq: 0,
            dev_shots: 0,
            pane: Pane::Mail,
            contacts: Vec::new(),
            contacts_query: String::new(),
            contact_selected: None,
            contacts_sync: SyncState::Idle,
            cal_month: month_start(ferrowl_core::ical::today()),
            agenda: Vec::new(),
            agenda_selected: None,
            calendar_sync: SyncState::Idle,
            invite: None,
            insights: None,
            due_adding: false,
            event_form: None,
            modifiers: keyboard::Modifiers::default(),
            anchor: None,
            popup: None,
            sender_input: (String::new(), String::new()),
            rule_edit: None,
            rule_preview: Vec::new(),
            page: 0,
            gmail: false,
            category: ferrowl_core::mail::Category::Primary,
            confirm_event_delete: false,
            contact_form: None,
            confirm_contact_delete: false,
        };
        // Development: open a given view directly (used for screenshots).
        match std::env::var("FERROWL_DEV_VIEW").as_deref() {
            Ok("settings") => app.screen = Screen::Settings(Section::Accounts),
            Ok(v) if v.starts_with("new-account") => {
                app.screen = Screen::Settings(Section::Accounts);
                app.editing = None;
                app.draft = AccountDraft::blank();
                // "new-account:<address>" types an address, as a user would.
                if let Some(email) = v.strip_prefix("new-account:") {
                    app.draft.email = email.into();
                    if let Some(i) = crate::providers::for_email(email) {
                        app.draft.apply_preset(i);
                    }
                }
            }
            Ok("appearance") => app.screen = Screen::Settings(Section::Appearance),
            Ok("general") => app.screen = Screen::Settings(Section::General),
            Ok("security") => app.screen = Screen::Settings(Section::Security),
            Ok("rules") => app.screen = Screen::Settings(Section::Rules),
            // The rule editor, with a condition and its preview.
            Ok(v) if v.starts_with("rule:") => {
                app.screen = Screen::Settings(Section::Rules);
                let _ = app.update(Message::RuleNew);
                let _ = app.update(Message::RuleCondition(v["rule:".len()..].into()));
            }
            Ok("popover") => app.appearance_open = true,
            // The cached inbox without a server (layout of the message list).
            Ok("list") => {
                if let Some(acc) = app.config.account() {
                    app.headers = Backend::cached_headers(acc, "INBOX");
                    app.current_folder = Some("INBOX".into());
                }
            }
            Ok("calendar") => {
                app.pane = Pane::Calendar;
                if let Some(m) = std::env::var("FERROWL_DEV_MONTH")
                    .ok()
                    .and_then(|m| chrono::NaiveDate::parse_from_str(&format!("{m}-01"), "%Y-%m-%d").ok())
                {
                    app.cal_month = m;
                }
                app.reload_agenda();
                let nth = std::env::var("FERROWL_DEV_OPEN_NTH")
                    .ok()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                app.agenda_selected = (nth < app.agenda.len()).then_some(nth);
                // The event form, new or for the open event.
                let _ = match std::env::var("FERROWL_DEV_EVENT").as_deref() {
                    Ok("new") => app.update(Message::NewEvent),
                    // The new event with the start date's picker open.
                    Ok("picker") => {
                        let _ = app.update(Message::NewEvent);
                        app.update(Message::EventPicker(EventField::StartDate))
                    }
                    Ok("edit") => app.update(Message::EditEvent),
                    Ok("delete") => app.update(Message::DeleteEvent(false)),
                    _ => Task::none(),
                };
            }
            Ok("contacts") => {
                app.pane = Pane::Contacts;
                app.reload_contacts();
                let nth = std::env::var("FERROWL_DEV_OPEN_NTH")
                    .ok()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                app.contact_selected = (nth < app.contacts.len()).then_some(nth);
                // The contact form, new or for the open contact.
                let _ = match std::env::var("FERROWL_DEV_CONTACT").as_deref() {
                    Ok("new") => app.update(Message::NewContact),
                    Ok("edit") => app.update(Message::EditContact),
                    Ok("delete") => app.update(Message::DeleteContact(false)),
                    _ => Task::none(),
                };
            }
            Ok("compose") => {
                let mut c = Compose::new(ComposeMode::New);
                c.to = "giulia.ferri@example.com".into();
                c.subject = "Customer portal quote".into();
                c.body = text_editor::Content::with_text(
                    "Hi Giulia,\n\nsounds **good** to me. Two notes:\n\n- delivery in _two_ phases\n- see [the brief](https://example.com)\n\nFabio",
                );
                if std::env::var("FERROWL_DEV_PREVIEW").is_ok() {
                    c.preview = Some(iced::widget::markdown::parse(&c.body.text()).collect());
                }
                app.compose = Some(c);
            }
            _ => {}
        }
        app.refresh_pgp();
        let scale = iced::window::latest()
            .and_then(iced::window::scale_factor)
            .map(Message::ScaleFactor);
        // Development: type into "To" once the list (and address book) loaded.
        let dev_to = match std::env::var("FERROWL_DEV_TO") {
            Ok(v) => Task::perform(
                async move {
                    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                    v
                },
                |v| Message::ComposeField(ComposeField::To, v),
            ),
            Err(_) => Task::none(),
        };
        let task = Task::batch([app.connect(), scale, dev_to]);
        (app, task)
    }

    pub fn title(&self) -> String {
        match (&self.current_folder, self.screen) {
            (_, Screen::Settings(_)) => format!("{} — Ferrowl", t("settings")),
            (Some(f), _) => format!("{} — Ferrowl", self.folder_label(f)),
            _ => "Ferrowl".into(),
        }
    }

    pub fn theme(&self) -> Theme {
        theme::iced_theme(theme::get(&self.config.ui.theme))
    }

    pub fn palette(&self) -> Palette {
        theme::get(&self.config.ui.theme).palette
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            keyboard::listen().map(Message::Key),
            iced::time::every(AUTO_REFRESH).map(|_| Message::Tick),
            iced::window::resize_events().map(|(_, size)| Message::WindowResized(size)),
            iced::window::close_requests().map(|_| Message::Quit),
            Subscription::run(raise_requests),
            iced::event::listen_with(|event, _status, _window| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::CursorMoved(position))
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)) => {
                    Some(Message::MouseDown)
                }
                iced::Event::Mouse(iced::mouse::Event::ButtonReleased(iced::mouse::Button::Left)) => {
                    Some(Message::MouseUp)
                }
                iced::Event::Window(iced::window::Event::Unfocused) => Some(Message::Unfocused),
                _ => None,
            }),
        ];
        if std::env::var_os("FERROWL_SCREENSHOT").is_some() {
            let delay = std::env::var("FERROWL_SCREENSHOT_DELAY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8);
            subs.push(iced::time::every(Duration::from_secs(delay)).map(|_| Message::DevScreenshot));
        }
        // Push: IDLE on the inbox of the account in use.
        // (POP3 has no push: the periodic refresh downloads new mail.)
        if self.conn == Conn::Online
            && let (Some(account), Some(inbox)) =
                (self.config.account(), self.folder_by_kind(FolderKind::Inbox))
            && !account.is_pop3()
        {
            subs.push(crate::push::subscription(crate::push::Watch {
                account: account.clone(),
                folder: inbox.name.clone(),
                generation: self.generation,
                other: false,
            }));
        }
        // The other accounts' inboxes too: their unread count for the dock
        // icon and, when enabled, a notification for their new mail.
        for (i, account) in self.config.accounts.iter().enumerate() {
            if i == self.config.active || account.is_pop3() {
                continue;
            }
            subs.push(crate::push::subscription(crate::push::Watch {
                account: account.clone(),
                folder: "INBOX".into(),
                generation: 0,
                other: true,
            }));
        }
        Subscription::batch(subs)
    }

    // ------------------------------------------------------------ helper

    /// Runs a backend operation; its result is dropped if the account
    /// changes in the meantime.
    fn perform<T: Send + 'static>(
        &self,
        fut: impl std::future::Future<Output = T> + Send + 'static,
        f: impl Fn(T) -> Message + Send + 'static,
    ) -> Task<Message> {
        let generation = self.generation;
        Task::perform(fut, move |r| Message::Scoped(generation, Box::new(f(r))))
    }

    /// Unread mail in the inbox of account `i`: the open account's from the
    /// list, the others' from their push watch (0 until it is known).
    pub fn account_unread(&self, i: usize) -> u32 {
        if i == self.config.active {
            let inbox = self.folder_by_kind(FolderKind::Inbox).map(|f| f.name.as_str());
            return inbox.and_then(|f| self.unseen.get(f).copied()).unwrap_or(0);
        }
        self.config
            .accounts
            .get(i)
            .and_then(|a| self.other_unread.get(&a.email).copied())
            .unwrap_or(0)
    }

    /// The dock icon shows the unread mail of every account's inbox: the open
    /// account's from the list, the others' from their push watch.
    fn update_badge(&mut self) -> Task<Message> {
        let count: u32 = (0..self.config.accounts.len())
            .map(|i| self.account_unread(i))
            .sum();
        if self.badge == Some(count) {
            return Task::none();
        }
        self.badge = Some(count);
        Task::perform(
            async move {
                let _ = tokio::task::spawn_blocking(move || crate::push::set_badge(count)).await;
            },
            |_| Message::Noop,
        )
    }

    /// Compares an inbox listing with the last one: unseen messages with a
    /// higher UID are new mail and get a desktop notification. The first
    /// listing only sets the baseline.
    fn note_new_mail(&mut self, headers: &[Header]) -> Task<Message> {
        let Some(max) = headers.iter().map(|h| h.uid).max() else {
            return Task::none();
        };
        let Some(previous) = self
            .inbox_max_uid
            .replace(max.max(self.inbox_max_uid.unwrap_or(0)))
        else {
            return Task::none();
        };
        let mut new: Vec<&Header> = headers.iter().filter(|h| h.uid > previous && !h.seen).collect();
        // Rules may keep some of them quiet.
        if let Some(acc) = self.config.account() {
            let quiet = Backend::silenced(acc, headers);
            new.retain(|h| !quiet.contains(&h.uid));
        }
        if new.is_empty() || !self.config.ui.notifications {
            return Task::none();
        }
        let (summary, body) = match new.as_slice() {
            [one] => (one.from.clone(), one.subject.clone()),
            many => (
                tf("notif_many", &[("n", &many.len().to_string())]),
                many.iter()
                    .take(3)
                    .map(|h| format!("{} — {}", h.from, h.subject))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        };
        Task::perform(
            async move {
                let _ = tokio::task::spawn_blocking(move || crate::push::notify(summary, body)).await;
            },
            |_| Message::Noop,
        )
    }

    /// Runs the user's rules on the inbox's new messages (the engine
    /// remembers which ones they already saw).
    fn apply_rules(&self, folder: String, headers: &[Header]) -> Task<Message> {
        let b = self.backend.clone();
        let headers = headers.to_vec();
        let folders = self.folders.clone();
        self.perform(
            async move { b.apply_rules(folder, headers, folders).await },
            Message::MailRules,
        )
    }

    // -------------------------------------------------------- message actions
    // By UID, so they work on any row (context menu, drag and drop), not only
    // on the open message.

    /// Builds attachment thumbnails (images, first PDF page) in the background.
    fn start_thumbnails(&self) -> Task<Message> {
        let Some(msg) = &self.message else {
            return Task::none();
        };
        let uid = msg.uid;
        let atts: Vec<(usize, String, Vec<u8>)> = msg
            .attachments
            .iter()
            .enumerate()
            .filter(|(_, a)| {
                matches!(
                    crate::preview::kind(&a.name, &a.data),
                    crate::preview::Kind::Image | crate::preview::Kind::Pdf
                )
            })
            .map(|(i, a)| (i, a.name.clone(), a.data.clone()))
            .collect();
        if atts.is_empty() {
            return Task::none();
        }
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    atts.iter()
                        .filter_map(|(i, name, data)| Some((*i, crate::preview::thumbnail(name, data)?)))
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_default()
            },
            move |list| Message::Thumbs(uid, list),
        )
    }

    fn run_action(&mut self, uid: u32, action: CtxAction) -> Task<Message> {
        match action {
            CtxAction::Open => self.open(uid),
            CtxAction::Reply | CtxAction::ReplyAll | CtxAction::Forward => {
                // These need the full message: open it first if needed.
                if self.message.as_ref().is_some_and(|m| m.uid == uid) {
                    self.update(match action {
                        CtxAction::Reply => Message::Reply,
                        CtxAction::ReplyAll => Message::ReplyAll,
                        _ => Message::Forward,
                    })
                } else {
                    self.pending_action = Some((uid, action));
                    self.open(uid)
                }
            }
            CtxAction::ToggleRead => self.toggle_read(uid),
            CtxAction::Archive => self.archive(uid),
            CtxAction::Delete => self.delete(uid),
            CtxAction::DeleteForever => self.ask_delete_forever(uid),
            CtxAction::Move(target) => self.move_message(uid, target),
            CtxAction::MoveMenu => Task::none(),
        }
    }

    /// An action on all checked messages, one server command each.
    /// Stars or unstars messages in the lists right away (the server
    /// follows; a failure reloads the folder).
    fn set_stars(&mut self, uids: &[u32], on: bool) {
        for h in self
            .headers
            .iter_mut()
            .chain(self.search_results.iter_mut().flatten())
        {
            if uids.contains(&h.uid) {
                h.flagged = on;
            }
        }
    }

    /// Gmail inbox: the Primary / Promotions / Social… tabs are shown.
    pub fn tabs_shown(&self) -> bool {
        self.gmail
            && self.pane == Pane::Mail
            && self.folder_by_kind(FolderKind::Inbox).map(|f| f.name.as_str())
                == self.current_folder.as_deref()
    }

    /// The messages the toolbar actions apply to: the checked ones, else the
    /// open one.
    pub fn targets(&self) -> Vec<u32> {
        if self.checked.is_empty() {
            self.selected.into_iter().collect()
        } else {
            self.checked.iter().copied().collect()
        }
    }

    /// Gmail labels that can be put on messages: the user's own (Gmail's
    /// system folders and the open folder left out).
    pub fn label_choices(&self) -> Vec<&Folder> {
        self.folders
            .iter()
            .filter(|f| {
                f.kind == FolderKind::Other
                    && f.selectable
                    && !f.name.starts_with("[Gmail]")
                    && !f.name.starts_with("[Google Mail]")
                    && self.current_folder.as_deref() != Some(f.name.as_str())
            })
            .collect()
    }

    /// No message is open (or opening): the list uses the whole width.
    /// Width of the message list beside the reader: the user's choice,
    /// narrowed when the window leaves the reader too little room.
    pub fn list_width(&self) -> f32 {
        use ferrowl_core::config::{LIST_MAX_WIDTH, LIST_MIN_WIDTH};
        let room = (self.window_width - 240.0 - 480.0).max(LIST_MIN_WIDTH);
        self.config
            .ui
            .list_width
            .clamp(LIST_MIN_WIDTH, LIST_MAX_WIDTH.min(room))
    }

    pub fn list_is_wide(&self) -> bool {
        self.selected.is_none() && !self.loading_message
    }

    /// Rows of the message list as shown: the messages of each row
    /// (one, or a whole conversation).
    pub fn rows(&self) -> Vec<Vec<u32>> {
        self.list_rows().into_iter().map(|(_, uids, _)| uids).collect()
    }

    /// The list as shown, in the chosen order: (row's message — the newest
    /// of a conversation —, its messages, any unread).
    pub fn list_rows(&self) -> Vec<(&Header, Vec<u32>, bool)> {
        let visible = self.visible();
        let order = self.config.ui.sort;
        let mut rows: Vec<(&Header, Vec<u32>, bool)> = if self.config.ui.threaded {
            ferrowl_core::threads::group(&visible)
                .into_iter()
                .map(|t| {
                    let uids = t.members.iter().map(|&i| visible[i].uid).collect();
                    (visible[t.latest()], uids, t.unread > 0)
                })
                .collect()
        } else {
            visible.iter().map(|h| (*h, vec![h.uid], !h.seen)).collect()
        };
        rows.sort_by(|a, b| order.compare(a.0, b.0));
        rows
    }

    /// Shift+click: checks every row between the last clicked one and
    /// this one (inclusive).
    fn check_range(&mut self, uid: u32) {
        let rows = self.rows();
        let pos = |u: u32| rows.iter().position(|r| r.contains(&u));
        let (Some(to), Some(from)) = (pos(uid), self.anchor.and_then(pos)) else {
            if let Some(r) = pos(uid) {
                self.checked.extend(rows[r].iter().copied());
            }
            self.anchor = Some(uid);
            return;
        };
        let (a, b) = if from <= to { (from, to) } else { (to, from) };
        for r in &rows[a..=b] {
            self.checked.extend(r.iter().copied());
        }
    }

    /// Moves messages to Spam (or back to the inbox). `quiet`: moved by the
    /// sender lists (one summary toast instead of the usual one).
    fn junk(&mut self, folder: String, uids: Vec<u32>, to_spam: bool, quiet: bool) -> Task<Message> {
        if uids.is_empty() {
            return Task::none();
        }
        let b = self.backend.clone();
        let folders = self.folders.clone();
        if quiet {
            let n = uids.len();
            return self.perform(
                async move {
                    if to_spam {
                        b.report_spam(folder, uids, folders).await
                    } else {
                        b.not_spam(folder, uids, folders).await
                    }
                },
                move |r| Message::RulesApplied(n, r),
            );
        }
        let note = if to_spam { "spam_moved" } else { "inbox_moved" };
        let ids = uids.clone();
        self.perform(
            async move {
                if to_spam {
                    b.report_spam(folder, ids, folders).await.map(|_| ())
                } else {
                    b.not_spam(folder, ids, folders).await.map(|_| ())
                }
            },
            move |result| Message::BulkDone {
                uids: uids.clone(),
                note,
                result,
            },
        )
    }

    /// "Not spam": the senders of these messages go to the allowed list.
    fn allow_senders_of(&mut self, uids: &[u32]) {
        let Some(acc) = self.config.account().cloned() else {
            return;
        };
        let addrs: Vec<String> = self
            .headers
            .iter()
            .chain(self.search_results.iter().flatten())
            .filter(|h| uids.contains(&h.uid) && !h.from_address.is_empty())
            .map(|h| h.from_address.clone())
            .collect();
        for a in addrs {
            let _ = Backend::set_sender_rule(&acc, &a, true);
        }
    }

    fn bulk(&mut self, action: Bulk) -> Task<Message> {
        let Some(folder) = self.current_folder.clone() else {
            return Task::none();
        };
        let uids = self.targets();
        if uids.is_empty() {
            return Task::none();
        }
        let b = self.backend.clone();
        let done = |note: &'static str, uids: Vec<u32>| {
            move |result| Message::BulkDone {
                uids: uids.clone(),
                note,
                result,
            }
        };
        match action {
            Bulk::Read | Bulk::Unread => {
                let seen = action == Bulk::Read;
                for h in self
                    .headers
                    .iter_mut()
                    .chain(self.search_results.iter_mut().flatten())
                {
                    if uids.contains(&h.uid) {
                        h.seen = seen;
                    }
                }
                self.perform(
                    async move { b.set_seen_many(folder, uids, seen).await },
                    Message::BulkSeen,
                )
            }
            Bulk::Star => {
                // Stars all, or unstars all when they already are.
                let on = !uids.iter().all(|u| {
                    self.headers
                        .iter()
                        .chain(self.search_results.iter().flatten())
                        .any(|h| h.uid == *u && h.flagged)
                });
                self.set_stars(&uids, on);
                self.perform(
                    async move { b.set_flagged_many(folder, uids, on).await },
                    Message::BulkSeen,
                )
            }
            Bulk::Spam => {
                let to_spam = self.current_kind() != FolderKind::Spam;
                if !to_spam {
                    self.allow_senders_of(&uids);
                }
                self.junk(folder, uids, to_spam, false)
            }
            Bulk::Archive => {
                if self.current_kind() == FolderKind::Archive {
                    return Task::none();
                }
                let folders = self.folders.clone();
                let ids = uids.clone();
                self.perform(
                    async move { b.archive_many(folder, ids, folders).await.map(|_| ()) },
                    done("bulk_archived", uids),
                )
            }
            Bulk::Delete => match self.folder_by_kind(FolderKind::Trash).map(|f| f.name.clone()) {
                Some(trash) if trash != folder => {
                    let ids = uids.clone();
                    self.perform(
                        async move { b.move_many(folder, ids, trash).await },
                        done("bulk_trashed", uids),
                    )
                }
                // In Trash: confirm, then delete for good.
                _ => {
                    self.bulk_delete = true;
                    self.confirm_delete = true;
                    Task::none()
                }
            },
        }
    }

    fn archive(&mut self, uid: u32) -> Task<Message> {
        let Some(folder) = self.current_folder.clone() else {
            return Task::none();
        };
        if self.current_kind() == FolderKind::Archive {
            return Task::none();
        }
        let b = self.backend.clone();
        let folders = self.folders.clone();
        self.perform(
            async move { b.archive(folder, uid, folders).await },
            move |result| Message::Archived { uid, result },
        )
    }

    fn delete(&mut self, uid: u32) -> Task<Message> {
        let Some(folder) = self.current_folder.clone() else {
            return Task::none();
        };
        match self.folder_by_kind(FolderKind::Trash).map(|f| f.name.clone()) {
            // Outside Trash: move to Trash (recoverable).
            Some(trash) if trash != folder => {
                let b = self.backend.clone();
                self.perform(
                    async move { b.move_to(folder, uid, trash).await },
                    move |result| Message::Done {
                        uid,
                        note: "moved_to_trash",
                        result,
                    },
                )
            }
            // In Trash (or no Trash folder): ask before deleting for good.
            _ => self.ask_delete_forever(uid),
        }
    }

    /// Shows the "delete forever?" dialog for a message (selecting/opening it
    /// first, since the dialog acts on the selected message).
    fn ask_delete_forever(&mut self, uid: u32) -> Task<Message> {
        let open = if self.selected != Some(uid) {
            self.open(uid)
        } else {
            Task::none()
        };
        self.confirm_delete = true;
        open
    }

    fn move_message(&mut self, uid: u32, target: String) -> Task<Message> {
        let Some(folder) = self.current_folder.clone().filter(|f| *f != target) else {
            return Task::none();
        };
        let b = self.backend.clone();
        let refresh = self.refresh_unseen();
        let mv = self.perform(
            async move { b.move_to(folder, uid, target).await },
            move |result| Message::Done {
                uid,
                note: "moved_ok",
                result,
            },
        );
        Task::batch([mv, refresh])
    }

    fn toggle_read(&mut self, uid: u32) -> Task<Message> {
        let Some(folder) = self.current_folder.clone() else {
            return Task::none();
        };
        let Some(seen) = self.header_mut(uid).map(|h| !h.seen) else {
            return Task::none();
        };
        if let Some(h) = self.headers.iter_mut().find(|h| h.uid == uid) {
            h.seen = seen;
        }
        if let Some(h) = self.search_results.iter_mut().flatten().find(|h| h.uid == uid) {
            h.seen = seen;
        }
        if let Some(n) = self.unseen.get_mut(&folder) {
            *n = if seen { n.saturating_sub(1) } else { *n + 1 };
        }
        let b = self.backend.clone();
        self.perform(
            async move { b.set_seen(folder, uid, seen).await },
            Message::SeenChanged,
        )
    }

    fn header_mut(&mut self, uid: u32) -> Option<&mut Header> {
        if self.headers.iter().any(|h| h.uid == uid) {
            self.headers.iter_mut().find(|h| h.uid == uid)
        } else {
            self.search_results.iter_mut().flatten().find(|h| h.uid == uid)
        }
    }

    /// Words of the search box that are not operators (see `ferrowl_core::query`).
    fn free_words(&self) -> Vec<String> {
        ferrowl_core::query::Query::parse(&self.search).words
    }

    /// Searches the local index (all cached messages of the folder, not only
    /// the ones loaded). Falls back to in-memory matching if the index has
    /// nothing (e.g. cache unavailable).
    fn run_search(&mut self) {
        let words = self.free_words();
        self.search_results = match (self.config.account(), &self.current_folder) {
            (Some(acc), Some(folder)) if !words.is_empty() => {
                let found = Backend::search(acc, folder, &words);
                (!found.is_empty() || crate::cache::get().is_some()).then_some(found)
            }
            _ => None,
        };
    }

    /// POP3: messages downloaded in the background go straight into the
    /// open list (it comes from the local mailbox, so this is instant).
    fn show_pop3_downloads(&mut self, folder: &str) -> Task<Message> {
        if self.current_folder.as_deref() == Some(folder)
            && let Some(account) = self.config.account().filter(|a| a.is_pop3())
        {
            self.headers = Backend::cached_headers(account, folder);
            return self.refresh_unseen();
        }
        Task::none()
    }

    /// One background indexing step, after a pause: the first one leaves the
    /// server to the user's own actions for a few seconds, the next ones are
    /// spaced out so busy servers are not hammered.
    fn index_step(&self, folder: String, before: u32, pause: Duration) -> Task<Message> {
        let b = self.backend.clone();
        let f = folder.clone();
        self.perform(
            async move {
                tokio::time::sleep(pause).await;
                b.index_older(f, before, 150).await
            },
            move |result| Message::Indexed {
                folder: folder.clone(),
                result,
            },
        )
    }

    /// Re-reads the OpenPGP state of the active account.
    fn refresh_pgp(&mut self) {
        self.refresh_pgp_for(self.config.active);
    }

    fn refresh_pgp_for(&mut self, i: usize) {
        let account = self.config.accounts.get(i);
        self.pgp_fpr = account
            .and_then(|a| crate::crypto::own_key(&a.email))
            .map(|k| crate::crypto::fingerprint(&k.to_public_key()));
        self.pgp_pass_draft = account.map(|a| a.pgp_passphrase.clone()).unwrap_or_default();
        self.pgp_contacts = crate::crypto::contact_count();
    }

    /// Logical width available to the message in the reader.
    pub fn html_width(&self) -> u32 {
        // Measured by the reader; until then sidebar 240 + list + padding.
        let w = self
            .reader_width
            // Less the page's 1 px border on each side.
            .map(|w| w - 2.0)
            .unwrap_or(self.window_width - 240.0 - self.list_width() - 80.0)
            .clamp(200.0, 820.0);
        (w as u32) / 4 * 4
    }

    /// Lays the open HTML message out again when the reader's width changed
    /// (stale requests are dropped by the renderer, so this can run on every
    /// resize step, like a browser does).
    fn relayout_html(&mut self) -> Task<Message> {
        match &self.html_render {
            Some(r) if r.width != self.html_width() => self.start_html_render(),
            _ => Task::none(),
        }
    }

    /// Renders the open message's HTML in the background, if it has one and
    /// the formatted view is selected.
    /// Sends a selection change to the HTML renderer, one at a time (drags
    /// send many: only the latest pending one is kept).
    fn html_select(&mut self, op: crate::html_view::Select) -> Task<Message> {
        use crate::html_view::Select;
        let Some(uid) = self.html_render.as_ref().map(|r| r.uid) else {
            return Task::none();
        };
        if self.html_select_busy {
            if let (Some(Select::Extend(..)), Select::Extend(..)) = (self.html_select_queue.last(), op) {
                self.html_select_queue.pop();
            }
            self.html_select_queue.push(op);
            return Task::none();
        }
        self.html_select_busy = true;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || crate::html_view::select(uid, op))
                    .await
                    .ok()
                    .flatten()
            },
            Message::HtmlSelected,
        )
    }

    fn start_html_render(&mut self) -> Task<Message> {
        let Some(msg) = &self.message else {
            return Task::none();
        };
        let Some(html) = msg
            .html
            .clone()
            .filter(|_| self.config.ui.prefer_html && self.html_failed != Some(msg.uid))
        else {
            return Task::none();
        };
        self.html_rendering = true;
        let uid = msg.uid;
        let job = crate::html_view::Job {
            uid: msg.uid,
            html,
            width: self.html_width(),
            scale: self.scale_factor,
            allow_remote: self.config.ui.load_remote_images || self.html_force_remote == Some(msg.uid),
            inline: msg.inline.clone(),
        };
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || crate::html_view::render(job))
                    .await
                    .unwrap_or(Err(crate::html_view::Unrendered::Failed))
            },
            move |rendered| Message::HtmlRendered(uid, rendered),
        )
    }

    fn reset_mailbox(&mut self) {
        self.folders.clear();
        self.headers.clear();
        self.search_results = None;
        self.indexed.clear();
        self.inbox_max_uid = None;
        self.current_folder = None;
        self.selected = None;
        self.message = None;
        self.html_render = None;
        self.hover_link = None;
        self.unseen.clear();
        self.confirm_delete = false;
        self.last_sync = None;
        self.pane = Pane::Mail;
        self.contacts.clear();
        self.contacts_query.clear();
        self.contact_selected = None;
        self.contacts_sync = SyncState::Idle;
        self.contact_form = None;
        self.confirm_contact_delete = false;
        self.agenda.clear();
        self.agenda_selected = None;
        self.calendar_sync = SyncState::Idle;
        // Its answer is dropped as stale: do not wait for it.
        self.due_adding = false;
    }

    /// Shows account `i` in the main window, with a fresh IMAP session.
    /// Results still in flight for the previous account go to the old backend
    /// and are ignored (see `stale`).
    fn switch_account(&mut self, i: usize) -> Task<Message> {
        self.config.active = i;
        self.refresh_pgp_for(i);
        self.generation += 1;
        self.backend = Backend::default();
        self.reset_mailbox();
        self.screen = Screen::Mail;
        self.connect()
    }

    fn connect(&mut self) -> Task<Message> {
        let Some(acc) = self.config.account().cloned() else {
            return Task::none();
        };
        self.conn = Conn::Connecting;
        let b = self.backend.clone();
        self.perform(async move { b.connect(acc).await }, Message::Connected)
    }

    /// Syncs the active account's contacts after `delay`.
    fn sync_contacts(&mut self, delay: Duration) -> Task<Message> {
        let Some(acc) = self.config.account().cloned() else {
            return Task::none();
        };
        self.contacts_sync = SyncState::Running;
        self.perform(
            async move {
                tokio::time::sleep(delay).await;
                Backend::sync_contacts(acc).await
            },
            Message::ContactsSynced,
        )
    }

    /// The invitation card for a message with a calendar part.
    fn invite_card(&self, msg: &FullMessage) -> Option<InviteCard> {
        let ics = msg.calendar.clone()?;
        let invitation = ferrowl_core::invite::read(&ics)?;
        let acc = self.config.account()?;
        Some(InviteCard {
            uid: msg.uid,
            reply: Backend::invitation_reply(acc, &invitation.uid),
            in_calendar: invitation.method == "CANCEL" && Backend::in_calendar(acc, &invitation.uid),
            ics,
            invitation,
            busy: false,
        })
    }

    /// Syncs the active account's calendars after `delay`.
    fn sync_calendar(&mut self, delay: Duration) -> Task<Message> {
        let Some(acc) = self.config.account().cloned() else {
            return Task::none();
        };
        self.calendar_sync = SyncState::Running;
        self.perform(
            async move {
                tokio::time::sleep(delay).await;
                Backend::sync_calendar(acc).await
            },
            Message::CalendarSynced,
        )
    }

    /// Reads the shown month's events from the cache, keeping the open one.
    fn reload_agenda(&mut self) {
        let Some(acc) = self.config.account() else {
            return;
        };
        let open = self
            .agenda_selected
            .and_then(|i| self.agenda.get(i))
            .map(|e| (e.occurrence.uid.clone(), e.occurrence.start));
        let from = ferrowl_core::ical::day_start(self.cal_month);
        let to = ferrowl_core::ical::day_start(shift_month(self.cal_month, 1));
        self.agenda = Backend::agenda(acc, from, to);
        self.agenda_selected = open.and_then(|(uid, start)| {
            self.agenda
                .iter()
                .position(|e| e.occurrence.uid == uid && e.occurrence.start == start)
        });
    }

    /// Reads the contacts list from the cache, keeping the open contact.
    fn reload_contacts(&mut self) {
        let Some(acc) = self.config.account() else {
            return;
        };
        let open = self
            .contact_selected
            .and_then(|i| self.contacts.get(i))
            .map(|c| c.url.clone());
        self.contacts = Backend::contacts(acc, &self.contacts_query);
        self.contact_selected = open.and_then(|url| self.contacts.iter().position(|c| c.url == url));
    }

    fn load_folder(&mut self, folder: String, silent: bool) -> Task<Message> {
        if !silent {
            self.loading_list = true;
            // Show the cached list right away; the server's answer replaces it.
            if self.headers.is_empty()
                && let Some(acc) = self.config.account()
            {
                self.headers = Backend::cached_headers(acc, &folder);
            }
        }
        let b = self.backend.clone();
        let f = folder.clone();
        self.perform(async move { b.headers(f).await }, move |result| {
            Message::HeadersLoaded {
                folder: folder.clone(),
                silent,
                result,
            }
        })
    }

    fn refresh_unseen(&self) -> Task<Message> {
        let mut names: Vec<String> = self
            .folders
            .iter()
            .filter(|f| matches!(f.kind, FolderKind::Inbox | FolderKind::Spam | FolderKind::Drafts))
            .map(|f| f.name.clone())
            .collect();
        if let Some(c) = &self.current_folder
            && !names.contains(c)
        {
            names.push(c.clone());
        }
        let b = self.backend.clone();
        self.perform(async move { b.unseen(names).await }, Message::Unseen)
    }

    pub fn folder_by_kind(&self, kind: FolderKind) -> Option<&Folder> {
        self.folders.iter().find(|f| f.kind == kind)
    }

    pub fn folder_label(&self, name: &str) -> String {
        self.folders
            .iter()
            .find(|f| f.name == name)
            .map(Folder::display)
            .unwrap_or_else(|| name.to_string())
    }

    pub fn current_kind(&self) -> FolderKind {
        self.current_folder
            .as_ref()
            .and_then(|c| self.folders.iter().find(|f| &f.name == c))
            .map(|f| f.kind)
            .unwrap_or(FolderKind::Other)
    }

    /// Messages visible after filter and search (`from:`, `has:attachment`, `is:unread`).
    /// The conversation the message belongs to, oldest first (just the
    /// message itself when conversations are off or it has no replies).
    pub fn conversation(&self, uid: u32) -> Vec<&Header> {
        let list = self.search_results.as_ref().unwrap_or(&self.headers);
        if !self.config.ui.threaded {
            return list.iter().filter(|h| h.uid == uid).collect();
        }
        ferrowl_core::threads::group(list)
            .into_iter()
            .find(|t| t.members.iter().any(|&i| list[i].uid == uid))
            .map(|t| t.members.iter().rev().map(|&i| &list[i]).collect())
            .unwrap_or_default()
    }

    pub fn visible(&self) -> Vec<&Header> {
        // Operators and free words, as in the mobile apps (ferrowl_core::query).
        let query = ferrowl_core::query::Query::parse(&self.search);
        // Free words were already matched by the full-text index.
        let indexed = self.search_results.is_some();
        let by_tab = self.tabs_shown() && self.search.trim().is_empty();
        self.search_results
            .as_ref()
            .unwrap_or(&self.headers)
            .iter()
            // Gmail inbox: only the open tab's messages (not while searching).
            .filter(|h| !by_tab || h.category == self.category)
            .filter(|h| match self.filter {
                ListFilter::All => true,
                ListFilter::Unread => !h.seen,
                ListFilter::Starred => h.flagged,
                ListFilter::Attachments => h.has_attachment,
            })
            .filter(|h| query.matches(h) && (indexed || query.matches_words(h)))
            .collect()
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) -> Task<Message> {
        self.toast_seq += 1;
        let id = self.toast_seq;
        self.toast = Some(Toast {
            id,
            text: text.into(),
            error,
        });
        let secs = if error { 12 } else { 5 };
        Task::perform(tokio::time::sleep(Duration::from_secs(secs)), move |_| {
            Message::DismissToast(id)
        })
    }

    fn save_config(&mut self) -> Task<Message> {
        match self.config.save() {
            Ok(()) => Task::none(),
            Err(e) => self.toast(e.to_string(), true),
        }
    }

    fn open(&mut self, uid: u32) -> Task<Message> {
        let Some(folder) = self.current_folder.clone() else {
            return Task::none();
        };
        self.selected = Some(uid);
        // Its conversation opens in the list, showing its messages.
        self.open_threads.insert(uid);
        self.message = None;
        self.html_render = None;
        self.hover_link = None;
        self.html_rendering = false;
        self.loading_message = true;
        self.confirm_delete = false;
        let b = self.backend.clone();
        self.perform(async move { b.message(folder, uid).await }, Message::Loaded)
    }

    /// After removing a message from the list, opens the next one.
    fn remove_and_advance(&mut self, uid: u32) -> Task<Message> {
        let visible: Vec<u32> = self.visible().iter().map(|h| h.uid).collect();
        let pos = visible.iter().position(|&u| u == uid);
        self.headers.retain(|h| h.uid != uid);
        if let Some(r) = self.search_results.as_mut() {
            r.retain(|h| h.uid != uid);
        }
        if self.selected != Some(uid) {
            return Task::none();
        }
        self.selected = None;
        self.message = None;
        let next = pos.and_then(|p| {
            visible
                .get(p + 1)
                .or_else(|| p.checked_sub(1).and_then(|q| visible.get(q)))
                .copied()
        });
        match next {
            Some(n) => self.open(n),
            None => Task::none(),
        }
    }

    fn step(&mut self, delta: isize) -> Task<Message> {
        let visible: Vec<u32> = self.visible().iter().map(|h| h.uid).collect();
        if visible.is_empty() {
            return Task::none();
        }
        let idx = self
            .selected
            .and_then(|s| visible.iter().position(|&u| u == s))
            .map(|i| (i as isize + delta).clamp(0, visible.len() as isize - 1) as usize)
            .unwrap_or(0);
        if Some(visible[idx]) == self.selected {
            return Task::none();
        }
        self.open(visible[idx])
    }

    fn reply_draft(&self, msg: &FullMessage) -> Compose {
        let mut c = Compose::new(ComposeMode::Reply);
        c.rich = self.config.ui.rich_compose;
        c.to = msg.from_address.clone();
        c.subject = if msg.subject.to_lowercase().starts_with("re:") {
            msg.subject.clone()
        } else {
            format!("Re: {}", msg.subject)
        };
        let quoted: String = msg.body.lines().map(|l| format!("> {l}\n")).collect();
        let header = tf(
            "quote_header",
            &[("date", &ui::long_date(msg.timestamp)), ("name", &msg.from)],
        );
        c.body = text_editor::Content::with_text(&format!("\n\n{header}\n{quoted}"));
        // Encrypted conversation stays encrypted.
        c.encrypt = msg.pgp.as_ref().is_some_and(|p| p.encrypted) && self.pgp_fpr.is_some();
        c.in_reply_to = msg.message_id.clone();
        c.references = msg.references.clone();
        c
    }

    fn forward_draft(&self, msg: &FullMessage) -> Compose {
        let mut c = Compose::new(ComposeMode::Forward);
        c.rich = self.config.ui.rich_compose;
        c.subject = format!("Fwd: {}", msg.subject);
        c.body = text_editor::Content::with_text(&format!(
            "\n\n{}\n{}: {} <{}>\n{}: {}\n{}: {}\n{}: {}\n\n{}",
            t("forward_header"),
            t("from"),
            msg.from,
            msg.from_address,
            t("date"),
            ui::long_date(msg.timestamp),
            t("subject"),
            msg.subject,
            t("to"),
            msg.to,
            msg.body
        ));
        // The original attachments are saved to a temporary folder and re-attached.
        let dir = std::env::temp_dir().join(format!("ferrowl-fwd-{}", msg.uid));
        if !msg.attachments.is_empty() && std::fs::create_dir_all(&dir).is_ok() {
            for a in &msg.attachments {
                let path = dir.join(safe_name(&a.name));
                if std::fs::write(&path, &a.data).is_ok() {
                    c.attachments.push(path);
                }
            }
        }
        c
    }

    // ------------------------------------------------------------ update

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Scoped(generation, inner) => {
                if generation == self.generation {
                    self.update(*inner)
                } else {
                    Task::none()
                }
            }
            Message::Connected(Ok(folders)) => {
                self.conn = Conn::Online;
                self.gmail = self.backend.is_gmail();
                self.folders = folders;
                let start = self
                    .folder_by_kind(FolderKind::Inbox)
                    .or(self.folders.iter().find(|f| f.selectable))
                    .map(|f| f.name.clone());
                let still_there = self
                    .current_folder
                    .as_ref()
                    .is_some_and(|c| self.folders.iter().any(|f| &f.name == c));
                if !still_there {
                    self.current_folder = start;
                    self.headers.clear();
                    self.selected = None;
                    self.message = None;
                }
                let load = match self.current_folder.clone() {
                    Some(f) => self.load_folder(f, false),
                    None => Task::none(),
                };
                // The address book learns who we write to from Sent: read its
                // newest page once, after the inbox, in the background.
                let sent = match self.folder_by_kind(FolderKind::Sent).map(|f| f.name.clone()) {
                    Some(sent) if self.current_folder.as_deref() != Some(&sent) => {
                        let b = self.backend.clone();
                        Task::perform(
                            async move {
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                let _ = b.headers(sent).await;
                            },
                            |_| Message::Noop,
                        )
                    }
                    _ => Task::none(),
                };
                // Contacts from the CardDAV server, quietly: most mail
                // accounts have none, and that is not an error to show.
                let contacts = if self.contacts_sync == SyncState::Idle {
                    self.sync_contacts(Duration::from_secs(8))
                } else {
                    Task::none()
                };
                let calendar = if self.calendar_sync == SyncState::Idle {
                    self.sync_calendar(Duration::from_secs(10))
                } else {
                    Task::none()
                };
                Task::batch([load, self.refresh_unseen(), sent, contacts, calendar])
            }
            Message::Connected(Err(e)) => {
                self.conn = Conn::Error(e.clone());
                self.toast(e, true)
            }
            Message::Unseen(Ok(map)) => {
                self.unseen.extend(map);
                self.update_badge()
            }
            Message::OtherUnread(email, n) => {
                self.other_unread.insert(email, n);
                self.update_badge()
            }
            Message::Unseen(Err(_)) => Task::none(),
            Message::Check(uids, on) => {
                if self.modifiers.shift()
                    && let Some(&first) = uids.first()
                {
                    self.check_range(first);
                    return Task::none();
                }
                self.anchor = uids.first().copied();
                for uid in uids {
                    if on {
                        self.checked.insert(uid);
                    } else {
                        self.checked.remove(&uid);
                    }
                }
                Task::none()
            }
            Message::SelectBy(kind) => {
                self.popup = None;
                let pick: Vec<u32> = self
                    .visible()
                    .iter()
                    .filter(|h| match kind {
                        Select::All => true,
                        Select::None => false,
                        Select::Read => h.seen,
                        Select::Unread => !h.seen,
                        Select::Starred => h.flagged,
                    })
                    .map(|h| h.uid)
                    .collect();
                self.checked = pick.into_iter().collect();
                Task::none()
            }
            Message::Bulk(action) => self.bulk(action),
            Message::BulkMove(target) => {
                self.popup = None;
                let Some(folder) = self.current_folder.clone().filter(|f| *f != target.name) else {
                    return Task::none();
                };
                let uids = self.targets();
                if uids.is_empty() {
                    return Task::none();
                }
                let b = self.backend.clone();
                let ids = uids.clone();
                self.perform(
                    async move { b.move_many(folder, ids, target.name).await },
                    move |result| Message::BulkDone {
                        uids: uids.clone(),
                        note: "bulk_moved",
                        result,
                    },
                )
            }
            Message::BulkSeen(result) => match result {
                Ok(()) => {
                    self.checked.clear();
                    self.refresh_unseen()
                }
                Err(e) => self.toast(e, true),
            },
            Message::BulkDone { uids, note, result } => match result {
                Ok(()) => {
                    self.headers.retain(|h| !uids.contains(&h.uid));
                    if let Some(r) = self.search_results.as_mut() {
                        r.retain(|h| !uids.contains(&h.uid));
                    }
                    if self.selected.is_some_and(|u| uids.contains(&u)) {
                        self.selected = None;
                        self.message = None;
                    }
                    self.checked.clear();
                    let note = tf(note, &[("n", &uids.len().to_string())]);
                    Task::batch([self.toast(note, false), self.refresh_unseen()])
                }
                Err(e) => self.toast(e, true),
            },
            Message::SelectFolder(name) => {
                self.page = 0;
                self.pane = Pane::Mail;
                self.checked.clear();
                self.open_threads.clear();
                self.appearance_open = false;
                self.screen = Screen::Mail;
                if self.current_folder.as_deref() == Some(&name) {
                    return Task::none();
                }
                self.current_folder = Some(name.clone());
                self.headers.clear();
                self.search_results = None;
                self.selected = None;
                self.message = None;
                self.confirm_delete = false;
                self.load_folder(name, false)
            }
            Message::ToggleNode(id) => {
                if !self.expanded.remove(&id) {
                    self.expanded.insert(id);
                }
                Task::none()
            }
            Message::HeadersLoaded {
                folder,
                silent,
                result,
            } => {
                if self.current_folder.as_deref() != Some(&folder) {
                    return Task::none();
                }
                self.loading_list = false;
                match result {
                    Ok((mut headers, first_seq)) => {
                        self.conn = Conn::Online;
                        // Sender lists: blocked mail leaves the inbox for
                        // Spam, allowed mail leaves Spam for the inbox.
                        let kind = self.folders.iter().find(|f| f.name == folder).map(|f| f.kind);
                        let ruled = match (kind, self.config.account()) {
                            (Some(k @ (FolderKind::Inbox | FolderKind::Spam)), Some(acc)) => {
                                let to_spam = k == FolderKind::Inbox;
                                let uids = Backend::ruled(acc, &headers, to_spam);
                                headers.retain(|h| !uids.contains(&h.uid));
                                self.junk(folder.clone(), uids, to_spam, true)
                            }
                            _ => Task::none(),
                        };
                        let (notify, rules) = if self
                            .folder_by_kind(FolderKind::Inbox)
                            .is_some_and(|f| f.name == folder)
                        {
                            (
                                self.note_new_mail(&headers),
                                self.apply_rules(folder.clone(), &headers),
                            )
                        } else {
                            (Task::none(), Task::none())
                        };
                        self.headers = headers;
                        self.last_sync = Some(chrono::Local::now());
                        self.run_search();
                        let index = if self.indexed.insert(folder.clone()) {
                            self.index_step(folder, first_seq, INDEX_FIRST_PAUSE)
                        } else {
                            Task::none()
                        };
                        Task::batch([self.refresh_unseen(), index, notify, ruled, rules])
                    }
                    Err(e) => {
                        if !silent {
                            self.conn = Conn::Error(e.clone());
                        }
                        self.toast(e, true)
                    }
                }
            }
            Message::Sync => match (&self.conn, self.current_folder.clone()) {
                (Conn::Online, Some(f)) => self.load_folder(f, false),
                (Conn::Connecting, _) => Task::none(),
                _ => self.connect(),
            },
            Message::Tick => match (&self.conn, self.current_folder.clone()) {
                (Conn::Online, Some(f)) if !self.loading_list => self.load_folder(f, true),
                _ => Task::none(),
            },
            Message::Search(s) => {
                self.page = 0;
                self.search = s;
                self.run_search();
                Task::none()
            }
            Message::Push(generation) => {
                let Some(inbox) = self.folder_by_kind(FolderKind::Inbox).map(|f| f.name.clone()) else {
                    return Task::none();
                };
                if generation != self.generation || self.conn != Conn::Online {
                    return Task::none();
                }
                if self.current_folder.as_deref() == Some(&inbox) {
                    if self.loading_list {
                        return Task::none();
                    }
                    return self.load_folder(inbox, true);
                }
                let b = self.backend.clone();
                self.perform(async move { b.headers(inbox).await }, Message::PushChecked)
            }
            Message::PushChecked(Ok((headers, _))) => {
                let notify = self.note_new_mail(&headers);
                let rules = match self.folder_by_kind(FolderKind::Inbox) {
                    Some(f) => self.apply_rules(f.name.clone(), &headers),
                    None => Task::none(),
                };
                Task::batch([notify, rules, self.refresh_unseen()])
            }
            Message::PushChecked(Err(_)) => Task::none(),
            Message::OtherMail(account, mails) => {
                if !self.config.ui.notifications {
                    return Task::none();
                }
                let (summary, body) = match mails.as_slice() {
                    [(from, subject)] => (format!("{from} · {account}"), subject.clone()),
                    many => (
                        format!(
                            "{} · {account}",
                            tf("notif_many", &[("n", &many.len().to_string())])
                        ),
                        many.iter()
                            .take(3)
                            .map(|(from, subject)| format!("{from} — {subject}"))
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                };
                Task::perform(
                    async move {
                        let _ = tokio::task::spawn_blocking(move || crate::push::notify(summary, body)).await;
                    },
                    |_| Message::Noop,
                )
            }
            Message::PgpGenerate => {
                let Some(a) = self.config.account().cloned() else {
                    return Task::none();
                };
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || -> Res<String> {
                            let key = crate::crypto::generate(&a.display_name, &a.email, &a.pgp_passphrase)
                                .map_err(|e| format!("{e:#}"))?;
                            crate::crypto::save_own_key(&a.email, &key).map_err(|e| format!("{e:#}"))?;
                            Ok(t("pgp_generated").to_string())
                        })
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()))
                    },
                    Message::PgpDone,
                )
            }
            Message::PgpImportOwn => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .add_filter("OpenPGP", &["asc", "gpg", "pgp", "key"])
                        .pick_file()
                        .await
                        .map(|f| f.path().to_path_buf())
                },
                Message::PgpImportOwnPicked,
            ),
            Message::PgpImportOwnPicked(Some(path)) => {
                let Some(a) = self.config.account().cloned() else {
                    return Task::none();
                };
                let result = crate::crypto::import_own_key(&a.email, &path)
                    .map_err(|e| format!("{e:#}"))
                    .and_then(|key| {
                        // The saved passphrase must unlock the key, or signing would fail later.
                        crate::crypto::sign(b"check", &key, &a.pgp_passphrase)
                            .map(|_| t("pgp_imported_own").to_string())
                            .map_err(|_| t("pgp_passphrase_mismatch").to_string())
                    });
                self.update(Message::PgpDone(result))
            }
            Message::PgpImportOwnPicked(None) | Message::PgpImportContactsPicked(None) => Task::none(),
            Message::PgpExport => {
                let Some(a) = self.config.account() else {
                    return Task::none();
                };
                let result = match crate::crypto::own_public_armored(&a.email) {
                    Some(armored) => save_attachment(&format!("{}.asc", a.email), armored.as_bytes())
                        .map(|path| tf("pgp_exported", &[("path", &path.display().to_string())]))
                        .map_err(|e| e.to_string()),
                    None => Err(t("err_pgp_no_own_key").to_string()),
                };
                self.update(Message::PgpDone(result))
            }
            Message::PgpImportContacts => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .add_filter("OpenPGP", &["asc", "gpg", "pgp", "key"])
                        .pick_files()
                        .await
                        .map(|fs| fs.into_iter().map(|f| f.path().to_path_buf()).collect())
                },
                Message::PgpImportContactsPicked,
            ),
            Message::PgpImportContactsPicked(Some(paths)) => {
                let mut addresses = Vec::new();
                let mut errors = Vec::new();
                for p in paths {
                    match crate::crypto::import_contact_keys(&p) {
                        Ok(a) => addresses.extend(a),
                        Err(e) => errors.push(format!("{}: {e:#}", p.display())),
                    }
                }
                let result = if addresses.is_empty() {
                    Err(errors.join("\n"))
                } else {
                    Ok(tf("pgp_imported", &[("list", &addresses.join(", "))]))
                };
                self.update(Message::PgpDone(result))
            }
            Message::PgpPassphrase(v) => {
                self.pgp_pass_draft = v;
                Task::none()
            }
            Message::PgpSavePassphrase => {
                let i = self.config.active;
                if let Some(a) = self.config.accounts.get_mut(i) {
                    a.pgp_passphrase = self.pgp_pass_draft.clone();
                    self.draft.pgp_passphrase = a.pgp_passphrase.clone();
                }
                let save = self.save_config();
                // If a key exists, tell right away whether the passphrase unlocks it.
                let check = match self.config.account() {
                    Some(a) => match crate::crypto::own_key(&a.email) {
                        Some(k) if crate::crypto::sign(b"check", &k, &a.pgp_passphrase).is_err() => {
                            self.toast(t("pgp_passphrase_mismatch"), true)
                        }
                        _ => self.toast(t("pgp_passphrase_saved"), false),
                    },
                    None => Task::none(),
                };
                Task::batch([save, check])
            }
            Message::PgpDone(result) => {
                self.refresh_pgp();
                match result {
                    Ok(msg) => self.toast(msg, false),
                    Err(e) => self.toast(e, true),
                }
            }
            Message::ComposeToggleSign => {
                if let Some(c) = self.compose.as_mut() {
                    c.sign = !c.sign;
                }
                Task::none()
            }
            Message::ComposeToggleEncrypt => {
                if let Some(c) = self.compose.as_mut() {
                    c.encrypt = !c.encrypt;
                }
                Task::none()
            }
            Message::ToggleNotifications(on) => {
                self.config.ui.notifications = on;
                self.save_config()
            }
            Message::Indexed { folder, result } => match result {
                // Only the open folder keeps indexing (one step at a time,
                // so user actions are not stuck behind a long download).
                Ok(Some(next)) if self.current_folder.as_deref() == Some(&folder) => {
                    let shown = self.show_pop3_downloads(&folder);
                    Task::batch([shown, self.index_step(folder, next, INDEX_PAUSE)])
                }
                Ok(Some(_)) => {
                    self.indexed.remove(&folder);
                    Task::none()
                }
                Ok(None) => {
                    let shown = self.show_pop3_downloads(&folder);
                    self.run_search();
                    shown
                }
                Err(_) => {
                    self.indexed.remove(&folder);
                    Task::none()
                }
            },
            Message::SetFilter(f) => {
                self.filter = f;
                self.page = 0;
                Task::none()
            }
            // Shift+click: select a range; Ctrl/Cmd+click: add or remove one
            // row; plain click: open.
            Message::Open(uid) if self.modifiers.shift() => {
                self.check_range(uid);
                Task::none()
            }
            Message::Open(uid) if self.modifiers.command() => {
                let row = self
                    .rows()
                    .into_iter()
                    .find(|r| r.contains(&uid))
                    .unwrap_or(vec![uid]);
                let on = !row.iter().all(|u| self.checked.contains(u));
                for u in row {
                    if on {
                        self.checked.insert(u);
                    } else {
                        self.checked.remove(&u);
                    }
                }
                self.anchor = Some(uid);
                Task::none()
            }
            Message::Open(uid) => {
                self.anchor = Some(uid);
                self.open(uid)
            }
            Message::ToggleThread(uids) => {
                if uids.iter().any(|u| self.open_threads.contains(u)) {
                    for u in &uids {
                        self.open_threads.remove(u);
                    }
                } else if let Some(&u) = uids.first() {
                    self.open_threads.insert(u);
                }
                Task::none()
            }
            Message::ToggleStar(uids, on) => {
                let Some(folder) = self.current_folder.clone() else {
                    return Task::none();
                };
                self.set_stars(&uids, on);
                let b = self.backend.clone();
                self.perform(
                    async move { b.set_flagged_many(folder, uids, on).await },
                    Message::StarSaved,
                )
            }
            Message::OpenPopup(kind) => {
                self.popup = Some((kind, self.cursor));
                Task::none()
            }
            Message::ClosePopup => {
                self.popup = None;
                Task::none()
            }
            Message::Page(delta) => {
                let size = self.config.ui.page_size.max(1) as usize;
                let rows = self.list_rows().len();
                let next = (self.page as i64 + delta as i64).max(0) as usize;
                // Past the loaded messages: take older ones from the local copy.
                if next * size >= rows
                    && let (Some(acc), Some(folder)) = (self.config.account(), self.current_folder.clone())
                {
                    let want = (self.headers.len() + 5 * size) as u32;
                    let older = Backend::cached_headers_up_to(acc, &folder, want);
                    let known: HashSet<u32> = self.headers.iter().map(|h| h.uid).collect();
                    self.headers
                        .extend(older.into_iter().filter(|h| !known.contains(&h.uid)));
                }
                if next * size < self.list_rows().len() {
                    self.page = next;
                }
                operation::snap_to(LIST_ID, iced::widget::scrollable::RelativeOffset::START)
            }
            Message::ToggleLabel(label, on) => {
                self.popup = None;
                let Some(folder) = self.current_folder.clone() else {
                    return Task::none();
                };
                let uids = self.targets();
                for h in self
                    .headers
                    .iter_mut()
                    .chain(self.search_results.iter_mut().flatten())
                    .filter(|h| uids.contains(&h.uid))
                {
                    h.labels.retain(|l| *l != label);
                    if on {
                        h.labels.push(label.clone());
                    }
                }
                let b = self.backend.clone();
                self.perform(
                    async move { b.set_label_many(folder, uids, label, on).await },
                    Message::LabelSaved,
                )
            }
            Message::LabelSaved(Ok(())) => Task::none(),
            Message::LabelSaved(Err(e)) => {
                let reload = match self.current_folder.clone() {
                    Some(f) => self.load_folder(f, true),
                    None => Task::none(),
                };
                Task::batch([self.toast(e, true), reload])
            }
            Message::SetCategory(c) => {
                self.category = c;
                self.page = 0;
                Task::none()
            }
            Message::GoPage(n) => {
                self.popup = None;
                self.page = n;
                operation::snap_to(LIST_ID, iced::widget::scrollable::RelativeOffset::START)
            }
            Message::SetPageSize(n) => {
                self.config.ui.page_size = n;
                self.page = 0;
                self.save_config()
            }
            Message::SetSort(order) => {
                self.page = 0;
                self.popup = None;
                self.config.ui.sort = order;
                self.save_config()
            }
            Message::SpamOpen => {
                let (Some(uid), Some(folder)) = (self.selected, self.current_folder.clone()) else {
                    return Task::none();
                };
                let to_spam = self.current_kind() != FolderKind::Spam;
                if !to_spam {
                    self.allow_senders_of(&[uid]);
                }
                self.junk(folder, vec![uid], to_spam, false)
            }
            Message::BlockSender => {
                let (Some(msg), Some(folder), Some(acc)) = (
                    self.message.clone(),
                    self.current_folder.clone(),
                    self.config.account().cloned(),
                ) else {
                    return Task::none();
                };
                if msg.from_address.is_empty() {
                    return Task::none();
                }
                if let Err(e) = Backend::set_sender_rule(&acc, &msg.from_address, false) {
                    return self.toast(e, true);
                }
                // This message and the sender's other loaded ones.
                let addr = msg.from_address.to_lowercase();
                let mut uids: Vec<u32> = self
                    .headers
                    .iter()
                    .filter(|h| h.from_address.to_lowercase() == addr)
                    .map(|h| h.uid)
                    .collect();
                if !uids.contains(&msg.uid) {
                    uids.push(msg.uid);
                }
                let who = if msg.from.is_empty() {
                    msg.from_address.clone()
                } else {
                    msg.from.clone()
                };
                let toast = self.toast(tf("sender_blocked", &[("who", &who)]), false);
                let move_ = if self.current_kind() == FolderKind::Spam {
                    Task::none()
                } else {
                    self.junk(folder, uids, true, true)
                };
                Task::batch([toast, move_])
            }
            Message::RulesApplied(n, result) => match result {
                Ok(_) => Task::batch([
                    self.toast(tf("rules_moved", &[("n", &n.to_string())]), false),
                    self.refresh_unseen(),
                ]),
                Err(e) => self.toast(e, true),
            },
            Message::RuleNew => {
                let id = format!("{:016x}", rand::random::<u64>());
                let rule = ferrowl_core::rules::Rule {
                    id,
                    name: String::new(),
                    condition: String::new(),
                    actions: vec![ferrowl_core::rules::Action::MarkRead],
                    enabled: true,
                };
                self.rule_edit = Some((rule, true));
                self.rule_preview.clear();
                Task::none()
            }
            Message::RuleEdit(id) => {
                let Some(acc) = self.config.account() else {
                    return Task::none();
                };
                if let Some(rule) = Backend::rules(acc).into_iter().find(|r| r.id == id) {
                    self.rule_preview = rule_preview(acc, &rule.condition);
                    self.rule_edit = Some((rule, false));
                }
                Task::none()
            }
            Message::RuleCondition(text) => {
                if let (Some((rule, _)), Some(acc)) = (self.rule_edit.as_mut(), self.config.account()) {
                    self.rule_preview = rule_preview(acc, &text);
                    rule.condition = text;
                }
                Task::none()
            }
            Message::RuleName(text) => {
                if let Some((rule, _)) = self.rule_edit.as_mut() {
                    rule.name = text;
                }
                Task::none()
            }
            Message::RuleAction(action, on) => {
                if let Some((rule, _)) = self.rule_edit.as_mut() {
                    rule.actions.retain(|a| a != &action);
                    if on {
                        rule.actions.push(action);
                    }
                }
                Task::none()
            }
            Message::RuleDestination(target) => {
                use ferrowl_core::rules::Action;
                if let Some((rule, _)) = self.rule_edit.as_mut() {
                    rule.actions
                        .retain(|a| !matches!(a, Action::Archive | Action::MoveTo(_)));
                    rule.actions.extend(target);
                }
                Task::none()
            }
            Message::RuleEnabled(on) => {
                if let Some((rule, _)) = self.rule_edit.as_mut() {
                    rule.enabled = on;
                }
                Task::none()
            }
            Message::RuleCancel => {
                self.rule_edit = None;
                self.rule_preview.clear();
                Task::none()
            }
            Message::RuleSave => {
                let (Some((rule, _)), Some(acc)) = (self.rule_edit.as_ref(), self.config.account()) else {
                    return Task::none();
                };
                let mut rule = rule.clone();
                rule.condition = rule.condition.trim().to_string();
                rule.name = rule.name.trim().to_string();
                if rule.condition.is_empty() || rule.actions.is_empty() {
                    return Task::none();
                }
                match Backend::save_rule(acc, &rule) {
                    Ok(()) => {
                        self.rule_edit = None;
                        self.rule_preview.clear();
                        self.toast(t("rules_saved").to_string(), false)
                    }
                    Err(e) => self.toast(e, true),
                }
            }
            Message::RuleDelete(id) => {
                let Some(acc) = self.config.account() else {
                    return Task::none();
                };
                if self.rule_edit.as_ref().is_some_and(|(r, _)| r.id == id) {
                    self.rule_edit = None;
                }
                match Backend::delete_rule(acc, &id) {
                    Ok(()) => Task::none(),
                    Err(e) => self.toast(e, true),
                }
            }
            Message::RuleSwitch(id, on) => {
                let Some(acc) = self.config.account() else {
                    return Task::none();
                };
                let Some(mut rule) = Backend::rules(acc).into_iter().find(|r| r.id == id) else {
                    return Task::none();
                };
                rule.enabled = on;
                match Backend::save_rule(acc, &rule) {
                    Ok(()) => Task::none(),
                    Err(e) => self.toast(e, true),
                }
            }
            Message::MailRules(Ok(plan)) => {
                if plan.is_empty() {
                    return Task::none();
                }
                // Show what the rules did without waiting for the next sync.
                let inbox = self.folder_by_kind(FolderKind::Inbox).map(|f| f.name.clone());
                if inbox.is_some() && self.current_folder == inbox {
                    let gone = plan.removed();
                    self.headers.retain(|h| !gone.contains(&h.uid));
                    for h in &mut self.headers {
                        if plan.mark_read.contains(&h.uid) {
                            h.seen = true;
                        }
                        if plan.star.contains(&h.uid) {
                            h.flagged = true;
                        }
                    }
                    self.run_search();
                }
                let mut n: Vec<u32> = plan
                    .mark_read
                    .iter()
                    .chain(&plan.star)
                    .chain(&plan.removed())
                    .copied()
                    .collect();
                n.sort_unstable();
                n.dedup();
                Task::batch([
                    self.toast(tf("rules_applied", &[("n", &n.len().to_string())]), false),
                    self.refresh_unseen(),
                ])
            }
            Message::MailRules(Err(e)) => self.toast(e, true),
            Message::SenderInput(allow, text) => {
                if allow {
                    self.sender_input.1 = text;
                } else {
                    self.sender_input.0 = text;
                }
                Task::none()
            }
            Message::AddSender(allow) => {
                let Some(acc) = self.config.account().cloned() else {
                    return Task::none();
                };
                let text = if allow {
                    std::mem::take(&mut self.sender_input.1)
                } else {
                    std::mem::take(&mut self.sender_input.0)
                };
                let text = text.trim();
                if !text.contains('@') {
                    return Task::none();
                }
                match Backend::set_sender_rule(&acc, text, allow) {
                    Ok(()) => Task::none(),
                    Err(e) => self.toast(e, true),
                }
            }
            Message::RemoveSender(address) => {
                if let Some(acc) = self.config.account() {
                    Backend::remove_sender_rule(acc, &address);
                }
                Task::none()
            }
            Message::Print => {
                let Some(msg) = &self.message else {
                    return Task::none();
                };
                let labels = crate::print::Labels {
                    from: t("from"),
                    to: t("to"),
                    date: t("date"),
                    when: crate::ui::long_date(msg.timestamp),
                };
                match crate::print::open(msg, self.config.ui.load_remote_images, &labels) {
                    Ok(()) => Task::none(),
                    Err(e) => self.toast(e.to_string(), true),
                }
            }
            Message::CloseReader => {
                self.selected = None;
                self.message = None;
                self.html_render = None;
                self.invite = None;
                Task::none()
            }
            Message::StarSaved(Ok(())) => Task::none(),
            Message::StarSaved(Err(e)) => {
                let reload = match self.current_folder.clone() {
                    Some(f) => self.load_folder(f, true),
                    None => Task::none(),
                };
                Task::batch([self.toast(e, true), reload])
            }
            Message::Loaded(Ok(msg)) => {
                if Some(msg.uid) != self.selected {
                    return Task::none();
                }
                self.loading_message = false;
                if let Some(h) = self.headers.iter_mut().find(|h| h.uid == msg.uid) {
                    h.seen = true;
                }
                if let Some(h) = self
                    .search_results
                    .iter_mut()
                    .flatten()
                    .find(|h| h.uid == msg.uid)
                {
                    h.seen = true;
                }
                if let Some(c) = &self.current_folder
                    && let Some(n) = self.unseen.get_mut(c)
                {
                    *n = n.saturating_sub(1);
                }
                self.body = text_editor::Content::with_text(&msg.body);
                self.body_probe = text_editor::Content::with_text(&msg.body);
                self.body_link_hover = false;
                self.invite = self.invite_card(&msg);
                self.insights = Some((
                    msg.uid,
                    ferrowl_core::insights::find(&msg.subject, &msg.from_address, &msg.body),
                ));
                self.message = Some(msg);
                self.html_render = None;
                // A failure was for the message open then: a UID repeats in
                // other folders and accounts, and this one gets its own try.
                self.html_failed = None;
                self.hover_link = None;
                let top = operation::snap_to(READER_ID, iced::widget::scrollable::RelativeOffset::START);
                let pending = match self.pending_action.take() {
                    Some((uid, action)) if self.message.as_ref().is_some_and(|m| m.uid == uid) => {
                        self.run_action(uid, action)
                    }
                    _ => Task::none(),
                };
                self.thumbs.clear();
                self.preview = None;
                let thumbs = self.start_thumbnails();
                let dev_preview = if std::env::var_os("FERROWL_DEV_PREVIEW_ATT").is_some()
                    && self.message.as_ref().is_some_and(|m| !m.attachments.is_empty())
                {
                    self.update(Message::PreviewAttachment(0))
                } else {
                    Task::none()
                };
                Task::batch([top, self.start_html_render(), pending, thumbs, dev_preview])
            }
            Message::Loaded(Err(e)) => {
                self.loading_message = false;
                self.toast(e, true)
            }
            Message::ToggleHtml => {
                self.config.ui.prefer_html = !self.config.ui.prefer_html;
                let render = if self.html_render.is_none() {
                    self.start_html_render()
                } else {
                    Task::none()
                };
                Task::batch([self.save_config(), render])
            }
            // A newer request (resize, zoom) replaced it: that one answers.
            Message::HtmlRendered(_, Err(crate::html_view::Unrendered::Superseded)) => Task::none(),
            Message::HtmlRendered(uid, Err(crate::html_view::Unrendered::Failed)) => {
                // The HTML could not be painted: show the text instead.
                if self.message.as_ref().is_some_and(|m| m.uid == uid) {
                    self.html_rendering = false;
                    self.html_failed = Some(uid);
                }
                Task::none()
            }
            Message::HtmlRendered(_, Ok(rendered)) => {
                // Ignore results for a message that is no longer open.
                if self.message.as_ref().is_some_and(|m| m.uid == rendered.uid) {
                    self.html_rendering = false;
                    // First render of this message: start from the top (a re-render
                    // after a resize keeps the position).
                    let first = self.html_render.is_none();
                    self.html_render = Some(rendered);
                    // The reader changed width while this was being painted.
                    let again = self.relayout_html();
                    if first {
                        return Task::batch([
                            operation::snap_to(READER_ID, iced::widget::scrollable::RelativeOffset::START),
                            again,
                        ]);
                    }
                    return again;
                }
                Task::none()
            }
            Message::LoadRemoteImages => {
                self.html_force_remote = self.message.as_ref().map(|m| m.uid);
                self.start_html_render()
            }
            Message::ToggleRemoteImages(on) => {
                self.config.ui.load_remote_images = on;
                self.save_config()
            }
            Message::HtmlPointer(x, y) => {
                self.html_pointer = Some((x, y));
                if let Some(((px, py), moved)) = &mut self.html_press
                    && (*moved || (*px - x).abs() + (*py - y).abs() > 3.0)
                {
                    *moved = true;
                    return self.html_select(crate::html_view::Select::Extend(x, y));
                }
                if self
                    .hover_probe
                    .is_some_and(|(px, py)| (px - x).abs() + (py - y).abs() < 6.0)
                {
                    return Task::none();
                }
                self.hover_probe = Some((x, y));
                let Some(uid) = self.html_render.as_ref().map(|r| r.uid) else {
                    return Task::none();
                };
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || crate::html_view::link_at(uid, x, y))
                            .await
                            .ok()
                            .flatten()
                    },
                    Message::HoverLink,
                )
            }
            Message::HoverLink(link) => {
                self.hover_link = link;
                Task::none()
            }
            Message::HtmlLeave => {
                self.hover_link = None;
                self.html_pointer = None;
                self.hover_probe = None;
                Task::none()
            }
            Message::HtmlPress => {
                let Some((x, y)) = self.html_pointer else {
                    return Task::none();
                };
                self.html_press = Some(((x, y), false));
                let now = Instant::now();
                let double = self.html_last_press.is_some_and(|(at, (px, py))| {
                    now.duration_since(at) < Duration::from_millis(400)
                        && (px - x).abs() + (py - y).abs() < 6.0
                });
                self.html_last_press = (!double).then_some((now, (x, y)));
                self.html_select(if double {
                    crate::html_view::Select::Word(x, y)
                } else {
                    crate::html_view::Select::Start(x, y)
                })
            }
            Message::HtmlSelected(rendered) => {
                self.html_select_busy = false;
                if let (Some(new), Some(old)) = (rendered, &mut self.html_render)
                    && new.uid == old.uid
                    && new.width == old.width
                {
                    *old = new;
                }
                match self.html_select_queue.is_empty() {
                    true => Task::none(),
                    false => {
                        let next = self.html_select_queue.remove(0);
                        self.html_select(next)
                    }
                }
            }
            Message::CopyText(Some(text)) => iced::clipboard::write(text),
            Message::CopyText(None) => Task::none(),
            Message::WindowResized(size) => {
                self.window_width = size.width;
                self.window_height = size.height;
                self.relayout_html()
            }
            Message::ReaderWidth(w) => {
                self.reader_width = Some(w);
                self.relayout_html()
            }
            Message::Thumbs(uid, list) => {
                if self.message.as_ref().is_some_and(|m| m.uid == uid) {
                    self.thumbs = list.into_iter().collect();
                }
                Task::none()
            }
            Message::PreviewAttachment(i) => {
                let Some((uid, a)) = self
                    .message
                    .as_ref()
                    .and_then(|m| Some((m.uid, m.attachments.get(i)?.clone())))
                else {
                    return Task::none();
                };
                self.preview = Some((i, None));
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || crate::preview::build(&a.name, &a.data))
                            .await
                            .unwrap_or_else(|e| crate::preview::Content::Error(e.to_string()))
                    },
                    move |content| Message::PreviewReady(uid, i, content),
                )
            }
            Message::PreviewReady(uid, i, content) => {
                if self.message.as_ref().is_some_and(|m| m.uid == uid)
                    && self.preview.as_ref().is_some_and(|(j, _)| *j == i)
                {
                    self.preview = Some((i, Some(content)));
                }
                Task::none()
            }
            Message::ClosePreview => {
                self.preview = None;
                Task::none()
            }
            Message::Quit => quit(),
            Message::Raise => iced::window::latest().and_then(|id| {
                Task::batch([iced::window::minimize(id, false), iced::window::gain_focus(id)])
            }),
            Message::ListResizeStart => {
                self.list_resize = Some((self.cursor.x, self.list_width()));
                Task::none()
            }
            Message::CursorMoved(at) => {
                self.cursor = at;
                if let Some((x0, w0)) = self.list_resize {
                    // The list sits after the sidebar: in RTL it grows leftwards.
                    let dx = if i18n::is_rtl() { x0 - at.x } else { at.x - x0 };
                    self.config.ui.list_width = w0 + dx;
                    self.config.ui.list_width = self.list_width();
                }
                if let Some(d) = self.drag.as_mut()
                    && !d.active
                    && (at.x - d.start.x).abs() + (at.y - d.start.y).abs() > 8.0
                {
                    d.active = true;
                }
                Task::none()
            }
            Message::MouseDown => {
                // A press on a row may become a drag.
                if self.menu.is_none()
                    && self.compose.is_none()
                    && let Some(uid) = self.hover_row
                {
                    let label = self
                        .visible()
                        .iter()
                        .find(|h| h.uid == uid)
                        .map(|h| ui::trunc(&h.subject, 40))
                        .unwrap_or_default();
                    self.drag = Some(Drag {
                        uid,
                        start: self.cursor,
                        active: false,
                        label,
                    });
                }
                Task::none()
            }
            Message::MouseUp if self.html_press.is_some() => {
                // A click without a drag: follow the link under it.
                let Some((_, false)) = self.html_press.take() else {
                    return Task::none();
                };
                let (Some((x, y)), Some(uid)) = (self.html_pointer, self.html_render.as_ref().map(|r| r.uid))
                else {
                    return Task::none();
                };
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || crate::html_view::link_at(uid, x, y))
                            .await
                            .ok()
                            .flatten()
                    },
                    |link| match link {
                        Some(url) => Message::OpenLink(url),
                        None => Message::Noop,
                    },
                )
            }
            Message::MouseUp if self.list_resize.take().is_some() => {
                let save = self.save_config();
                Task::batch([save, self.relayout_html()])
            }
            Message::MouseUp => match (self.drag.take(), self.drop_folder.clone()) {
                (Some(d), Some(target)) if d.active => self.move_message(d.uid, target),
                _ => Task::none(),
            },
            // Keys and buttons released in another window never reach us: a
            // Ctrl or Shift left "held" would turn every click on the list
            // into a selection, so forget them when the focus goes away.
            Message::Unfocused => {
                self.modifiers = keyboard::Modifiers::default();
                self.drag = None;
                self.html_press = None;
                Task::none()
            }
            Message::RowEnter(uid) => {
                self.hover_row = Some(uid);
                Task::none()
            }
            Message::RowExit(uid) => {
                if self.hover_row == Some(uid) {
                    self.hover_row = None;
                }
                Task::none()
            }
            Message::FolderEnter(name) => {
                self.drop_folder = Some(name);
                Task::none()
            }
            Message::FolderExit(name) => {
                if self.drop_folder.as_deref() == Some(&name) {
                    self.drop_folder = None;
                }
                Task::none()
            }
            Message::OpenMenu(uid) => {
                self.drag = None;
                self.menu = Some(ContextMenu {
                    uid,
                    at: self.cursor,
                    folders: false,
                });
                Task::none()
            }
            Message::CloseMenu => {
                self.menu = None;
                Task::none()
            }
            Message::MenuAction(uid, action) => {
                if action == CtxAction::MoveMenu {
                    if let Some(m) = self.menu.as_mut() {
                        m.folders = true;
                    }
                    return Task::none();
                }
                self.menu = None;
                self.run_action(uid, action)
            }
            Message::ScaleFactor(f) => {
                self.scale_factor = f;
                Task::none()
            }
            Message::OpenLink(url) => {
                if !crate::html::is_openable(&url) {
                    return Task::none();
                }
                if let Some(rest) = url.strip_prefix("mailto:") {
                    if self.compose.is_none() && self.config.account().is_some() {
                        let mut c = Compose::new(ComposeMode::New);
                        c.rich = self.config.ui.rich_compose;
                        let (to, query) = rest.split_once('?').unwrap_or((rest, ""));
                        c.to = to.to_string();
                        for (k, v) in query.split('&').filter_map(|kv| kv.split_once('=')) {
                            if k.eq_ignore_ascii_case("subject") {
                                c.subject = percent_decode(v);
                            }
                        }
                        self.compose = Some(c);
                    }
                    return Task::none();
                }
                if let Err(e) = open::that_detached(&url) {
                    return self.toast(e.to_string(), true);
                }
                Task::none()
            }
            Message::BodyHover(point) => {
                // Hand cursor over links: place the hidden copy's cursor
                // under the mouse and look for a link there.
                self.body_probe.perform(text_editor::Action::Click(point));
                let at = self.body_probe.cursor().position;
                self.body_link_hover = self
                    .body_probe
                    .line(at.line)
                    .is_some_and(|l| crate::linkify::at(&l.text, at.column).is_some());
                Task::none()
            }
            Message::BodyLeave => {
                self.body_link_hover = false;
                Task::none()
            }
            Message::BodyAction(action) => {
                // Read-only text: selection and copy allowed, edits not.
                if action.is_edit() {
                    return Task::none();
                }
                let click = matches!(action, text_editor::Action::Click(_));
                self.body.perform(action);
                // A click on a link opens it.
                if click {
                    let at = self.body.cursor().position;
                    if let Some(url) = self
                        .body
                        .line(at.line)
                        .and_then(|l| crate::linkify::at(&l.text, at.column))
                    {
                        return self.update(Message::OpenLink(url));
                    }
                }
                Task::none()
            }
            Message::Reply => {
                if let Some(m) = &self.message {
                    self.compose = Some(self.reply_draft(m));
                }
                Task::none()
            }
            Message::ReplyAll => {
                if let Some(m) = &self.message {
                    let mut c = self.reply_draft(m);
                    // Everyone else on the thread, without ourselves or duplicates.
                    let me = self
                        .config
                        .account()
                        .map(|a| a.email.to_lowercase())
                        .unwrap_or_default();
                    let mut seen: HashSet<String> = [me, m.from_address.to_lowercase()].into_iter().collect();
                    let mut keep = |list: &[String]| -> Vec<String> {
                        list.iter()
                            .filter(|a| seen.insert(a.to_lowercase()))
                            .cloned()
                            .collect()
                    };
                    let to = keep(&m.to_addrs);
                    let cc = keep(&m.cc_addrs);
                    if !to.is_empty() {
                        c.to = format!("{}, {}", c.to, to.join(", "));
                    }
                    if !cc.is_empty() {
                        c.cc = cc.join(", ");
                        c.show_cc = true;
                    }
                    self.compose = Some(c);
                }
                Task::none()
            }
            Message::Forward => {
                if let Some(m) = &self.message {
                    self.compose = Some(self.forward_draft(m));
                }
                Task::none()
            }
            Message::Archive => match self.selected {
                Some(uid) => self.archive(uid),
                None => Task::none(),
            },
            Message::Archived { uid, result } => match result {
                Ok(target) => {
                    if self.folder_by_kind(FolderKind::Archive).is_none() {
                        // Folder just created: reload the folder list in the background.
                        let b = self.backend.clone();
                        let acc = self.config.account().cloned();
                        let _ = target;
                        let reload = match acc {
                            Some(a) => self.perform(async move { b.connect(a).await }, Message::Connected),
                            None => Task::none(),
                        };
                        let next = self.remove_and_advance(uid);
                        let toast = self.toast(t("archived"), false);
                        return Task::batch([reload, next, toast]);
                    }
                    let next = self.remove_and_advance(uid);
                    Task::batch([next, self.toast(t("archived"), false)])
                }
                Err(e) => self.toast(e, true),
            },
            Message::Delete => match self.selected {
                Some(uid) => self.delete(uid),
                None => Task::none(),
            },
            Message::ConfirmDelete(yes) if self.bulk_delete => {
                self.confirm_delete = false;
                self.bulk_delete = false;
                let (true, Some(folder)) = (yes, self.current_folder.clone()) else {
                    return Task::none();
                };
                let uids = self.targets();
                let b = self.backend.clone();
                let ids = uids.clone();
                self.perform(
                    async move { b.delete_forever_many(folder, ids).await },
                    move |result| Message::BulkDone {
                        uids: uids.clone(),
                        note: "bulk_deleted_forever",
                        result,
                    },
                )
            }
            Message::ConfirmDelete(yes) => {
                self.confirm_delete = false;
                let (true, Some(uid), Some(folder)) = (yes, self.selected, self.current_folder.clone())
                else {
                    return Task::none();
                };
                let b = self.backend.clone();
                self.perform(
                    async move { b.delete_forever(folder, uid).await },
                    move |result| Message::Done {
                        uid,
                        note: "deleted_forever",
                        result,
                    },
                )
            }
            Message::Done { uid, note, result } => match result {
                Ok(()) => {
                    let next = self.remove_and_advance(uid);
                    Task::batch([next, self.toast(t(note), false)])
                }
                Err(e) => self.toast(e, true),
            },
            Message::ToggleRead => match self.selected {
                Some(uid) => self.toggle_read(uid),
                None => Task::none(),
            },
            Message::SeenChanged(Ok(())) => Task::none(),
            Message::SeenChanged(Err(e)) => self.toast(e, true),
            Message::SaveAttachment(i) => {
                let Some(att) = self.message.as_ref().and_then(|m| m.attachments.get(i)) else {
                    return Task::none();
                };
                match save_attachment(&att.name, &att.data) {
                    Ok(path) => {
                        let p = path.display().to_string();
                        self.toast(tf("saved_to", &[("path", &p)]), false)
                    }
                    Err(e) => self.toast(tf("save_failed", &[("err", &e.to_string())]), true),
                }
            }
            Message::ToggleAppearance => {
                self.appearance_open = !self.appearance_open;
                Task::none()
            }
            Message::CloseAppearance => {
                self.appearance_open = false;
                Task::none()
            }
            Message::SetTheme(key) => {
                self.config.ui.theme = key.to_string();
                self.save_config()
            }
            Message::SetDensity(d) => {
                self.config.ui.density = d;
                self.save_config()
            }
            Message::SetThreaded(on) => {
                self.config.ui.threaded = on;
                self.save_config()
            }
            Message::TogglePreview(on) => {
                self.config.ui.show_preview = on;
                self.save_config()
            }
            Message::SetLanguage(code) => {
                i18n::set(Some(&code));
                self.config.ui.language = code;
                // Folder labels depend on the language: already computed on the fly.
                self.save_config()
            }
            Message::OpenSettings(section) => {
                self.appearance_open = false;
                if let Some(a) = self.config.account()
                    && self.screen == Screen::Mail
                {
                    self.draft = AccountDraft::from(a);
                    self.editing = Some(self.config.active);
                }
                self.confirm_remove = false;
                self.test_result = None;
                self.screen = Screen::Settings(section);
                Task::none()
            }
            Message::CloseSettings => {
                if self.config.account().is_some() {
                    self.screen = Screen::Mail;
                }
                Task::none()
            }
            Message::Draft(field, value) => {
                let d = &mut self.draft;
                let target = match field {
                    Field::DisplayName => &mut d.display_name,
                    Field::Email => &mut d.email,
                    Field::Username => &mut d.username,
                    Field::Password => &mut d.password,
                    Field::ImapHost => &mut d.imap_host,
                    Field::ImapPort => &mut d.imap_port,
                    Field::SmtpHost => &mut d.smtp_host,
                    Field::SmtpPort => &mut d.smtp_port,
                    Field::CardDav => &mut d.carddav_url,
                    Field::CalDav => &mut d.caldav_url,
                    Field::DavUsername => &mut d.dav_username,
                    Field::DavPassword => &mut d.dav_password,
                };
                *target = value;
                // New account: recognise the provider from the address.
                if matches!(field, Field::Email) && self.editing.is_none() {
                    let d = &mut self.draft;
                    match crate::providers::for_email(&d.email) {
                        Some(i) if d.preset != Some(i) && (d.preset.is_some() || d.imap_host.is_empty()) => {
                            d.apply_preset(i)
                        }
                        _ if d.preset.is_some() => d.username = d.email.trim().to_string(),
                        _ => {}
                    }
                }
                self.test_result = None;
                Task::none()
            }
            Message::PickProvider(choice) => {
                match choice {
                    Some(i) => self.draft.apply_preset(i),
                    // Manual setup: keep what is filled in, show every field.
                    None => self.draft.preset = None,
                }
                self.test_result = None;
                Task::none()
            }
            Message::SetProtocol(protocol) => {
                let d = &mut self.draft;
                // Move the incoming port along with the protocol's TLS port.
                d.imap_port = match (protocol, d.imap_port.as_str()) {
                    (crate::config::Protocol::Pop3, "993" | "") => "995".into(),
                    (crate::config::Protocol::Imap, "995" | "") => "993".into(),
                    _ => d.imap_port.clone(),
                };
                d.protocol = protocol;
                self.test_result = None;
                Task::none()
            }
            Message::SetKeepDays(days) => {
                self.draft.pop3_keep_days = days;
                Task::none()
            }
            Message::UseEmailAsUsername => {
                self.draft.username = self.draft.email.trim().to_string();
                Task::none()
            }
            Message::TestConnection => {
                let Some(acc) = self.draft.to_account() else {
                    return Task::none();
                };
                self.testing = true;
                self.test_result = None;
                Task::perform(Backend::test(acc), Message::Tested)
            }
            Message::Tested(result) => {
                self.testing = false;
                self.test_result = Some(result);
                Task::none()
            }
            Message::SaveAccount => {
                let Some(acc) = self.draft.to_account() else {
                    return Task::none();
                };
                let index = match self.editing {
                    Some(i) if i < self.config.accounts.len() => {
                        if self.config.accounts[i] == acc && i == self.config.active {
                            // Unchanged active account: just go back to the mail.
                            self.screen = Screen::Mail;
                            if self.conn == Conn::Online {
                                return Task::none();
                            }
                        }
                        self.config.accounts[i] = acc;
                        i
                    }
                    _ => {
                        self.config.accounts.push(acc);
                        self.config.accounts.len() - 1
                    }
                };
                self.editing = Some(index);
                let save = self.save_config();
                Task::batch([save, self.switch_account(index)])
            }
            Message::SwitchAccount(i) => {
                if i >= self.config.accounts.len() || (i == self.config.active && self.conn != Conn::Offline)
                {
                    return Task::none();
                }
                let save = {
                    self.config.active = i;
                    self.save_config()
                };
                Task::batch([save, self.switch_account(i)])
            }
            Message::EditAccount(i) => {
                if let Some(a) = self.config.accounts.get(i) {
                    self.draft = AccountDraft::from(a);
                    self.editing = Some(i);
                    self.test_result = None;
                    self.confirm_remove = false;
                }
                Task::none()
            }
            Message::TogglePassword => {
                self.show_password = !self.show_password;
                Task::none()
            }
            Message::OpenAccountSettings(i) => {
                self.appearance_open = false;
                self.screen = Screen::Settings(Section::Accounts);
                self.update(Message::EditAccount(i))
            }
            Message::AddAccount => {
                self.appearance_open = false;
                self.screen = Screen::Settings(Section::Accounts);
                self.update(Message::NewAccount)
            }
            Message::NewAccount => {
                self.draft = AccountDraft::blank();
                self.editing = None;
                self.test_result = None;
                self.confirm_remove = false;
                iced::widget::operation::focus(crate::ui::settings::EMAIL_FIELD)
            }
            Message::OAuthSignIn(provider) => {
                if self.oauth_busy {
                    return Task::none();
                }
                self.oauth_busy = true;
                self.test_result = None;
                let hint = self.draft.email.trim().to_string();
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let open = |url: &str| -> anyhow::Result<()> { Ok(open::that_detached(url)?) };
                            ferrowl_core::oauth::sign_in(provider, &hint, &open)
                                .map(|email| (provider, email))
                                .map_err(|e| format!("{e:#}"))
                        })
                        .await
                        .unwrap_or_else(|e| Err(e.to_string()))
                    },
                    Message::OAuthDone,
                )
            }
            Message::OAuthDone(result) => {
                self.oauth_busy = false;
                match result {
                    Ok((provider, email)) => {
                        // Signed in: the account's address and servers come
                        // from the provider.
                        let ((imap, imap_port), (smtp, smtp_port)) = provider.servers(&email);
                        let d = &mut self.draft;
                        if d.display_name.trim().is_empty() {
                            d.display_name = email.split('@').next().unwrap_or_default().to_string();
                        }
                        d.email = email.clone();
                        d.username = email;
                        d.password.clear();
                        d.oauth = Some(provider);
                        d.imap_host = imap.into();
                        d.imap_port = imap_port.to_string();
                        d.smtp_host = smtp.into();
                        d.smtp_port = smtp_port.to_string();
                        Task::none()
                    }
                    Err(e) => {
                        self.test_result = Some(Err(e));
                        Task::none()
                    }
                }
            }
            Message::OAuthUsePassword => {
                self.draft.oauth = None;
                Task::none()
            }
            Message::RemoveAccount(confirmed) => {
                let Some(i) = self.editing else {
                    return Task::none();
                };
                if !confirmed {
                    self.confirm_remove = true;
                    return Task::none();
                }
                self.confirm_remove = false;
                if i >= self.config.accounts.len() {
                    return Task::none();
                }
                let removed = self.config.accounts.remove(i);
                if let Some(provider) = removed.oauth {
                    ferrowl_core::oauth::forget(provider, &removed.email);
                }
                // Same mailbox added twice shares the cache: keep it then.
                let key = ferrowl_core::cache::account_key(&removed);
                if !self
                    .config
                    .accounts
                    .iter()
                    .any(|a| ferrowl_core::cache::account_key(a) == key)
                {
                    ferrowl_core::cache::forget_account(&removed);
                }
                let was_active = i == self.config.active;
                if self.config.active > i || self.config.active >= self.config.accounts.len() {
                    self.config.active = self.config.active.saturating_sub(1);
                }
                let save = self.save_config();
                match self.config.account().cloned() {
                    Some(a) => {
                        self.draft = AccountDraft::from(&a);
                        self.editing = Some(self.config.active);
                        if was_active {
                            let active = self.config.active;
                            let task = self.switch_account(active);
                            // Stay in settings after removing.
                            self.screen = Screen::Settings(Section::Accounts);
                            return Task::batch([save, task]);
                        }
                        save
                    }
                    None => {
                        self.draft = AccountDraft::blank();
                        self.editing = None;
                        self.reset_mailbox();
                        self.backend = Backend::default();
                        self.conn = Conn::Offline;
                        save
                    }
                }
            }
            Message::OpenContacts => {
                self.pane = Pane::Contacts;
                self.appearance_open = false;
                self.screen = Screen::Mail;
                self.reload_contacts();
                Task::none()
            }
            Message::ContactsQuery(q) => {
                self.contacts_query = q;
                self.reload_contacts();
                Task::none()
            }
            Message::ContactSelect(i) => {
                self.contact_selected = (i < self.contacts.len()).then_some(i);
                self.contact_form = None;
                self.confirm_contact_delete = false;
                Task::none()
            }
            Message::NewContact => {
                self.contact_form = Some(ContactForm::new(&Default::default()));
                self.confirm_contact_delete = false;
                Task::none()
            }
            Message::EditContact => {
                if let Some(c) = self.contact_selected.and_then(|i| self.contacts.get(i)) {
                    self.contact_form = Some(ContactForm::new(c));
                }
                self.confirm_contact_delete = false;
                Task::none()
            }
            Message::ContactField(field, value) => {
                if let Some(f) = self.contact_form.as_mut() {
                    let target = match field {
                        ContactField::Name => Some(&mut f.name),
                        ContactField::Organization => Some(&mut f.organization),
                        ContactField::Email(i) => f.emails.get_mut(i),
                        ContactField::Phone(i) => f.phones.get_mut(i),
                    };
                    if let Some(target) = target {
                        *target = value;
                    }
                    f.error = None;
                }
                Task::none()
            }
            Message::ContactNote(action) => {
                if let Some(f) = self.contact_form.as_mut() {
                    f.note.perform(action);
                }
                Task::none()
            }
            Message::ContactAddLine(email) => {
                if let Some(f) = self.contact_form.as_mut() {
                    if email {
                        f.emails.push(String::new());
                    } else {
                        f.phones.push(String::new());
                    }
                }
                Task::none()
            }
            Message::SaveContact => {
                let Some(acc) = self.config.account().cloned() else {
                    return Task::none();
                };
                let Some(f) = self.contact_form.as_mut().filter(|f| f.can_save()) else {
                    return Task::none();
                };
                f.saving = true;
                let contact = f.contact();
                self.perform(Backend::save_contact(acc, contact), Message::ContactSaved)
            }
            Message::ContactSaved(result) => match result {
                Ok(url) => {
                    self.contact_form = None;
                    // Show the saved contact, whatever the search.
                    self.contacts_query.clear();
                    self.reload_contacts();
                    self.contact_selected = self.contacts.iter().position(|c| c.url == url);
                    self.toast(t("contact_saved"), false)
                }
                Err(e) => {
                    if let Some(f) = self.contact_form.as_mut() {
                        f.saving = false;
                        f.error = Some(e);
                    }
                    Task::none()
                }
            },
            Message::CancelContact => {
                self.contact_form = None;
                self.confirm_contact_delete = false;
                Task::none()
            }
            Message::DeleteContact(confirmed) => {
                if !confirmed {
                    self.confirm_contact_delete = true;
                    return Task::none();
                }
                self.confirm_contact_delete = false;
                let Some((acc, c)) = self
                    .config
                    .account()
                    .cloned()
                    .zip(self.contact_selected.and_then(|i| self.contacts.get(i)))
                else {
                    return Task::none();
                };
                let url = c.url.clone();
                self.perform(Backend::delete_contact(acc, url), Message::ContactDeleted)
            }
            Message::ContactDeleted(result) => {
                self.contact_selected = None;
                self.reload_contacts();
                match result {
                    Ok(()) => self.toast(t("contact_deleted"), false),
                    Err(e) => self.toast(e, true),
                }
            }
            Message::SyncContacts => {
                if self.contacts_sync == SyncState::Running {
                    return Task::none();
                }
                self.sync_contacts(Duration::ZERO)
            }
            Message::ContactsSynced(r) => {
                self.contacts_sync = match r {
                    Ok(_) => SyncState::Done,
                    Err(e) => SyncState::Failed(e),
                };
                self.reload_contacts();
                Task::none()
            }
            Message::OpenCalendar => {
                self.pane = Pane::Calendar;
                self.appearance_open = false;
                self.screen = Screen::Mail;
                self.reload_agenda();
                Task::none()
            }
            Message::CalendarMonth(delta) => {
                self.cal_month = if delta == 0 {
                    month_start(ferrowl_core::ical::today())
                } else {
                    shift_month(self.cal_month, delta)
                };
                self.agenda_selected = None;
                self.reload_agenda();
                Task::none()
            }
            Message::AgendaSelect(i) => {
                self.event_form = None;
                self.confirm_event_delete = false;
                self.agenda_selected = (i < self.agenda.len()).then_some(i);
                Task::none()
            }
            Message::SyncCalendar => {
                if self.calendar_sync == SyncState::Running {
                    return Task::none();
                }
                self.sync_calendar(Duration::ZERO)
            }
            Message::CalendarSynced(r) => {
                self.calendar_sync = match r {
                    Ok(_) => SyncState::Done,
                    Err(e) => SyncState::Failed(e),
                };
                self.reload_agenda();
                Task::none()
            }
            Message::InviteRespond(reply) => {
                let sent = self.folder_by_kind(FolderKind::Sent).map(|f| f.name.clone());
                let Some(card) = self.invite.as_mut().filter(|c| !c.busy) else {
                    return Task::none();
                };
                card.busy = true;
                let ics = card.ics.clone();
                let b = self.backend.clone();
                self.perform(
                    async move { b.respond_invitation(ics, reply, sent).await },
                    Message::InviteResponded,
                )
            }
            Message::InviteResponded(result) => {
                let toast = match result {
                    Ok(note) => self.toast(note, false),
                    Err(e) => self.toast(e, true),
                };
                if let Some(msg) = self.message.clone() {
                    self.invite = self.invite_card(&msg);
                }
                self.reload_agenda();
                toast
            }
            Message::CopyInsight(value) => {
                let toast = self.toast(t("insight_copied"), false);
                Task::batch([iced::clipboard::write(value), toast])
            }
            Message::AddDue(day, number, amount) => {
                if self.due_adding {
                    return Task::none();
                }
                let Some(acc) = self.config.account().cloned() else {
                    return Task::none();
                };
                let Some(calendar) = Backend::calendars(&acc).into_iter().next() else {
                    return self.toast(t("err_no_calendar"), true);
                };
                let what = [number, amount]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" · ");
                let what = if what.is_empty() {
                    t("insight_bill").to_string()
                } else {
                    what
                };
                let draft = ferrowl_core::events::Draft {
                    summary: tf("insight_due_title", &[("what", &what)]),
                    all_day: true,
                    start: day.and_hms_opt(9, 0, 0).unwrap_or_default(),
                    end: day.and_hms_opt(10, 0, 0).unwrap_or_default(),
                    location: String::new(),
                    description: String::new(),
                    repeat: ferrowl_core::events::Repeat::Never,
                };
                self.due_adding = true;
                self.perform(
                    Backend::save_event(acc, calendar.url, None, draft),
                    Message::DueAdded,
                )
            }
            Message::DueAdded(result) => {
                self.due_adding = false;
                let toast = match result {
                    Ok(_) => self.toast(t("insight_due_added"), false),
                    Err(e) => self.toast(e, true),
                };
                self.reload_agenda();
                toast
            }
            Message::InviteRemove => {
                let (Some(card), Some(acc)) = (self.invite.as_mut(), self.config.account().cloned()) else {
                    return Task::none();
                };
                card.busy = true;
                let uid = card.invitation.uid.clone();
                self.perform(Backend::remove_cancelled(acc, uid), Message::InviteRemoved)
            }
            Message::InviteRemoved(result) => {
                let toast = match result {
                    Ok(note) => self.toast(note, false),
                    Err(e) => self.toast(e, true),
                };
                if let Some(msg) = self.message.clone() {
                    self.invite = self.invite_card(&msg);
                }
                self.reload_agenda();
                toast
            }
            Message::NewEvent => {
                let Some(acc) = self.config.account() else {
                    return Task::none();
                };
                let calendars = Backend::calendars(acc);
                let Some(first) = calendars.first() else {
                    return self.toast(t("err_no_calendar"), true);
                };
                // On today when this month is shown, else on its first day.
                let today = ferrowl_core::ical::today();
                let day = if month_start(today) == self.cal_month {
                    today
                } else {
                    self.cal_month
                };
                let draft = ferrowl_core::events::Draft::new_on(day);
                self.event_form = Some(EventForm::new(None, first.url.clone(), &draft));
                self.confirm_event_delete = false;
                Task::none()
            }
            Message::EditEvent => {
                let Some((acc, e)) = self
                    .config
                    .account()
                    .zip(self.agenda_selected.and_then(|i| self.agenda.get(i)))
                else {
                    return Task::none();
                };
                if let Some(draft) = Backend::event_draft(acc, &e.url) {
                    self.event_form = Some(EventForm::new(
                        Some(e.url.clone()),
                        e.calendar_url.clone(),
                        &draft,
                    ));
                }
                self.confirm_event_delete = false;
                Task::none()
            }
            Message::EventField(field, value) => {
                if let Some(f) = self.event_form.as_mut() {
                    let target = match field {
                        EventField::Summary => &mut f.summary,
                        EventField::Location => &mut f.location,
                        EventField::StartDate => &mut f.start_date,
                        EventField::StartTime => &mut f.start_time,
                        EventField::EndDate => &mut f.end_date,
                        EventField::EndTime => &mut f.end_time,
                    };
                    *target = value;
                    f.error = None;
                }
                Task::none()
            }
            Message::EventNotes(action) => {
                if let Some(f) = self.event_form.as_mut() {
                    f.notes.perform(action);
                }
                Task::none()
            }
            Message::EventPicker(field) => {
                if let Some(f) = self.event_form.as_mut() {
                    f.picker = if f.picker.is_some_and(|(open, _)| open == field) {
                        None
                    } else {
                        let typed = if field == EventField::EndDate {
                            &f.end_date
                        } else {
                            &f.start_date
                        };
                        let day = chrono::NaiveDate::parse_from_str(typed, "%Y-%m-%d")
                            .unwrap_or_else(|_| ferrowl_core::ical::today());
                        Some((field, chrono::Datelike::with_day(&day, 1).unwrap_or(day)))
                    };
                }
                Task::none()
            }
            Message::EventPickerMonth(delta) => {
                if let Some((_, month)) = self.event_form.as_mut().and_then(|f| f.picker.as_mut()) {
                    let moved = if delta < 0 {
                        month.checked_sub_months(chrono::Months::new(1))
                    } else {
                        month.checked_add_months(chrono::Months::new(1))
                    };
                    if let Some(m) = moved {
                        *month = m;
                    }
                }
                Task::none()
            }
            Message::EventPickDate(day) => {
                if let Some(f) = self.event_form.as_mut()
                    && let Some((field, _)) = f.picker.take()
                {
                    let parse = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
                    let iso = |d: chrono::NaiveDate| d.format("%Y-%m-%d").to_string();
                    if field == EventField::EndDate {
                        f.end_date = iso(day);
                    } else {
                        // Moving the start moves the end too, keeping the length.
                        if let (Some(old), Some(end)) = (parse(&f.start_date), parse(&f.end_date)) {
                            f.end_date = iso(end + (day - old));
                        }
                        f.start_date = iso(day);
                    }
                    f.error = None;
                }
                Task::none()
            }
            Message::EventAllDay(on) => {
                if let Some(f) = self.event_form.as_mut() {
                    f.all_day = on;
                }
                Task::none()
            }
            Message::EventRepeat(r) => {
                if let Some(f) = self.event_form.as_mut() {
                    f.repeat = r;
                }
                Task::none()
            }
            Message::EventCalendar(url) => {
                if let Some(f) = self.event_form.as_mut() {
                    f.calendar = url;
                }
                Task::none()
            }
            Message::SaveEvent => {
                let Some(acc) = self.config.account().cloned() else {
                    return Task::none();
                };
                let Some(f) = self.event_form.as_mut().filter(|f| !f.saving) else {
                    return Task::none();
                };
                let Some(draft) = f.draft().filter(|d| d.is_valid()) else {
                    f.error = Some(t("event_bad_time").to_string());
                    return Task::none();
                };
                f.saving = true;
                let (calendar, url) = (f.calendar.clone(), f.url.clone());
                self.perform(
                    Backend::save_event(acc, calendar, url, draft),
                    Message::EventSaved,
                )
            }
            Message::EventSaved(result) => match result {
                Ok(note) => {
                    self.event_form = None;
                    self.reload_agenda();
                    self.toast(note, false)
                }
                Err(e) => {
                    if let Some(f) = self.event_form.as_mut() {
                        f.saving = false;
                        f.error = Some(e);
                    }
                    Task::none()
                }
            },
            Message::CancelEvent => {
                self.event_form = None;
                self.confirm_event_delete = false;
                Task::none()
            }
            Message::DeleteEvent(confirmed) => {
                if !confirmed {
                    self.confirm_event_delete = true;
                    return Task::none();
                }
                self.confirm_event_delete = false;
                let Some((acc, e)) = self
                    .config
                    .account()
                    .cloned()
                    .zip(self.agenda_selected.and_then(|i| self.agenda.get(i)))
                else {
                    return Task::none();
                };
                let url = e.url.clone();
                self.perform(Backend::delete_event(acc, url), Message::EventDeleted)
            }
            Message::EventDeleted(result) => {
                self.agenda_selected = None;
                self.reload_agenda();
                match result {
                    Ok(note) => self.toast(note, false),
                    Err(e) => self.toast(e, true),
                }
            }
            Message::WriteTo(name, address) => {
                if self.compose.is_none() && self.config.account().is_some() {
                    let mut c = Compose::new(ComposeMode::New);
                    c.rich = self.config.ui.rich_compose;
                    c.to = if name.is_empty() || name == address {
                        address
                    } else {
                        format!("{name} <{address}>")
                    };
                    self.compose = Some(c);
                }
                Task::none()
            }
            Message::ComposeNew => {
                if self.compose.is_none() && self.config.account().is_some() {
                    let mut c = Compose::new(ComposeMode::New);
                    c.rich = self.config.ui.rich_compose;
                    self.compose = Some(c);
                }
                Task::none()
            }
            Message::ComposePick(field, name, address) => {
                if let Some(c) = self.compose.as_mut() {
                    let target = match field {
                        ComposeField::To => &mut c.to,
                        ComposeField::Cc => &mut c.cc,
                        ComposeField::Bcc => &mut c.bcc,
                        ComposeField::Subject => return Task::none(),
                    };
                    // Replace the word being typed with the chosen address.
                    let keep = target
                        .rfind([',', ';'])
                        .map(|i| target[..=i].to_string() + " ")
                        .unwrap_or_default();
                    let entry = if name.is_empty() {
                        address
                    } else {
                        format!("{name} <{address}>")
                    };
                    *target = format!("{keep}{entry}, ");
                    c.suggest = None;
                }
                Task::none()
            }
            Message::ComposePickFirst(field) => {
                let first = self
                    .compose
                    .as_ref()
                    .and_then(|c| c.suggest.as_ref())
                    .filter(|(f, _)| *f == field)
                    .and_then(|(_, list)| list.first().cloned());
                match first {
                    Some((name, address)) => self.update(Message::ComposePick(field, name, address)),
                    None => Task::none(),
                }
            }
            Message::ComposeField(field, value) => {
                // Suggestions for the word being typed in a recipient field.
                let suggest = match (field, self.config.account()) {
                    (ComposeField::To | ComposeField::Cc | ComposeField::Bcc, Some(acc)) => {
                        let word = value.rsplit([',', ';']).next().unwrap_or("").trim();
                        let list = if word.chars().count() >= 2 && !word.contains('<') {
                            Backend::suggest(acc, word)
                        } else {
                            Vec::new()
                        };
                        (!list.is_empty()).then_some((field, list))
                    }
                    _ => None,
                };
                if let Some(c) = self.compose.as_mut() {
                    c.suggest = suggest;
                    match field {
                        ComposeField::To => c.to = value,
                        ComposeField::Cc => c.cc = value,
                        ComposeField::Bcc => c.bcc = value,
                        ComposeField::Subject => c.subject = value,
                    }
                    c.error = None;
                }
                Task::none()
            }
            Message::ComposeToggleCc => {
                if let Some(c) = self.compose.as_mut() {
                    c.show_cc = !c.show_cc;
                }
                Task::none()
            }
            Message::ComposeFormat(fmt) => {
                if let Some(c) = self.compose.as_mut()
                    && c.rich
                    && c.preview.is_none()
                {
                    c.format(fmt);
                }
                Task::none()
            }
            Message::ComposeToggleRich => {
                let Some(c) = self.compose.as_mut() else {
                    return Task::none();
                };
                c.rich = !c.rich;
                if !c.rich {
                    c.preview = None;
                }
                self.config.ui.rich_compose = c.rich;
                self.save_config()
            }
            Message::ComposeTogglePreview => {
                if let Some(c) = self.compose.as_mut() {
                    c.preview = match c.preview {
                        Some(_) => None,
                        None => Some(iced::widget::markdown::parse(&c.body.text()).collect()),
                    };
                }
                Task::none()
            }
            Message::ComposeEdit(action) => {
                if let Some(c) = self.compose.as_mut() {
                    c.body.perform(action);
                }
                Task::none()
            }
            Message::ComposeAttach => Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title(t("attach"))
                        .pick_files()
                        .await
                        .map(|files| files.iter().map(|f| f.path().to_path_buf()).collect())
                        .unwrap_or_default()
                },
                Message::ComposeAttached,
            ),
            Message::ComposeAttached(paths) => {
                if let Some(c) = self.compose.as_mut() {
                    for p in paths {
                        if !c.attachments.contains(&p) {
                            c.attachments.push(p);
                        }
                    }
                }
                Task::none()
            }
            Message::ComposeRemove(i) => {
                if let Some(c) = self.compose.as_mut()
                    && i < c.attachments.len()
                {
                    c.attachments.remove(i);
                }
                Task::none()
            }
            Message::ComposeSend => {
                let sent = self.folder_by_kind(FolderKind::Sent).map(|f| f.name.clone());
                let Some(c) = self.compose.as_mut() else {
                    return Task::none();
                };
                if c.sending {
                    return Task::none();
                }
                if c.to.trim().is_empty() {
                    c.error = Some(t("err_no_recipient").into());
                    return Task::none();
                }
                c.sending = true;
                c.error = None;
                let out = Outgoing {
                    to: c.to.clone(),
                    cc: c.cc.clone(),
                    bcc: c.bcc.clone(),
                    subject: c.subject.clone(),
                    body: c.body.text(),
                    html: (c.rich && crate::markdown::has_formatting(&c.body.text()))
                        .then(|| crate::markdown::to_html_document(&c.body.text())),
                    in_reply_to: c.in_reply_to.clone(),
                    references: c.references.clone(),
                    attachments: c.attachments.clone(),
                    sign: c.sign,
                    encrypt: c.encrypt,
                    calendar: None,
                };
                let b = self.backend.clone();
                Task::perform(async move { b.send(out, sent).await }, Message::ComposeSent)
            }
            Message::ComposeSent(result) => match result {
                Ok(note) => {
                    self.compose = None;
                    let toast = match note {
                        None => self.toast(t("sent_ok"), false),
                        Some(e) => self.toast(tf("sent_no_copy", &[("err", &e)]), true),
                    };
                    // If we are in Sent, reload to show the message.
                    let reload = match (self.current_kind(), self.current_folder.clone()) {
                        (FolderKind::Sent, Some(f)) => self.load_folder(f, true),
                        _ => Task::none(),
                    };
                    Task::batch([toast, reload])
                }
                Err(e) => {
                    if let Some(c) = self.compose.as_mut() {
                        c.sending = false;
                        c.error = Some(e);
                    }
                    Task::none()
                }
            },
            Message::ComposeClose => {
                self.compose = None;
                Task::none()
            }
            Message::Key(event) => self.shortcut(event),
            Message::DismissToast(id) => {
                if self.toast.as_ref().is_some_and(|t| t.id == id) {
                    self.toast = None;
                }
                Task::none()
            }
            Message::CopyDetails(details) => {
                let copy = iced::clipboard::write(details);
                Task::batch([copy, self.toast(t("copied"), false)])
            }
            Message::DevScreenshot => {
                // Development: show the selected message with this plain-text
                // body without a server (FERROWL_DEV_BODY, "\n" for new lines).
                if let (Ok(body), Some(uid), None) = (
                    std::env::var("FERROWL_DEV_BODY"),
                    self.selected,
                    self.message.as_ref(),
                ) && let Some(h) = self.headers.iter().find(|h| h.uid == uid).cloned()
                {
                    let msg = FullMessage {
                        uid,
                        from: h.from.clone(),
                        from_address: String::new(),
                        to: String::new(),
                        cc: String::new(),
                        to_addrs: Vec::new(),
                        cc_addrs: Vec::new(),
                        subject: h.subject.clone(),
                        timestamp: h.timestamp,
                        message_id: None,
                        references: Vec::new(),
                        body: body.replace("\\n", "\n"),
                        // FERROWL_DEV_HTML=<file>: an HTML part as well.
                        html: std::env::var_os("FERROWL_DEV_HTML")
                            .and_then(|f| std::fs::read_to_string(f).ok()),
                        inline: HashMap::new(),
                        attachments: Vec::new(),
                        dkim_pass: false,
                        pgp: None,
                        calendar: None,
                    };
                    return self.update(Message::Loaded(Ok(msg)));
                }
                // Open the first message so the reader pane is populated.
                if std::env::var_os("FERROWL_DEV_CONFIRM").is_some() && self.selected.is_some() {
                    self.confirm_delete = true;
                }
                if std::env::var_os("FERROWL_DEV_MENU").is_some()
                    && self.menu.is_none()
                    && let Some(uid) = self.visible().get(1).map(|h| h.uid)
                {
                    self.menu = Some(ContextMenu {
                        uid,
                        at: iced::Point::new(420.0, 260.0),
                        folders: false,
                    });
                }
                // Development: check the first N rows to show the bulk bar.
                if let Some(n) = std::env::var("FERROWL_DEV_CHECK")
                    .ok()
                    .and_then(|n| n.parse::<usize>().ok())
                    && self.checked.is_empty()
                {
                    let first: Vec<u32> = self.visible().iter().take(n).map(|h| h.uid).collect();
                    self.checked.extend(first);
                }
                // Development: star the newest message after the first shot.
                if std::env::var_os("FERROWL_DEV_STAR").is_some()
                    && self.dev_shots == 1
                    && let Some(h) = self.visible().first().filter(|h| !h.flagged)
                {
                    let uid = h.uid;
                    return self.update(Message::ToggleStar(vec![uid], true));
                }
                // Development: answer the open invitation after the first shot.
                if let Some(reply) = std::env::var("FERROWL_DEV_INVITE")
                    .ok()
                    .and_then(|r| ferrowl_core::invite::Reply::from_partstat(&r))
                    && self.dev_shots >= 1
                    && self.invite.as_ref().is_some_and(|c| c.reply.is_none() && !c.busy)
                {
                    return self.update(Message::InviteRespond(reply));
                }
                if std::env::var_os("FERROWL_SCREENSHOT_OPEN").is_some() && self.selected.is_none() {
                    let nth = std::env::var("FERROWL_DEV_OPEN_NTH")
                        .ok()
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0);
                    if let Some(uid) = self.visible().get(nth).map(|h| h.uid) {
                        return self.open(uid);
                    }
                }
                iced::window::latest()
                    .and_then(iced::window::screenshot)
                    .map(Message::DevShot)
            }
            Message::DevShot(shot) => {
                if let Some(path) = std::env::var_os("FERROWL_SCREENSHOT") {
                    let mut path = PathBuf::from(path);
                    self.dev_shots += 1;
                    if self.dev_shots > 1 {
                        path.set_extension(format!("{}.png", self.dev_shots));
                    }
                    if let Err(e) = save_png(&path, &shot) {
                        eprintln!("screenshot failed: {e}");
                    }
                }
                let wanted: u32 = std::env::var("FERROWL_SCREENSHOT_COUNT")
                    .ok()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                if self.dev_shots >= wanted {
                    quit()
                } else {
                    Task::none()
                }
            }
            Message::Noop => Task::none(),
        }
    }

    /// Keyboard shortcuts (only when no text field has focus).
    fn shortcut(&mut self, event: keyboard::Event) -> Task<Message> {
        // Shift / Ctrl held while clicking the list selects ranges.
        if let keyboard::Event::ModifiersChanged(m) = event {
            self.modifiers = m;
            return Task::none();
        }
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return Task::none();
        };
        self.modifiers = modifiers;
        // Tab / Shift+Tab move between the fields of forms (account,
        // compose, event, settings…); iced does not do it by itself.
        if let Key::Named(Named::Tab) = key.as_ref() {
            return if modifiers.shift() {
                operation::focus_previous()
            } else {
                operation::focus_next()
            };
        }
        // Delete dialog open: Enter confirms, Esc cancels, other keys do nothing.
        if self.confirm_delete {
            return match key {
                Key::Named(Named::Enter) => self.update(Message::ConfirmDelete(true)),
                Key::Named(Named::Escape) => self.update(Message::ConfirmDelete(false)),
                _ => Task::none(),
            };
        }
        if let Key::Named(Named::Escape) = key {
            if self.preview.is_some() {
                self.preview = None;
            } else if self.menu.is_some() {
                self.menu = None;
            } else if self.drag.is_some() {
                self.drag = None;
            } else if self.appearance_open {
                self.appearance_open = false;
            } else if self.popup.is_some() {
                self.popup = None;
            } else if self.compose.is_none() && self.pane == Pane::Mail && self.selected.is_some() {
                // Close the message: the list takes the whole width.
                return self.update(Message::CloseReader);
            }
            return Task::none();
        }
        // Mail shortcuts only where the mail is shown.
        if self.compose.is_some() || self.screen != Screen::Mail || self.pane != Pane::Mail {
            return Task::none();
        }
        if modifiers.command() {
            return match key.as_ref() {
                Key::Character("k") => operation::focus(SEARCH_ID),
                Key::Character("c") if self.html_render.as_ref().is_some_and(|r| r.selection) => {
                    let uid = self.html_render.as_ref().map(|r| r.uid).unwrap_or_default();
                    Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || crate::html_view::selected_text(uid))
                                .await
                                .ok()
                                .flatten()
                        },
                        Message::CopyText,
                    )
                }
                _ => Task::none(),
            };
        }
        if modifiers.alt() {
            return Task::none();
        }
        match key.as_ref() {
            Key::Character("c") => self.update(Message::ComposeNew),
            Key::Character("r") => self.update(Message::Reply),
            Key::Character("a") => self.update(Message::ReplyAll),
            Key::Character("f") => self.update(Message::Forward),
            Key::Character("e") => self.update(Message::Archive),
            Key::Character("u") => self.update(Message::ToggleRead),
            Key::Character("j") => self.step(1),
            Key::Character("k") => self.step(-1),
            Key::Character("/") => operation::focus(SEARCH_ID),
            Key::Named(Named::ArrowDown) => self.step(1),
            Key::Named(Named::ArrowUp) => self.step(-1),
            // Shift+Del: delete for good, skipping Trash (asks first).
            Key::Named(Named::Delete) if modifiers.shift() => match self.selected {
                Some(uid) => self.ask_delete_forever(uid),
                None => Task::none(),
            },
            Key::Named(Named::Delete) => self.update(Message::Delete),
            _ => Task::none(),
        }
    }

    // ------------------------------------------------------------ view

    pub fn view(&self) -> Element<'_, Message> {
        match self.screen {
            Screen::Mail => ui::mail_view::view(self),
            Screen::Settings(section) => ui::settings::view(self, section),
        }
    }

    pub fn density_padding(&self) -> f32 {
        match self.config.ui.density {
            Density::Compact => 8.0,
            Density::Comfortable => 14.0,
            Density::Spacious => 20.0,
        }
    }
}

fn save_png(path: &std::path::Path, shot: &iced::window::Screenshot) -> anyhow::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, shot.size.width, shot.size.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()?.write_image_data(&shot.rgba)?;
    Ok(())
}

fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0' | ':') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim_start_matches('.').to_string();
    if s.is_empty() { "attachment".into() } else { s }
}

fn save_attachment(name: &str, data: &[u8]) -> anyhow::Result<PathBuf> {
    let dir = dirs::download_dir()
        .or_else(dirs::home_dir)
        .ok_or_else(|| anyhow::anyhow!(t("err_no_downloads")))?;
    let safe = safe_name(name);
    let mut path = dir.join(&safe);
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("({n}) {safe}"));
        n += 1;
    }
    std::fs::write(&path, data)?;
    Ok(path)
}

/// Minimal percent-decoding for `mailto:` query values.
fn percent_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(h), Some(l)) = (
                bytes.get(i + 1).and_then(|&b| hex(b)),
                bytes.get(i + 2).and_then(|&b| hex(b)),
            )
        {
            out.push(h * 16 + l);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Ends the process immediately. Background threads (IMAP IDLE waits up to
/// minutes, the HTML renderer, blocking IMAP calls) would otherwise keep the
/// async runtime alive after the window is gone. Settings are saved on every
/// change and the SQLite cache is crash-safe, so nothing is lost.
/// Raise requests from later launches (see `single_instance`).
fn raise_requests() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(
        1,
        async move |mut out: iced::futures::channel::mpsc::Sender<Message>| {
            use iced::futures::SinkExt;
            let Some(rx) = crate::single_instance::take_raise_requests() else {
                return;
            };
            let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
            loop {
                let rx = rx.clone();
                let got =
                    tokio::task::spawn_blocking(move || rx.lock().map(|r| r.recv().is_ok()).unwrap_or(false))
                        .await
                        .unwrap_or(false);
                if !got || out.send(Message::Raise).await.is_err() {
                    return;
                }
            }
        },
    )
}

fn quit<T>() -> Task<T> {
    std::process::exit(0)
}

/// First day of the month of `d`.
fn month_start(d: chrono::NaiveDate) -> chrono::NaiveDate {
    use chrono::Datelike;
    d.with_day(1).unwrap_or(d)
}

/// First day of the month `delta` months after `d`'s.
pub fn shift_month(d: chrono::NaiveDate, delta: i32) -> chrono::NaiveDate {
    use chrono::Datelike;
    let total = d.year() * 12 + d.month0() as i32 + delta;
    chrono::NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1).unwrap_or(d)
}

/// Up to five inbox messages on this computer a rule's condition picks.
fn rule_preview(account: &Account, condition: &str) -> Vec<(String, String)> {
    Backend::rule_preview(account, condition, 5)
        .into_iter()
        .map(|h| (h.from, h.subject))
        .collect()
}
