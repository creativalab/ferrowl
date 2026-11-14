//! OAuth2 sign-in for Microsoft (Outlook.com, Microsoft 365) and Google.
//!
//! Desktop flow with PKCE (RFC 8252): Ferrowl opens the provider's sign-in
//! page in the system browser, listens once on a loopback port for the
//! redirect, and exchanges the code for tokens. The password never reaches
//! Ferrowl. IMAP and SMTP then authenticate with XOAUTH2 and a short-lived
//! access token, renewed from the refresh token when needed.
//!
//! Refresh tokens are kept in the system keychain (or, where there is none,
//! in `oauth-tokens.json` in the data directory, owner-only), not in the
//! settings file: providers rotate them on use and the latest one must
//! always be saved.
//!
//! The engine does not open the browser itself: the app passes a function
//! that does (desktop: the default browser; mobile: a custom tab).

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Microsoft,
    Google,
}

/// Application (client) IDs from the providers' developer consoles. Public
/// values for a desktop app; an environment variable at build time overrides
/// the default.
const MICROSOFT_CLIENT_ID: &str = match option_env!("FERROWL_MICROSOFT_CLIENT_ID") {
    Some(id) => id,
    // "Ferrowl" app registration (public client) in the Creativalab
    // Microsoft Entra tenant, publisher domain ferrowl.com.
    None => "40824528-0620-4dc4-9eee-84f31a1637f5",
};
const GOOGLE_DESKTOP_CLIENT_ID: &str = match option_env!("FERROWL_GOOGLE_CLIENT_ID") {
    Some(id) => id,
    // "Ferrowl desktop" OAuth client of the Google Cloud project "Ferrowl".
    None => "72326679637-br76ngo5he65p71eesvk2qdjmb63qqhj.apps.googleusercontent.com",
};
/// "Ferrowl iOS" OAuth client (bundle id com.ferrowl.app): no secret, the
/// answer comes back to the app's own address (see [GOOGLE_IOS_REDIRECT]).
pub const GOOGLE_IOS_CLIENT_ID: &str =
    "72326679637-s3g4uak2k5861mat9p2ar1m5h32i0ibn.apps.googleusercontent.com";
/// Where Google sends the iOS app back: the client id reversed, as Google
/// requires for iOS clients.
pub const GOOGLE_IOS_REDIRECT: &str =
    "com.googleusercontent.apps.72326679637-s3g4uak2k5861mat9p2ar1m5h32i0ibn:/oauth2redirect";
const GOOGLE_CLIENT_ID: &str = if cfg!(target_os = "ios") {
    GOOGLE_IOS_CLIENT_ID
} else {
    GOOGLE_DESKTOP_CLIENT_ID
};
/// Google requires a "secret" even for installed apps; it is not
/// confidential (it ships in every copy of the app), but it is kept out of
/// the public repository and given at build time.
const GOOGLE_CLIENT_SECRET: &str = match option_env!("FERROWL_GOOGLE_CLIENT_SECRET") {
    // The iOS client has no secret.
    Some(s) if !cfg!(target_os = "ios") => s,
    _ => "",
};

struct Endpoints {
    authorize: &'static str,
    token: &'static str,
    scope: &'static str,
    client_id: &'static str,
    client_secret: &'static str,
}

impl Provider {
    fn endpoints(self) -> Endpoints {
        match self {
            Provider::Microsoft => Endpoints {
                authorize: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                token: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
                scope: "openid email offline_access https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send",
                client_id: MICROSOFT_CLIENT_ID,
                client_secret: "",
            },
            Provider::Google => Endpoints {
                authorize: "https://accounts.google.com/o/oauth2/v2/auth",
                token: "https://oauth2.googleapis.com/token",
                scope: "openid email https://mail.google.com/",
                client_id: GOOGLE_CLIENT_ID,
                client_secret: GOOGLE_CLIENT_SECRET,
            },
        }
    }

    /// Whether this build has an application ID for the provider.
    pub fn available(self) -> bool {
        let e = self.endpoints();
        // Google also needs its client secret (given at build time), except
        // with the iOS client.
        !e.client_id.is_empty()
            && (self != Provider::Google || cfg!(target_os = "ios") || !e.client_secret.is_empty())
    }

