//! WebDAV collections of the account: address books (CardDAV, RFC 6352)
//! and calendars (CalDAV, RFC 4791). Finds them and keeps a copy of their
//! items in sync.
//!
//! Discovery follows RFC 6764: the address typed in the account settings,
//! else a known server for the mail domain, else
//! `https://<domain>/.well-known/carddav` (or `caldav`); then
//! current-user-principal -> home set -> the collections in it.
//!
//! Sync compares ETags: one PROPFIND lists every item with its ETag, only
//! new or changed items are downloaded (multiget REPORT), items gone from
//! the server are deleted. This works on every server, including those
//! without sync-collection (RFC 6578).
//!
//! The account's mail username and password are used (Basic auth), or the
//! separate ones the user set for these servers (a calendar kept elsewhere
//! than the mail), only
//! over HTTPS (except for an `http://` address the user typed, e.g. a server
//! on the local network), and only to hosts that belong to the mail
//! provider: the typed address, a known provider's server, or the domain of
//! the account's IMAP/SMTP servers. The website of a custom mail domain may
//! be hosted by someone else, so `.well-known` there is asked without them.

use crate::config::Account;
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use std::time::Duration;
use ureq::http::{Request, StatusCode};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 5;
/// Cards per addressbook-multiget request.
const BATCH: usize = 50;
/// Largest response read (a big address book with photos).
const MAX_BODY: u64 = 64 * 1024 * 1024;

const NS_DAV: &str = "DAV:";
const NS_CARD: &str = "urn:ietf:params:xml:ns:carddav";
const NS_CAL: &str = "urn:ietf:params:xml:ns:caldav";
const NS_APPLE: &str = "http://apple.com/ns/ical/";

/// Contacts servers of providers whose domain does not answer
/// `.well-known`.
const KNOWN_CARD: &[(&[&str], &str)] = &[
    (
        &["icloud.com", "me.com", "mac.com"],
        "https://contacts.icloud.com/",
    ),
    (YAHOO, "https://carddav.address.yahoo.com/"),
    (&["fastmail.com", "fastmail.fm"], "https://carddav.fastmail.com/"),
    (&["mailbox.org"], "https://dav.mailbox.org/"),
    (&["posteo.de", "posteo.net"], "https://posteo.de:8843/"),
    (&["gmx.net", "gmx.de", "gmx.com"], "https://carddav.gmx.net/"),
    (&["web.de"], "https://carddav.web.de/"),
];

/// Calendar servers, as above.
const KNOWN_CAL: &[(&[&str], &str)] = &[
    (&["icloud.com", "me.com", "mac.com"], "https://caldav.icloud.com/"),
    (YAHOO, "https://caldav.calendar.yahoo.com/"),
    (&["fastmail.com", "fastmail.fm"], "https://caldav.fastmail.com/"),
    (&["mailbox.org"], "https://dav.mailbox.org/"),
    (&["posteo.de", "posteo.net"], "https://posteo.de:8443/"),
    (&["gmx.net", "gmx.de", "gmx.com"], "https://caldav.gmx.net/"),
    (&["web.de"], "https://caldav.web.de/"),
];

const YAHOO: &[&str] = &[
    "yahoo.com",
    "yahoo.it",
    "yahoo.fr",
    "yahoo.de",
    "yahoo.es",
    "yahoo.co.uk",
    "ymail.com",
];

/// What kind of collection is synced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Address books (vCard).
    Contacts,
    /// Calendars (iCalendar events).
    Calendar,
}

impl Kind {
    fn known(self) -> &'static [(&'static [&'static str], &'static str)] {
        match self {
            Kind::Contacts => KNOWN_CARD,
            Kind::Calendar => KNOWN_CAL,
        }
    }

    fn well_known(self) -> &'static str {
        match self {
            Kind::Contacts => "carddav",
            Kind::Calendar => "caldav",
        }
    }

    /// The server address typed in the account settings.
    fn typed(self, account: &Account) -> &str {
        match self {
            Kind::Contacts => account.carddav_url.trim(),
            Kind::Calendar => account.caldav_url.trim(),
        }
    }

    fn home_set(self) -> &'static str {
        match self {
            Kind::Contacts => "<card:addressbook-home-set/>",
            Kind::Calendar => "<cal:calendar-home-set/>",
        }
    }

    /// The multiget REPORT for some item paths.
    fn multiget(self, hrefs: &str) -> String {
        match self {
            Kind::Contacts => format!(
                r#"<?xml version="1.0" encoding="utf-8"?><card:addressbook-multiget xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav"><d:prop><d:getetag/><card:address-data/></d:prop>{hrefs}</card:addressbook-multiget>"#
            ),
            Kind::Calendar => format!(
                r#"<?xml version="1.0" encoding="utf-8"?><cal:calendar-multiget xmlns:d="DAV:" xmlns:cal="urn:ietf:params:xml:ns:caldav"><d:prop><d:getetag/><cal:calendar-data/></d:prop>{hrefs}</cal:calendar-multiget>"#
            ),
        }
    }

    /// Error text for this kind: the contacts or the calendar server.
    fn text(self, contacts: &str, calendar: &str) -> &'static str {
        match self {
            Kind::Contacts => crate::i18n::t(contacts),
            Kind::Calendar => crate::i18n::t(calendar),
        }
    }

    fn not_found(self) -> &'static str {
        match self {
            Kind::Contacts => crate::i18n::t("err_dav_none"),
            Kind::Calendar => crate::i18n::t("err_caldav_none"),
        }
    }
}

/// One address book or calendar on the server.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Collection {
    /// Absolute URL of the collection.
    pub url: String,
    pub name: String,
    /// Calendar color as sent by the server ("#RRGGBB[AA]"), if any.
    pub color: String,
}

/// A downloaded item (a vCard or an iCalendar object).
#[derive(Debug, Clone)]
pub struct Item {
    /// Absolute URL of the item (its identity).
    pub url: String,
    pub etag: String,
    pub data: String,
}

/// What a sync changed, for the caller to store.
#[derive(Debug, Default)]
pub struct Changes {
    pub collections: Vec<Collection>,
    /// New or changed items, with their collection's URL.
    pub items: Vec<(String, Item)>,
    /// URLs of items no longer on the server.
    pub gone: Vec<String>,
}

/// The server refused the username or password.
#[derive(Debug)]
pub struct AuthRefused(pub Kind);

impl std::fmt::Display for AuthRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.text("err_dav_auth", "err_caldav_auth"))
    }
}

impl std::error::Error for AuthRefused {}

pub struct Client {
    agent: ureq::Agent,
    auth: String,
    kind: Kind,
    /// Plain HTTP was typed by the user, so it is allowed.
    allow_http: bool,
    /// Domains the credentials may be sent to (see the module docs).
    trusted: Vec<String>,
}

impl Client {
    pub fn new(account: &Account, kind: Kind) -> Client {
        let agent = ureq::Agent::config_builder()
            .allow_non_standard_methods(true)
            .http_status_as_error(false)
            // Followed by hand, to keep the method and body (see `send`).
            .max_redirects(0)
            .timeout_global(Some(TIMEOUT))
            .user_agent(concat!("Ferrowl/", env!("CARGO_PKG_VERSION")))
            .build()
            .into();
        // Separate credentials for these servers, when the user set them.
        let (user, password) = if account.dav_username.trim().is_empty() {
            (&account.username, &account.password)
        } else {
            (&account.dav_username, &account.dav_password)
        };
        let auth = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", user.trim(), password));
        Client {
            agent,
            auth: format!("Basic {auth}"),
            kind,
            allow_http: kind.typed(account).starts_with("http://"),
            trusted: trusted_domains(account, kind),
        }
    }

    /// Whether the credentials may go to this URL's host.
    fn may_authenticate(&self, url: &str) -> bool {
        let host = host_of(url);
        self.trusted
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    }

    /// Sends a WebDAV request, following redirects. Returns the final URL,
    /// status and body.
    fn send(
        &self,
        method: &str,
        url: &str,
        depth: Option<&str>,
        body: &str,
    ) -> Result<(String, StatusCode, String)> {
        self.request(method, url, depth, body, "application/xml; charset=utf-8", None)
    }

    /// Stores an item (PUT). `etag` = the version being replaced, so a
    /// change made meanwhile elsewhere is not overwritten; `None` = new item
    /// (refused if one already exists at that URL).
    pub fn put(&self, url: &str, data: &str, content_type: &str, etag: Option<&str>) -> Result<()> {
        let (_, status, _) = self.request("PUT", url, None, data, content_type, Some(etag.unwrap_or("*")))?;
        // Changed (or created) elsewhere since our copy.
        if status == StatusCode::PRECONDITION_FAILED {
            bail!("{}", crate::i18n::t("err_event_changed"));
        }
        if !status.is_success() {
            bail!("PUT: HTTP {status}");
        }
        Ok(())
    }

    /// Deletes an item (a missing one counts as deleted).
    pub fn delete(&self, url: &str, etag: Option<&str>) -> Result<()> {
        let (_, status, _) = self.request("DELETE", url, None, "", "text/plain", etag.or(Some("")))?;
        if status == StatusCode::PRECONDITION_FAILED {
            bail!("{}", crate::i18n::t("err_event_changed"));
        }
        if !status.is_success() && status != StatusCode::NOT_FOUND {
            bail!("DELETE: HTTP {status}");
        }
        Ok(())
    }