    /// IMAP and SMTP servers for an address of this provider.
    pub fn servers(self, email: &str) -> ((&'static str, u16), (&'static str, u16)) {
        match self {
            Provider::Microsoft => {
                let domain = email.rsplit('@').next().unwrap_or("").to_ascii_lowercase();
                let consumer = ["outlook.", "hotmail.", "live.", "msn."]
                    .iter()
                    .any(|p| domain.starts_with(p));
                let smtp = if consumer {
                    "smtp-mail.outlook.com"
                } else {
                    "smtp.office365.com"
                };
                (("outlook.office365.com", 993), (smtp, 587))
            }
            Provider::Google => (("imap.gmail.com", 993), ("smtp.gmail.com", 465)),
        }
    }

    /// Guess from the address (only for well-known consumer domains).
    pub fn for_email(email: &str) -> Option<Provider> {
        let domain = email.rsplit('@').next()?.to_ascii_lowercase();
        if ["outlook.", "hotmail.", "live.", "msn."]
            .iter()
            .any(|p| domain.starts_with(p))
        {
            Some(Provider::Microsoft)
        } else if domain == "gmail.com" || domain == "googlemail.com" {
            Some(Provider::Google)
        } else {
            None
        }
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

// ------------------------------------------------------------ token store

fn store_path() -> Result<PathBuf> {
    Ok(crate::paths::data_dir()
        .context("data directory not found")?
        .join("oauth-tokens.json"))
}

fn key(provider: Provider, email: &str) -> String {
    format!("{provider:?}:{}", email.trim().to_lowercase())
}

/// Keychain entry name of a refresh token.
fn keychain_entry(provider: Provider, email: &str) -> String {
    format!("oauth:{}", key(provider, email))
}

fn load_store() -> HashMap<String, String> {
    store_path()
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_store(store: &HashMap<String, String>) -> Result<()> {
    let path = store_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(store)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Where the mobile apps keep refresh tokens (Android Keystore, iOS
/// Keychain). When set, it replaces the system keychain and the file.
static VAULT: OnceLock<Box<dyn crate::secrets::Store>> = OnceLock::new();

/// Hands refresh-token storage to the app (once, at start-up).
pub fn set_vault(vault: Box<dyn crate::secrets::Store>) {
    let _ = VAULT.set(vault);
}

fn load_refresh_token(provider: Provider, email: &str) -> Option<String> {
    use crate::secrets::Store;
    if let Some(vault) = VAULT.get() {
        return vault.get(&keychain_entry(provider, email));
    }
    crate::secrets::SystemKeychain
        .get(&keychain_entry(provider, email))
        .or_else(|| load_store().get(&key(provider, email)).cloned())
}

fn save_refresh_token(provider: Provider, email: &str, token: &str) -> Result<()> {
    use crate::secrets::Store;
    if let Some(vault) = VAULT.get() {
        if !vault.set(&keychain_entry(provider, email), token) {
            bail!("could not save the sign-in on this device");
        }
        return Ok(());
    }
    let _guard = file_lock().lock().unwrap_or_else(|p| p.into_inner());
    if crate::secrets::SystemKeychain.set(&keychain_entry(provider, email), token) {
        // In the keychain now: drop any older copy from the file.
        let mut store = load_store();
        if store.remove(&key(provider, email)).is_some() {
            write_store(&store)?;
        }
        return Ok(());
    }
    let mut store = load_store();
    store.insert(key(provider, email), token.to_string());
    write_store(&store)
}

/// Forgets the tokens of an account (sign-out / account removed).
pub fn forget(provider: Provider, email: &str) {
    use crate::secrets::Store;
    if let Some(vault) = VAULT.get() {
        vault.delete(&keychain_entry(provider, email));
    }
    let _guard = file_lock().lock().unwrap_or_else(|p| p.into_inner());
    crate::secrets::SystemKeychain.delete(&keychain_entry(provider, email));
    let mut store = load_store();
    if store.remove(&key(provider, email)).is_some() {
        let _ = write_store(&store);
    }
    cache()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&key(provider, email));
}

fn file_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Access tokens in memory: key → (token, valid until).
fn cache() -> &'static Mutex<HashMap<String, (String, Instant)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (String, Instant)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

// ------------------------------------------------------------ flows

/// Access tokens the app gets itself (Android: Google Identity Services
/// for Google accounts). `None` = not handled there or not available.
type TokenSource = dyn Fn(Provider, &str) -> Option<String> + Send + Sync;
static SOURCE: OnceLock<Box<TokenSource>> = OnceLock::new();

/// Lets the app provide access tokens (once, at start-up). Asked first;
/// when it answers `None` for a provider it does not handle, the engine's
/// own refresh is used.
pub fn set_token_source(source: Box<TokenSource>) {
    let _ = SOURCE.set(source);
}

/// A valid access token for the account, refreshed when it is about to expire.
pub fn access_token(provider: Provider, email: &str) -> Result<String> {
    if let Some(source) = SOURCE.get()
        && let Some(token) = source(provider, email)
    {
        return Ok(token);
    }
    let k = key(provider, email);
    if let Some((token, until)) = cache().lock().unwrap_or_else(|p| p.into_inner()).get(&k)
        && *until > Instant::now() + Duration::from_secs(60)
    {
        return Ok(token.clone());
    }
    let refresh =
        load_refresh_token(provider, email).ok_or_else(|| anyhow!(crate::i18n::t("err_oauth_signed_out")))?;
    let e = provider.endpoints();
    let mut form = vec![
        ("client_id", e.client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh.as_str()),
    ];
    if !e.client_secret.is_empty() {
        form.push(("client_secret", e.client_secret));
    }
    if provider == Provider::Microsoft {
        form.push(("scope", e.scope));
    }
    let tokens = post_token(e.token, &form)?;
    remember(provider, email, &tokens)?;
    Ok(tokens.access_token)
}

// ------------------------------------------------------------ push server

/// Which OAuth client got a refresh token: a token can only be refreshed
/// by the client it was issued to (Google on iOS has its own client).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Client {
    #[default]
    Standard,
    GoogleIos,
}

/// The sign-in a phone hands to Ferrowl's push server, so it can watch an
/// account that has no password: the refresh token and its client. Sent
/// sealed with the phone's key, like the account itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub refresh_token: String,
    #[serde(default)]
    pub client: Client,
}

/// This device's grant for an account (None when not signed in here).
pub fn grant(provider: Provider, email: &str) -> Option<Grant> {
    let refresh_token = load_refresh_token(provider, email)?;
    let client = if cfg!(target_os = "ios") && provider == Provider::Google {
        Client::GoogleIos
    } else {
        Client::Standard
    };
    Some(Grant {
        refresh_token,
        client,
    })
}

/// A new access token from a refresh.
#[derive(Debug, Clone)]
pub struct Refreshed {
    pub access_token: String,
    pub lifetime: Duration,
    /// Some providers (Microsoft) send a new refresh token: keep it.
    pub refresh_token: Option<String>,
}

/// Refreshes a grant without the device's vault (the push server keeps
/// grants itself).
pub fn refresh_grant(provider: Provider, grant: &Grant) -> Result<Refreshed> {
    let e = provider.endpoints();
    let (client_id, client_secret) = match grant.client {
        Client::GoogleIos => (GOOGLE_IOS_CLIENT_ID, ""),
        Client::Standard => (e.client_id, e.client_secret),
    };
    let mut form = vec![
        ("client_id", client_id),
        ("grant_type", "refresh_token"),
        ("refresh_token", grant.refresh_token.as_str()),
    ];
    if !client_secret.is_empty() {
        form.push(("client_secret", client_secret));
    }
    if provider == Provider::Microsoft {
        form.push(("scope", e.scope));
    }
    let tokens = post_token(e.token, &form)?;
    Ok(Refreshed {
        lifetime: Duration::from_secs(tokens.expires_in.unwrap_or(3600)),
        refresh_token: tokens.refresh_token.filter(|r| r != &grant.refresh_token),
        access_token: tokens.access_token,
    })
}

fn remember(provider: Provider, email: &str, tokens: &TokenResponse) -> Result<()> {
    if let Some(refresh) = &tokens.refresh_token {
        save_refresh_token(provider, email, refresh)?;
    }
    let lifetime = Duration::from_secs(tokens.expires_in.unwrap_or(3600));
    cache().lock().unwrap_or_else(|p| p.into_inner()).insert(
        key(provider, email),
        (tokens.access_token.clone(), Instant::now() + lifetime),
    );
    Ok(())
}

fn post_token(url: &str, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(url)
        .send_form(form.iter().copied())
        .context("token request")?;
    let body = response.body_mut().read_to_string().context("token response")?;
    if let Ok(tokens) = serde_json::from_str::<TokenResponse>(&body) {
        return Ok(tokens);
    }
    match serde_json::from_str::<TokenError>(&body) {
        Ok(err) => bail!("{}: {}", err.error, err.error_description.unwrap_or_default()),
        Err(_) => bail!("unexpected token response ({})", response.status()),
    }
}

/// A sign-in in progress: the page to open, and what is needed to finish it
/// when the provider sends the browser back to `redirect`.
#[derive(Debug, Clone)]
pub struct Pending {
    pub url: String,
    pub state: String,
    pub verifier: String,
    pub redirect: String,
}

/// First half of a sign-in: the provider's page for `redirect` (PKCE).
pub fn begin(provider: Provider, login_hint: &str, redirect: &str) -> Result<Pending> {
    let e = provider.endpoints();
    if e.client_id.is_empty() {
        bail!(crate::i18n::t("err_oauth_unavailable"));
    }
    let verifier = random_string(64);
    let challenge = pkce_challenge(&verifier);
    let state = random_string(24);
    let mut url = url::Url::parse(e.authorize)?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", e.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect)
            .append_pair("scope", e.scope)
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("state", &state);
        if !login_hint.trim().is_empty() {
            q.append_pair("login_hint", login_hint.trim());
        }
        if provider == Provider::Google {
            q.append_pair("access_type", "offline")
                .append_pair("prompt", "consent");
        }
    }
    Ok(Pending {
        url: url.into(),
        state,
        verifier,
        redirect: redirect.to_string(),
    })
}

/// Second half: exchanges the `code` the provider sent back for tokens,
/// stores them and returns the signed-in address.
pub fn finish(provider: Provider, pending: &Pending, code: &str, login_hint: &str) -> Result<String> {
    let e = provider.endpoints();
    let mut form = vec![
        ("client_id", e.client_id),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", pending.redirect.as_str()),
        ("code_verifier", pending.verifier.as_str()),
    ];
    if !e.client_secret.is_empty() {
        form.push(("client_secret", e.client_secret));
    }
    if provider == Provider::Microsoft {
        form.push(("scope", e.scope));
    }
    let tokens = post_token(e.token, &form)?;
    let email = tokens
        .id_token
        .as_deref()
        .and_then(email_from_id_token)
        .unwrap_or_else(|| login_hint.trim().to_string());
    if email.is_empty() {
        bail!("the provider did not return the email address");
    }
    if tokens.refresh_token.is_none() {
        bail!("the provider did not return a refresh token");
    }
    remember(provider, &email, &tokens)?;
    Ok(email)
}

/// The code from a redirect address (`...?code=...&state=...`), after
/// checking it answers `pending`. Provider errors become messages.
pub fn code_from_redirect(pending: &Pending, redirect: &str) -> Result<String> {
    let url = url::Url::parse(redirect).context("sign-in answer")?;
    let params: HashMap<String, String> = url.query_pairs().into_owned().collect();
    if let Some(err) = params.get("error") {
        let desc = params.get("error_description").cloned().unwrap_or_default();
        bail!("{err}: {desc}");
    }
    if params.get("state") != Some(&pending.state) {
        bail!("sign-in response did not match this request");
    }
    params
        .get("code")
        .cloned()
        .ok_or_else(|| anyhow!("sign-in response without a code"))
}

/// Interactive sign-in on a computer: opens the provider's page with
/// `open_browser`, waits for the redirect on a loopback port (up to five
/// minutes) and stores the tokens. Returns the signed-in address.
pub fn sign_in(
    provider: Provider,
    login_hint: &str,
    open_browser: &dyn Fn(&str) -> Result<()>,
) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").context("opening the sign-in port")?;
    let port = listener.local_addr()?.port();
    let pending = begin(provider, login_hint, &format!("http://localhost:{port}"))?;
    open_browser(&pending.url).context("opening the browser")?;
    let code = wait_for_code(&listener, &pending.state, Duration::from_secs(300))?;
    finish(provider, &pending, &code, login_hint)
}