    /// `condition`: `Some("*")` = If-None-Match: * (create only), `Some(etag)`
    /// = If-Match, `Some("")` or `None` = unconditional.
    fn request(
        &self,
        method: &str,
        url: &str,
        depth: Option<&str>,
        body: &str,
        content_type: &str,
        condition: Option<&str>,
    ) -> Result<(String, StatusCode, String)> {
        let mut url = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            if !url.starts_with("https://") && !(self.allow_http && url.starts_with("http://")) {
                bail!("{}", self.kind.text("err_dav_insecure", "err_caldav_insecure"));
            }
            let authenticated = self.may_authenticate(&url);
            let mut req = Request::builder()
                .method(method)
                .uri(&url)
                .header("Content-Type", content_type);
            match condition {
                Some("*") => req = req.header("If-None-Match", "*"),
                Some(etag) if !etag.is_empty() => req = req.header("If-Match", etag),
                _ => {}
            }
            if authenticated {
                req = req.header("Authorization", &self.auth);
            }
            if let Some(d) = depth {
                req = req.header("Depth", d);
            }
            let req = req.body(body.to_owned())?;
            let mut resp = self.agent.run(req).map_err(|e| {
                let key = match self.kind {
                    Kind::Contacts => "err_dav_connect",
                    Kind::Calendar => "err_caldav_connect",
                };
                anyhow!(crate::i18n::tf(key, &[("msg", &e.to_string())]))
            })?;
            let status = resp.status();
            if status.is_redirection() {
                let to = resp
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| anyhow!("redirect without Location"))?;
                url = join(&url, to);
                continue;
            }
            // A refusal of our credentials. Elsewhere a 401 just means this
            // is not the server we look for (discovery goes on).
            if authenticated && (status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN) {
                return Err(AuthRefused(self.kind).into());
            }
            let text = resp
                .body_mut()
                .with_config()
                .limit(MAX_BODY)
                .read_to_string()
                .unwrap_or_default();
            return Ok((url, status, text));
        }
        bail!("too many redirects")
    }

    /// PROPFIND returning the multistatus responses (empty if the URL is
    /// not a WebDAV resource).
    fn propfind(&self, url: &str, depth: &str, props: &str) -> Result<(String, Vec<Response>)> {
        let body = format!(
            r#"<?xml version="1.0" encoding="utf-8"?><d:propfind xmlns:d="DAV:" xmlns:card="urn:ietf:params:xml:ns:carddav" xmlns:cal="urn:ietf:params:xml:ns:caldav" xmlns:ical="http://apple.com/ns/ical/"><d:prop>{props}</d:prop></d:propfind>"#
        );
        let (url, status, text) = self.send("PROPFIND", url, Some(depth), &body)?;
        if status.as_u16() != 207 {
            return Ok((url, Vec::new()));
        }
        Ok((url.clone(), parse_multistatus(&text, &url)?))
    }

    /// Finds the account's address books or calendars.
    pub fn discover(&self, account: &Account) -> Result<Vec<Collection>> {
        for start in start_urls(account, self.kind) {
            match self.collections_from(&start) {
                Ok(books) if !books.is_empty() => return Ok(books),
                // A wrong password ends the search: trying other servers
                // with it would not help.
                Err(e) if e.is::<AuthRefused>() => return Err(e),
                _ => {}
            }
        }
        bail!("{}", self.kind.not_found())
    }

    fn collections_from(&self, start: &str) -> Result<Vec<Collection>> {
        const DESCRIBE: &str =
            "<d:resourcetype/><d:displayname/><ical:calendar-color/><cal:supported-calendar-component-set/>";
        let home_set = self.kind.home_set();
        // The start URL may be a context path, the principal, the home
        // set or a collection itself: ask for everything at once.
        let (url, found) = self.propfind(
            start,
            "0",
            &format!("<d:current-user-principal/>{home_set}{DESCRIBE}"),
        )?;
        let Some(here) = found.first() else {
            return Ok(Vec::new());
        };
        if here.is(self.kind) {
            return Ok(vec![here.collection(&url)]);
        }
        let mut home = here.home.clone();
        if home.is_none()
            && let Some(principal) = &here.principal
        {
            let (_, p) = self.propfind(principal, "0", home_set)?;
            home = p.first().and_then(|r| r.home.clone());
        }
        let home = home.unwrap_or(url);
        let (_, list) = self.propfind(&home, "1", DESCRIBE)?;
        Ok(list
            .iter()
            .filter(|r| r.is(self.kind))
            .map(|r| r.collection(&r.href))
            .collect())
    }

    /// Lists a collection's items as (url, etag).
    fn etags(&self, collection: &str) -> Result<Vec<(String, String)>> {
        let (_, list) = self.propfind(collection, "1", "<d:getetag/><d:resourcetype/>")?;
        Ok(list
            .into_iter()
            .filter(|r| !r.is_collection && !r.etag.is_empty() && !same_url(&r.href, collection))
            .map(|r| (r.href, r.etag))
            .collect())
    }

    /// Downloads items by URL (multiget).
    fn multiget(&self, collection: &str, urls: &[String]) -> Result<Vec<Item>> {
        let mut out = Vec::new();
        for chunk in urls.chunks(BATCH) {
            let hrefs: String = chunk
                .iter()
                .map(|u| format!("<d:href>{}</d:href>", xml_escape(&path_of(u))))
                .collect();
            let (url, status, text) =
                self.send("REPORT", collection, Some("1"), &self.kind.multiget(&hrefs))?;
            if status.as_u16() != 207 {
                bail!("multiget: HTTP {status}");
            }
            for r in parse_multistatus(&text, &url)? {
                if let Some(data) = r.data {
                    out.push(Item {
                        url: r.href,
                        etag: r.etag,
                        data,
                    });
                }
            }
        }
        Ok(out)
    }

    /// Compares the server with `known` (item URL -> ETag, from the cache)
    /// and downloads what changed.
    pub fn sync(&self, account: &Account, known: &[(String, String)]) -> Result<Changes> {
        let collections = self.discover(account)?;
        let mut changes = Changes::default();
        let mut seen = std::collections::HashSet::new();
        for c in &collections {
            let list = self.etags(&c.url).with_context(|| c.name.clone())?;
            let wanted: Vec<String> = list
                .iter()
                .filter(|(url, etag)| !known.iter().any(|(u, e)| u == url && e == etag))
                .map(|(url, _)| url.clone())
                .collect();
            seen.extend(list.into_iter().map(|(u, _)| u));
            for item in self.multiget(&c.url, &wanted)? {
                changes.items.push((c.url.clone(), item));
            }
        }
        changes.gone = known
            .iter()
            .filter(|(u, _)| !seen.contains(u))
            .map(|(u, _)| u.clone())
            .collect();
        changes.collections = collections;
        Ok(changes)
    }
}