/// Accepts connections on the loopback port until the redirect with our
/// `state` arrives (or the timeout passes) and returns the code.
fn wait_for_code(listener: &TcpListener, state: &str, timeout: Duration) -> Result<String> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    loop {
        if Instant::now() > deadline {
            bail!(crate::i18n::t("err_oauth_timeout"));
        }
        let (mut stream, _) = match listener.accept() {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            // A connection that failed before we took it: wait for the next.
            Err(_) => continue,
        };
        // Browsers open spare connections they may never use, and other
        // programs can knock on the port too: a connection that sends no
        // readable request line is dropped and the wait goes on.
        let mut line = String::new();
        let read = stream
            .set_nonblocking(false)
            .and_then(|_| stream.set_read_timeout(Some(Duration::from_secs(10))))
            .and_then(|_| BufReader::new(&stream).read_line(&mut line));
        if read.is_err() {
            continue;
        }
        // "GET /?code=...&state=... HTTP/1.1"
        let target = line.split_whitespace().nth(1).unwrap_or("/");
        let params: HashMap<String, String> = url::Url::parse(&format!("http://localhost{target}"))
            .map(|u| u.query_pairs().into_owned().collect())
            .unwrap_or_default();
        if params.is_empty() {
            // Favicon and other requests: answer and keep waiting.
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            continue;
        }
        let ours = params.get("state").map(String::as_str) == Some(state);
        let ok = ours && params.contains_key("code");
        let (title, text) = if ok {
            (
                crate::i18n::t("oauth_page_ok_title"),
                crate::i18n::t("oauth_page_ok_text"),
            )
        } else {
            (
                crate::i18n::t("oauth_page_fail_title"),
                crate::i18n::t("oauth_page_fail_text"),
            )
        };
        let page = format!(
            "<!doctype html><meta charset=utf-8><title>Ferrowl</title>\
             <body style=\"font:16px system-ui;margin:15vh auto;max-width:28em;text-align:center\">\
             <h1 style=\"font-size:1.4em\">{title}</h1><p>{text}</p></body>"
        );
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        // Only an answer to this sign-in ends the wait: anything else (an
        // old tab, another request) gets the failure page and is ignored.
        if !ours {
            continue;
        }
        if let Some(err) = params.get("error") {
            let desc = params.get("error_description").cloned().unwrap_or_default();
            bail!("{err}: {desc}");
        }
        if !ok {
            bail!("sign-in response did not match this request");
        }
        // Browsers sometimes load the address again (a retry, a reload,
        // HTTPS-first): keep answering for a while so they do not show an
        // error page after a sign-in that worked.
        if let Ok(again) = listener.try_clone() {
            std::thread::spawn(move || serve_page(again, page, Duration::from_secs(30)));
        }
        return Ok(params["code"].clone());
    }
}