/// Where discovery starts, in order.
fn start_urls(account: &Account, kind: Kind) -> Vec<String> {
    let typed = kind.typed(account);
    if !typed.is_empty() {
        let url = if typed.contains("://") {
            typed.to_owned()
        } else {
            format!("https://{typed}")
        };
        return vec![url];
    }
    let domain = account
        .email
        .trim()
        .rsplit_once('@')
        .map(|(_, d)| d.to_ascii_lowercase())
        .unwrap_or_default();
    let mut out = Vec::new();
    if let Some((_, url)) = kind.known().iter().find(|(d, _)| d.contains(&domain.as_str())) {
        out.push(url.to_string());
    }
    if !domain.is_empty() {
        out.push(format!("https://{domain}/.well-known/{}", kind.well_known()));
    }
    out
}

/// Domains of the account's provider: the typed server, a known
/// provider's server, and the IMAP and SMTP servers' domains.
fn trusted_domains(account: &Account, kind: Kind) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    let typed = kind.typed(account);
    if !typed.is_empty() {
        // The exact host typed by the user (not its parent domain).
        let url = if typed.contains("://") {
            typed.to_owned()
        } else {
            format!("https://{typed}")
        };
        hosts.push(host_of(&url));
    }
    // Separate credentials for these servers (a calendar kept elsewhere):
    // they go only to the typed server's domain (its sibling hosts too,
    // e.g. iCloud's pNN-caldav), never to the mail provider's.
    if !account.dav_username.trim().is_empty() {
        let mut hosts: Vec<String> = hosts.iter().filter_map(|h| base_domain(h)).collect();
        hosts.retain(|h| !h.is_empty());
        hosts.dedup();
        return hosts;
    }
    let mut bases = vec![account.imap_host.clone(), account.smtp_host.clone()];
    bases.extend(
        start_urls(account, kind)
            .iter()
            .filter(|u| !u.contains("/.well-known/"))
            .map(|u| host_of(u)),
    );
    hosts.extend(bases.iter().filter_map(|h| base_domain(h)));
    hosts.retain(|h| !h.is_empty());
    hosts.dedup();
    hosts
}