/// Answers every request on `listener` with `page` until `time` passes.
fn serve_page(listener: TcpListener, page: String, time: Duration) {
    let deadline = Instant::now() + time;
    while Instant::now() < deadline {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut line = String::new();
                let _ = BufReader::new(&stream).read_line(&mut line);
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
                    page.len()
                );
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn random_string(len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

fn pkce_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The address in an ID token (only displayed/used as account name: the
/// token comes straight from the provider over TLS, so it is not verified).
fn email_from_id_token(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    ["email", "preferred_username", "upn"].iter().find_map(|k| {
        claims
            .get(*k)?
            .as_str()
            .filter(|s| s.contains('@'))
            .map(str::to_lowercase)
    })
}

/// XOAUTH2 initial response (SASL), as IMAP and SMTP expect it.
pub fn xoauth2(email: &str, access_token: &str) -> String {
    format!("user={email}\x01auth=Bearer {access_token}\x01\x01")
}

/// IMAP `AUTHENTICATE XOAUTH2`.
pub struct ImapXoauth2 {
    pub response: String,
}

impl imap::Authenticator for ImapXoauth2 {
    type Response = String;
    fn process(&self, challenge: &[u8]) -> String {
        // First round: our credentials. If the server rejects them it sends a
        // JSON error as a second challenge; an empty answer ends the exchange.
        if challenge.is_empty() {
            self.response.clone()
        } else {
            String::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        // RFC 7636, appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn xoauth2_format() {
        assert_eq!(
            xoauth2("a@b.it", "tok"),
            "user=a@b.it\u{1}auth=Bearer tok\u{1}\u{1}"
        );
    }

    #[test]
    fn id_token_email() {
        let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"preferred_username":"Name@Outlook.com"}"#);
        assert_eq!(
            email_from_id_token(&format!("h.{payload}.s")).as_deref(),
            Some("name@outlook.com")
        );
    }

    #[test]
    fn providers_from_addresses() {
        assert_eq!(Provider::for_email("x@outlook.it"), Some(Provider::Microsoft));
        assert_eq!(Provider::for_email("x@hotmail.com"), Some(Provider::Microsoft));
        assert_eq!(Provider::for_email("x@gmail.com"), Some(Provider::Google));
        assert_eq!(Provider::for_email("x@example.it"), None);
        assert_eq!(
            Provider::Microsoft.servers("x@live.com").1.0,
            "smtp-mail.outlook.com"
        );
        assert_eq!(
            Provider::Microsoft.servers("x@company.com").1.0,
            "smtp.office365.com"
        );
    }

    #[test]
    fn code_from_app_redirect() {
        let pending = Pending {
            url: String::new(),
            state: "xyz".into(),
            verifier: String::new(),
            redirect: "msauth://com.ferrowl.app/abc%3D".into(),
        };
        assert_eq!(
            code_from_redirect(&pending, "msauth://com.ferrowl.app/abc%3D?code=M.C1&state=xyz").unwrap(),
            "M.C1"
        );
        assert!(
            code_from_redirect(&pending, "msauth://com.ferrowl.app/abc%3D?code=M.C1&state=other").is_err()
        );
        let err = code_from_redirect(
            &pending,
            "msauth://com.ferrowl.app/abc%3D?error=access_denied&error_description=cancelled&state=xyz",
        )
        .unwrap_err();
        assert!(err.to_string().contains("access_denied"));
    }

    #[test]
    fn redirect_is_received() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /?code=abc&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            let mut out = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut out);
        });
        assert_eq!(
            wait_for_code(&listener, "xyz", Duration::from_secs(5)).unwrap(),
            "abc"
        );
    }

    #[test]
    fn stray_connections_do_not_end_the_wait() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let send = move |bytes: &[u8]| {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(bytes).unwrap();
            let mut out = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut out);
        };
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            // Opened and closed without a request, not text, another state.
            drop(std::net::TcpStream::connect(("127.0.0.1", port)).unwrap());
            send(b"\xff\xfe\xfd\r\n");
            send(b"GET /?code=old&state=other HTTP/1.1\r\n\r\n");
            send(b"GET /?error=access_denied&state=other HTTP/1.1\r\n\r\n");
            send(b"GET /?code=abc&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n");
        });
        assert_eq!(
            wait_for_code(&listener, "xyz", Duration::from_secs(5)).unwrap(),
            "abc"
        );
    }

    #[test]
    fn repeated_redirect_still_gets_the_page() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let get = move || {
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /?code=abc&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n")
                .unwrap();
            let mut out = String::new();
            let _ = std::io::Read::read_to_string(&mut s, &mut out);
            out
        };
        let first = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            get()
        });
        assert_eq!(
            wait_for_code(&listener, "xyz", Duration::from_secs(5)).unwrap(),
            "abc"
        );
        assert!(first.join().unwrap().starts_with("HTTP/1.1 200"));
        drop(listener);
        // The browser loads the address again after the code was taken.
        assert!(get().starts_with("HTTP/1.1 200"));
    }
}