/// Lowercase host of a URL, without port or credentials.
fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit('@').next().unwrap_or_default();
    let host = if authority.starts_with('[') {
        authority
            .split(']')
            .next()
            .map(|h| format!("{h}]"))
            .unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default().to_owned()
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// The registrable domain of a host: "imap.mail.example.co.uk" ->
/// "example.co.uk". Second-level labels like "co" or "com" under a country
/// code are kept, so a whole public suffix is never trusted. IP addresses
/// and single labels are returned as they are.
fn base_domain(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    if host.parse::<std::net::IpAddr>().is_ok() || host.starts_with('[') {
        return Some(host);
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() <= 2 {
        return Some(host);
    }
    let n = labels.len();
    let second = labels[n - 2];
    let generic = ["co", "com", "net", "org", "gov", "ac", "edu", "or", "ne", "go"];
    let keep = if labels[n - 1].len() == 2 && generic.contains(&second) {
        3
    } else {
        2
    };
    Some(labels[n.saturating_sub(keep)..].join("."))
}

/// One `<d:response>` of a multistatus, with the properties we read.
#[derive(Debug, Default)]
struct Response {
    href: String,
    etag: String,
    name: String,
    color: String,
    is_collection: bool,
    is_book: bool,
    is_calendar: bool,
    /// Component types a calendar holds (empty = not stated).
    components: Vec<String>,
    principal: Option<String>,
    home: Option<String>,
    data: Option<String>,
}

impl Response {
    /// Whether this is a collection of the wanted kind. Calendars that
    /// only hold tasks (VTODO lists) are left out.
    fn is(&self, kind: Kind) -> bool {
        match kind {
            Kind::Contacts => self.is_book,
            Kind::Calendar => {
                self.is_calendar
                    && (self.components.is_empty() || self.components.iter().any(|c| c == "VEVENT"))
            }
        }
    }

    fn collection(&self, url: &str) -> Collection {
        let name = if self.name.trim().is_empty() {
            last_segment(url)
        } else {
            self.name.trim().to_owned()
        };
        Collection {
            url: url.to_owned(),
            name,
            color: self.color.trim().to_owned(),
        }
    }
}

/// Reads a 207 Multi-Status body. Hrefs are made absolute against `base`.
/// Properties in a non-200 propstat (not found) are ignored.
fn parse_multistatus(xml: &str, base: &str) -> Result<Vec<Response>> {
    let doc = roxmltree::Document::parse(xml).context("reading the server's reply")?;
    let href_in = |n: roxmltree::Node| {
        child(n, NS_DAV, "href")
            .and_then(|h| h.text())
            .map(|t| join(base, t.trim()))
    };
    let mut out = Vec::new();
    for resp in doc.descendants().filter(|n| is(n, NS_DAV, "response")) {
        let Some(href) = href_in(resp) else { continue };
        let mut r = Response {
            href,
            ..Default::default()
        };
        for ps in resp.children().filter(|n| is(n, NS_DAV, "propstat")) {
            let ok = child(ps, NS_DAV, "status")
                .and_then(|s| s.text())
                .is_none_or(|s| s.contains(" 200"));
            let Some(prop) = child(ps, NS_DAV, "prop") else {
                continue;
            };
            if !ok {
                continue;
            }
            for p in prop.children().filter(roxmltree::Node::is_element) {
                let text = || p.text().unwrap_or_default().to_owned();
                match (p.tag_name().namespace(), p.tag_name().name()) {
                    (Some(NS_DAV), "getetag") => r.etag = text().trim().to_owned(),
                    (Some(NS_DAV), "displayname") => r.name = text(),
                    (Some(NS_DAV), "resourcetype") => {
                        r.is_collection = child(p, NS_DAV, "collection").is_some();
                        r.is_book = child(p, NS_CARD, "addressbook").is_some();
                        r.is_calendar = child(p, NS_CAL, "calendar").is_some();
                    }
                    (Some(NS_APPLE), "calendar-color") => r.color = text(),
                    (Some(NS_CAL), "supported-calendar-component-set") => {
                        r.components = p
                            .children()
                            .filter(|c| is(c, NS_CAL, "comp"))
                            .filter_map(|c| c.attribute("name"))
                            .map(str::to_ascii_uppercase)
                            .collect()
                    }
                    (Some(NS_DAV), "current-user-principal") => r.principal = href_in(p),
                    (Some(NS_CARD), "addressbook-home-set") | (Some(NS_CAL), "calendar-home-set") => {
                        r.home = href_in(p)
                    }
                    (Some(NS_CARD), "address-data") | (Some(NS_CAL), "calendar-data") => {
                        r.data = Some(p.children().filter_map(|c| c.text()).collect())
                    }
                    _ => {}
                }
            }
        }
        out.push(r);
    }
    Ok(out)
}

fn is(n: &roxmltree::Node, ns: &str, name: &str) -> bool {
    n.is_element() && n.tag_name().name() == name && n.tag_name().namespace() == Some(ns)
}

fn child<'a, 'i>(n: roxmltree::Node<'a, 'i>, ns: &str, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
    n.children().find(|c| is(c, ns, name))
}

/// Resolves `href` (absolute URL, absolute path or relative path) against
/// `base`.
pub(crate) fn join(base: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_owned();
    }
    let origin_end = base
        .find("://")
        .map(|i| i + 3)
        .and_then(|start| base[start..].find('/').map(|j| start + j))
        .unwrap_or(base.len());
    if href.starts_with('/') {
        return format!("{}{href}", &base[..origin_end]);
    }
    let dir_end = base
        .rfind('/')
        .filter(|&i| i >= origin_end)
        .map_or(base.len(), |i| i + 1);
    let mut dir = base[..dir_end].to_owned();
    if !dir.ends_with('/') {
        dir.push('/');
    }
    format!("{dir}{href}")
}

/// The path part of an absolute URL (what servers expect in `<d:href>`).
fn path_of(url: &str) -> String {
    url.find("://")
        .and_then(|i| url[i + 3..].find('/').map(|j| url[i + 3 + j..].to_owned()))
        .unwrap_or_else(|| url.to_owned())
}

fn same_url(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

fn last_segment(url: &str) -> String {
    let s = url.trim_end_matches('/');
    let s = s.rsplit('/').next().unwrap_or(s);
    percent_decode(s)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        if let (b'%', Some(b)) = (bytes[i], hex) {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn joins_hrefs() {
        let base = "https://dav.example.com/dav/books/";
        assert_eq!(join(base, "/p/x.vcf"), "https://dav.example.com/p/x.vcf");
        assert_eq!(join(base, "x.vcf"), "https://dav.example.com/dav/books/x.vcf");
        assert_eq!(join("https://h.it", "/a"), "https://h.it/a");
        assert_eq!(join(base, "https://o.it/a"), "https://o.it/a");
        assert_eq!(path_of("https://h.it:8443/a/b.vcf"), "/a/b.vcf");
        assert_eq!(last_segment("https://h.it/a/Lavoro%20e%20casa/"), "Lavoro e casa");
    }

    #[test]
    fn reads_calendar_collections() {
        let xml = r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:a="http://apple.com/ns/ical/">
<d:response><d:href>/cal/user/work/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>Lavoro</d:displayname><a:calendar-color>#A63A0CFF</a:calendar-color><c:supported-calendar-component-set><c:comp name="VEVENT"/></c:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
<d:response><d:href>/cal/user/tasks/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><c:supported-calendar-component-set><c:comp name="VTODO"/></c:supported-calendar-component-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
<d:response><d:href>/cal/user/</d:href><d:propstat><d:prop><c:calendar-home-set><d:href>/cal/user/</d:href></c:calendar-home-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>
</d:multistatus>"#;
        let list = parse_multistatus(xml, "https://dav.example.org/").unwrap();
        let cals: Vec<Collection> = list
            .iter()
            .filter(|r| r.is(Kind::Calendar))
            .map(|r| r.collection(&r.href))
            .collect();
        assert_eq!(
            cals,
            [Collection {
                url: "https://dav.example.org/cal/user/work/".into(),
                name: "Lavoro".into(),
                color: "#A63A0CFF".into(),
            }],
            "task lists are left out"
        );
        assert_eq!(list[2].home.as_deref(), Some("https://dav.example.org/cal/user/"));
        assert!(!list[0].is(Kind::Contacts));
    }

    #[test]
    fn credentials_only_go_to_the_provider() {
        let account = Account {
            email: "me@shop.example".into(),
            imap_host: "imap.mailhost.it".into(),
            smtp_host: "smtp.mailhost.it".into(),
            ..Default::default()
        };
        let c = Client::new(&account, Kind::Contacts);
        assert!(c.may_authenticate("https://dav.mailhost.it/x"));
        assert!(c.may_authenticate("https://mailhost.it/.well-known/carddav"));
        // The custom domain's website may be hosted by a third party.
        assert!(!c.may_authenticate("https://shop.example/.well-known/carddav"));
        assert!(!c.may_authenticate("https://evil-mailhost.it/"));

        let icloud = Account {
            email: "me@icloud.com".into(),
            imap_host: "imap.mail.me.com".into(),
            ..Default::default()
        };
        let c = Client::new(&icloud, Kind::Contacts);
        assert!(c.may_authenticate("https://p42-contacts.icloud.com/123/carddavhome/"));

        assert_eq!(
            base_domain("imap.mail.example.co.uk").as_deref(),
            Some("example.co.uk")
        );
        assert_eq!(base_domain("mail.example.it").as_deref(), Some("example.it"));
        assert_eq!(host_of("https://u@Dav.Example.org:8443/p"), "dav.example.org");
    }

    #[test]
    fn start_urls_by_domain() {
        let a = |email: &str, url: &str| Account {
            email: email.into(),
            carddav_url: url.into(),
            ..Default::default()
        };
        assert_eq!(
            start_urls(&a("x@icloud.com", ""), Kind::Contacts),
            [
                "https://contacts.icloud.com/",
                "https://icloud.com/.well-known/carddav"
            ]
        );
        assert_eq!(
            start_urls(&a("x@example.org", ""), Kind::Contacts),
            ["https://example.org/.well-known/carddav"]
        );
        assert_eq!(
            start_urls(&a("x@example.org", "dav.example.org/c"), Kind::Contacts),
            ["https://dav.example.org/c"]
        );
    }

    #[test]
    fn insecure_urls_are_refused() {
        let mut account = Account {
            email: "x@example.org".into(),
            ..Default::default()
        };
        let c = Client::new(&account, Kind::Contacts);
        assert!(c.send("PROPFIND", "http://example.org/", None, "").is_err());
        account.carddav_url = "http://127.0.0.1:1/".into();
        assert!(Client::new(&account, Kind::Contacts).allow_http);
    }

    /// (file name, ETag, vCard) of each card on the fake server.
    type Cards = Arc<Mutex<Vec<(&'static str, &'static str, String)>>>;

    /// A minimal CardDAV server: the well-known path redirects to a
    /// principal, which points to a home with one address book of two
    /// cards. Records every request line.
    fn fake_server(cards: Cards) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let log = Arc::new(Mutex::new(Vec::new()));
        let log2 = log.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut len = 0usize;
                let mut auth = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if lower.starts_with("authorization:") {
                        auth = line.trim().to_owned();
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let body = String::from_utf8(body).unwrap();
                let mut it = first.split_whitespace();
                let (method, path) = (it.next().unwrap().to_owned(), it.next().unwrap().to_owned());
                log2.lock().unwrap().push(format!("{method} {path}"));
                let ms = |inner: String| {
                    (
                        207,
                        format!(
                            r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:carddav">{inner}</d:multistatus>"#
                        ),
                        String::new(),
                    )
                };
                // "user:secret" in base64.
                let (status, text, location) = if !auth.ends_with("dXNlcjpzZWNyZXQ=") {
                    (401, String::new(), String::new())
                } else {
                    match (method.as_str(), path.as_str()) {
                        ("PROPFIND", "/.well-known/carddav") => (301, String::new(), "/dav/".to_owned()),
                        ("PROPFIND", "/dav/") => ms(r#"<d:response><d:href>/dav/</d:href><d:propstat><d:prop><d:current-user-principal><d:href>/dav/principals/user/</d:href></d:current-user-principal><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat><d:propstat><d:prop><c:addressbook-home-set/></d:prop><d:status>HTTP/1.1 404 Not Found</d:status></d:propstat></d:response>"#.into()),
                        ("PROPFIND", "/dav/principals/user/") => ms(r#"<d:response><d:href>/dav/principals/user/</d:href><d:propstat><d:prop><c:addressbook-home-set><d:href>/dav/books/user/</d:href></c:addressbook-home-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#.into()),
                        ("PROPFIND", "/dav/books/user/") => ms(r#"<d:response><d:href>/dav/books/user/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response><d:response><d:href>/dav/books/user/main/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:addressbook/></d:resourcetype><d:displayname>Personal</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#.into()),
                        ("PROPFIND", "/dav/books/user/main/") => {
                            let mut inner = String::from(r#"<d:response><d:href>/dav/books/user/main/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:addressbook/></d:resourcetype><d:getetag/></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#);
                            for (name, etag, _) in cards.lock().unwrap().iter() {
                                inner += &format!(r#"<d:response><d:href>/dav/books/user/main/{name}</d:href><d:propstat><d:prop><d:getetag>{etag}</d:getetag><d:resourcetype/></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#);
                            }
                            ms(inner)
                        }
                        ("REPORT", "/dav/books/user/main/") => {
                            let mut inner = String::new();
                            for (name, etag, data) in cards.lock().unwrap().iter() {
                                if body.contains(&format!("/main/{name}<")) {
                                    inner += &format!(r#"<d:response><d:href>/dav/books/user/main/{name}</d:href><d:propstat><d:prop><d:getetag>{etag}</d:getetag><c:address-data>{data}</c:address-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#);
                                }
                            }
                            ms(inner)
                        }
                        _ => (404, String::new(), String::new()),
                    }
                };
                let reason = match status {
                    207 => "Multi-Status",
                    301 => "Moved",
                    401 => "Unauthorized",
                    _ => "Not Found",
                };
                let loc = if location.is_empty() {
                    String::new()
                } else {
                    format!("Location: {location}\r\n")
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\n{loc}Content-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        (base, log)
    }

    #[test]
    fn discovers_and_syncs_with_etags() {
        let name = |data: &str| crate::vcard::parse(data).unwrap().name;
        let card = |fname: &str, email: &str| {
            format!("BEGIN:VCARD&#13;\nVERSION:4.0\nFN:{fname}\nEMAIL:{email}\nEND:VCARD\n")
        };
        let cards = Arc::new(Mutex::new(vec![
            ("a.vcf", "\"1\"", card("Anna Bianchi", "anna@example.org")),
            ("b.vcf", "\"1\"", card("Bruno Verdi", "bruno@example.org")),
        ]));
        let (base, log) = fake_server(cards.clone());
        let mut account = Account {
            email: "user@example.org".into(),
            username: "user".into(),
            password: "secret".into(),
            carddav_url: format!("{base}/.well-known/carddav"),
            ..Default::default()
        };
        let client = Client::new(&account, Kind::Contacts);

        // First sync: discovery through the redirect, both cards fetched.
        let first = client.sync(&account, &[]).unwrap();
        assert_eq!(
            first.collections,
            [Collection {
                url: format!("{base}/dav/books/user/main/"),
                name: "Personal".into(),
                ..Default::default()
            }]
        );
        let mut names: Vec<_> = first.items.iter().map(|(_, c)| name(&c.data)).collect();
        names.sort();
        assert_eq!(names, ["Anna Bianchi", "Bruno Verdi"]);
        assert!(first.gone.is_empty());

        // Second sync: one card changed, one deleted, one unknown in cache.
        let known: Vec<(String, String)> = first
            .items
            .iter()
            .map(|(_, c)| (c.url.clone(), c.etag.clone()))
            .collect();
        {
            let mut c = cards.lock().unwrap();
            c[0].1 = "\"2\"";
            c[0].2 = card("Anna Bianchi Rossi", "anna@example.org");
            c.remove(1);
        }
        log.lock().unwrap().clear();
        let second = client.sync(&account, &known).unwrap();
        assert_eq!(second.items.len(), 1);
        assert_eq!(name(&second.items[0].1.data), "Anna Bianchi Rossi");
        assert_eq!(second.gone, [format!("{base}/dav/books/user/main/b.vcf")]);
        let reports = log
            .lock()
            .unwrap()
            .iter()
            .filter(|l| l.starts_with("REPORT"))
            .count();
        assert_eq!(reports, 1, "only changed cards are downloaded");

        // Wrong password: a clear error, not "no address book".
        account.password = "wrong".into();
        let err = Client::new(&account, Kind::Contacts)
            .sync(&account, &[])
            .unwrap_err();
        assert!(err.is::<AuthRefused>(), "{err:#}");
    }

    /// A tiny CalDAV server keeping events in memory, with ETags and
    /// conditional PUT/DELETE. The typed URL is the calendar itself.
    fn fake_caldav() -> String {
        use std::collections::BTreeMap;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let mut items: BTreeMap<String, (u32, String)> = BTreeMap::new();
            let mut version = 0u32;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let (mut len, mut if_match, mut if_none) = (0usize, None::<String>, false);
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line.trim().is_empty() {
                        break;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if let Some(v) = line.strip_prefix("If-Match:").or(line.strip_prefix("if-match:")) {
                        if_match = Some(v.trim().to_owned());
                    }
                    if lower.starts_with("if-none-match:") {
                        if_none = true;
                    }
                }
                let mut body = vec![0; len];
                reader.read_exact(&mut body).unwrap();
                let body = String::from_utf8(body).unwrap();
                let mut it = first.split_whitespace();
                let (method, path) = (it.next().unwrap().to_owned(), it.next().unwrap().to_owned());
                let etag = |v: u32| format!("\"{v}\"");
                let ms = |inner: String| {
                    (
                        207,
                        format!(
                            r#"<?xml version="1.0"?><d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">{inner}</d:multistatus>"#
                        ),
                    )
                };
                let (status, text) = match (method.as_str(), path.as_str()) {
                    ("PROPFIND", "/cal/") => {
                        let mut inner = String::from(
                            r#"<d:response><d:href>/cal/</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><c:calendar/></d:resourcetype><d:displayname>Lavoro</d:displayname></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#,
                        );
                        if !body.contains("current-user-principal") {
                            for (name, (v, _)) in &items {
                                inner += &format!(
                                    r#"<d:response><d:href>/cal/{name}</d:href><d:propstat><d:prop><d:getetag>{}</d:getetag><d:resourcetype/></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#,
                                    etag(*v)
                                );
                            }
                        }
                        ms(inner)
                    }
                    ("REPORT", "/cal/") => {
                        let mut inner = String::new();
                        for (name, (v, data)) in &items {
                            if body.contains(&format!("/cal/{name}<")) {
                                inner += &format!(
                                    r#"<d:response><d:href>/cal/{name}</d:href><d:propstat><d:prop><d:getetag>{}</d:getetag><c:calendar-data>{}</c:calendar-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"#,
                                    etag(*v),
                                    xml_escape(data)
                                );
                            }
                        }
                        ms(inner)
                    }
                    ("PUT", p) if p.starts_with("/cal/") => {
                        let name = p[5..].to_owned();
                        let current = items.get(&name).map(|(v, _)| etag(*v));
                        let ok = match (&if_match, if_none) {
                            (Some(m), _) => current.as_deref() == Some(m.as_str()),
                            (None, true) => current.is_none(),
                            _ => true,
                        };
                        if ok {
                            version += 1;
                            items.insert(name, (version, body.clone()));
                            (201, String::new())
                        } else {
                            (412, String::new())
                        }
                    }
                    ("DELETE", p) if p.starts_with("/cal/") => {
                        let name = &p[5..];
                        let current = items.get(name).map(|(v, _)| etag(*v));
                        match (&if_match, current) {
                            (_, None) => (404, String::new()),
                            (Some(m), Some(c)) if *m != c => (412, String::new()),
                            _ => {
                                items.remove(name);
                                (204, String::new())
                            }
                        }
                    }
                    _ => (404, String::new()),
                };
                let reason = match status {
                    201 => "Created",
                    204 => "No Content",
                    207 => "Multi-Status",
                    412 => "Precondition Failed",
                    _ => "Not Found",
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            }
        });
        base
    }

    #[test]
    fn creates_edits_and_deletes_events() {
        use crate::events::{self, Draft};
        let base = fake_caldav();
        let account = Account {
            email: "user@example.org".into(),
            caldav_url: format!("{base}/cal/"),
            ..Default::default()
        };
        let c = Client::new(&account, Kind::Calendar);
        let mut draft = Draft::new_on(chrono::NaiveDate::from_ymd_opt(2026, 12, 1).unwrap());
        draft.summary = "Kickoff".into();
        let (uid, ics) = events::create(&draft);
        let url = crate::invite::item_url(&format!("{base}/cal/"), &uid);
        c.put(&url, &ics, "text/calendar", None).unwrap();
        // Creating again at the same URL is refused (If-None-Match).
        assert!(c.put(&url, &ics, "text/calendar", None).is_err());

        let first = c.sync(&account, &[]).unwrap();
        assert_eq!(first.items.len(), 1);
        let item = &first.items[0].1;
        assert_eq!(crate::ical::parse(&item.data).events[0].summary, "Kickoff");

        draft.summary = "Kickoff (moved)".into();
        let changed = events::update(&item.data, &draft);
        c.put(&url, &changed, "text/calendar", Some(&item.etag)).unwrap();
        // The old ETag no longer matches: a clear "changed elsewhere".
        let stale = c
            .put(&url, &changed, "text/calendar", Some(&item.etag))
            .unwrap_err();
        assert!(
            !stale.to_string().starts_with("PUT:"),
            "412 is reported as a conflict"
        );

        let known = vec![(item.url.clone(), item.etag.clone())];
        let second = c.sync(&account, &known).unwrap();
        assert_eq!(
            crate::ical::parse(&second.items[0].1.data).events[0].summary,
            "Kickoff (moved)"
        );
        assert_eq!(crate::ical::parse(&second.items[0].1.data).events[0].sequence, 1);

        c.delete(&url, Some(&second.items[0].1.etag)).unwrap();
        let third = c
            .sync(&account, &[(url.clone(), second.items[0].1.etag.clone())])
            .unwrap();
        assert_eq!(third.gone, [url]);
    }

    #[test]
    fn separate_credentials_stay_with_the_typed_server() {
        let account = Account {
            email: "me@aruba.it".into(),
            imap_host: "imaps.aruba.it".into(),
            smtp_host: "smtps.aruba.it".into(),
            caldav_url: "https://caldav.icloud.com".into(),
            dav_username: "me@icloud.com".into(),
            dav_password: "app-password".into(),
            ..Default::default()
        };
        let trusted = trusted_domains(&account, Kind::Calendar);
        assert_eq!(trusted, ["icloud.com"]);
        let client = Client::new(&account, Kind::Calendar);
        assert!(client.may_authenticate("https://p42-caldav.icloud.com/123/calendars/"));
        assert!(!client.may_authenticate("https://caldav.aruba.it/"));
    }
}
