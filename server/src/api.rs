//! HTTP API used by the app (JSON, behind an HTTPS reverse proxy).
//!
//! | Method | Path                  | Body                    | Effect |
//! |--------|-----------------------|-------------------------|--------|
//! | POST   | /v1/devices           | push token (below)      | registers the phone → `{device_id, device_token}` |
//! | PUT    | /v1/device            | push token (below)      | new push token |
//! | DELETE | /v1/device            |                         | forgets the phone and all its accounts |
//! | POST   | /v1/accounts          | `{key, account}`        | stores the account (sealed) and starts watching → `{account_id}` |
//! | DELETE | /v1/accounts/{id}     |                         | stops watching and deletes it |
//! | POST   | /v1/unlock            | `{key}`                 | after a server restart: resumes watching |
//! | GET    | /healthz              |                         | `ok` |
//!
//! Every call except registration needs `Authorization: Bearer <device_token>`.
//!
//! Push token: `{fcm_token}` (Android), or `{apns_token, apns_sandbox}`
//! (iPhone: the device token in hex; `apns_sandbox` true for development
//! builds).

use crate::fcm::{Notifier, Platform, Target};
use crate::seal::{DeviceKey, random_token};
use crate::store::{Device, SealedAccount, Store};
use crate::watch::Watchers;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use ferrowl_core::config::Account;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Accounts one phone may register.
const MAX_ACCOUNTS: usize = 10;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub watchers: Arc<Watchers>,
    pub notifier: Arc<dyn Notifier>,
    /// Device keys received since the server started (memory only).
    pub keys: Arc<Mutex<HashMap<String, DeviceKey>>>,
    /// Checks an account's login before storing it (replaced in tests).
    pub check_login: fn(&Account) -> Result<(), String>,
}

pub fn real_login_check(account: &Account) -> Result<(), String> {
    let mut s = crate::addr::connect(account).map_err(|e| login_check_error(account, &e))?;
    let _ = s.logout();
    Ok(())
}

/// What the phone is told when the check fails. Only a refused login says
/// more than "cannot connect": the answer comes from a real mail server
/// (TLS checked its name), while connection errors would tell what runs
/// at any address and port.
fn login_check_error(account: &Account, e: &anyhow::Error) -> String {
    if let Some(login) = e.downcast_ref::<ferrowl_core::mail::LoginError>() {
        login.to_string()
    } else if e.downcast_ref::<crate::addr::NotAllowed>().is_some() {
        format!("{}: {e}", ferrowl_core::mail::connect_error(account))
    } else {
        ferrowl_core::mail::connect_error(account)
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/v1/devices", post(register))
        .route("/v1/device", put(update_token).delete(delete_device))
        .route("/v1/accounts", post(add_account))
        .route("/v1/accounts/{id}", delete(delete_account))
        .route("/v1/unlock", post(unlock))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .with_state(state)
}

// ---------------------------------------------------------------- errors

pub struct ApiError(StatusCode, String);

impl ApiError {
    fn new(code: StatusCode, msg: impl Into<String>) -> Self {
        ApiError(code, msg.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        eprintln!("internal error: {e:#}");
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn device(state: &AppState, headers: &HeaderMap) -> ApiResult<Device> {
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "missing device token"))?;
    state
        .store
        .device_by_token(token)?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "unknown device"))
}

fn key_of(text: &str) -> ApiResult<DeviceKey> {
    DeviceKey::from_base64(text).map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))
}

/// The push address in a registration body.
fn target_of(body: &PushTokenBody) -> ApiResult<Target> {
    let target = match (&body.fcm_token, &body.apns_token) {
        (Some(t), None) => Target {
            platform: Platform::Fcm,
            token: t.clone(),
        },
        (None, Some(t)) => {
            // APNs device tokens are hex (32 bytes today, longer allowed).
            if !t.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid apns_token"));
            }
            Target {
                platform: if body.apns_sandbox {
                    Platform::ApnsSandbox
                } else {
                    Platform::Apns
                },
                token: t.to_ascii_lowercase(),
            }
        }
        _ => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "send fcm_token or apns_token",
            ));
        }
    };
    if target.token.is_empty() || target.token.len() > 4096 {
        return Err(ApiError::new(StatusCode::BAD_REQUEST, "invalid push token"));
    }
    Ok(target)
}

// ---------------------------------------------------------------- handlers

#[derive(Deserialize)]
struct PushTokenBody {
    #[serde(default)]
    fcm_token: Option<String>,
    #[serde(default)]
    apns_token: Option<String>,
    #[serde(default)]
    apns_sandbox: bool,
}

#[derive(Serialize)]
struct Registered {
    device_id: String,
    device_token: String,
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<PushTokenBody>,
) -> ApiResult<Json<Registered>> {
    let target = target_of(&body)?;
    let device_id = random_token();
    let device_token = random_token();
    state.store.add_device(&device_id, &device_token, &target)?;
    Ok(Json(Registered {
        device_id,
        device_token,
    }))
}

async fn update_token(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<PushTokenBody>,
) -> ApiResult<StatusCode> {
    let d = device(&state, &headers)?;
    let target = target_of(&body)?;
    state.store.set_target(&d.id, &target)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_device(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<StatusCode> {
    let d = device(&state, &headers)?;
    for acc in state.store.accounts(&d.id)? {
        state.watchers.stop(&acc.id);
    }
    state.store.delete_device(&d.id)?;
    lock(&state.keys).remove(&d.id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct AddAccount {
    key: String,
    account: Account,
}

#[derive(Serialize)]
struct Added {
    account_id: String,
}

async fn add_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AddAccount>,
) -> ApiResult<(StatusCode, Json<Added>)> {
    let d = device(&state, &headers)?;
    let key = key_of(&body.key)?;
    let account = body.account;
    if account.is_pop3() {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "POP3 accounts cannot be watched (no IDLE)",
        ));
    }
    let existing = state.store.accounts(&d.id)?;
    if existing.len() >= MAX_ACCOUNTS {
        return Err(too_many());
    }
    // All accounts of a device share its key.
    if let Some(first) = existing.first()
        && key.open(&first.nonce, &first.sealed, &first.id).is_err()
    {
        return Err(ApiError::new(StatusCode::UNAUTHORIZED, "wrong device key"));
    }

    // The account must log in before it is stored.
    let check = state.check_login;
    let probe = account.clone();
    tokio::task::spawn_blocking(move || check(&probe))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .map_err(|msg| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, msg))?;

    let account_id = random_token();
    let plain = zeroize::Zeroizing::new(serde_json::to_vec(&account).map_err(anyhow::Error::from)?);
    let (nonce, sealed) = key.seal(&plain, &account_id)?;
    // Counted again with the insert: other requests may have added
    // accounts during the login check.
    let added = state.store.add_account_within(
        &d.id,
        &SealedAccount {
            id: account_id.clone(),
            nonce: nonce.to_vec(),
            sealed,
        },
        MAX_ACCOUNTS,
    )?;
    if !added {
        return Err(too_many());
    }
    lock(&state.keys).insert(d.id.clone(), key);
    state.watchers.start(
        account_id.clone(),
        d.id,
        account,
        state.store.clone(),
        state.notifier.clone(),
    );
    Ok((StatusCode::CREATED, Json(Added { account_id })))
}

fn too_many() -> ApiError {
    ApiError::new(StatusCode::CONFLICT, "too many accounts for this device")
}

async fn delete_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let d = device(&state, &headers)?;
    if !state.store.delete_account(&d.id, &id)? {
        return Err(ApiError::new(StatusCode::NOT_FOUND, "no such account"));
    }
    state.watchers.stop(&id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Unlock {
    key: String,
}

#[derive(Serialize)]
struct Unlocked {
    watching: usize,
}

async fn unlock(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Unlock>,
) -> ApiResult<Json<Unlocked>> {
    let d = device(&state, &headers)?;
    let key = key_of(&body.key)?;
    let accounts = state.store.accounts(&d.id)?;
    let mut opened = Vec::new();
    for acc in &accounts {
        let plain = key
            .open(&acc.nonce, &acc.sealed, &acc.id)
            .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "wrong device key"))?;
        let account: Account = serde_json::from_slice(&plain).map_err(anyhow::Error::from)?;
        opened.push((acc.id.clone(), account));
    }
    lock(&state.keys).insert(d.id.clone(), key);
    for (id, account) in opened {
        if !state.watchers.is_running(&id) {
            state.watchers.start(
                id,
                d.id.clone(),
                account,
                state.store.clone(),
                state.notifier.clone(),
            );
        }
    }
    Ok(Json(Unlocked {
        watching: accounts.len(),
    }))
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcm::LogNotifier;
    use axum::body::Body;
    use axum::http::Request;
    use base64::Engine as _;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn state() -> AppState {
        AppState {
            store: Arc::new(Store::in_memory().unwrap()),
            watchers: Arc::new(Watchers::default()),
            notifier: Arc::new(LogNotifier),
            keys: Arc::default(),
            // Refuses every login: no network in tests, and no watcher starts.
            check_login: |_| Err("login refused".into()),
        }
    }

    async fn call(
        app: &Router,
        method: &str,
        uri: &str,
        token: Option<&str>,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let res = app
            .clone()
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    fn account_json() -> serde_json::Value {
        serde_json::to_value(Account {
            email: "ada@example.org".into(),
            username: "ada".into(),
            password: "pw".into(),
            // Closed local port: a watcher fails at once, without network.
            imap_host: "127.0.0.1".into(),
            imap_port: 1,
            smtp_host: "127.0.0.1".into(),
            ..Default::default()
        })
        .unwrap()
    }

    #[tokio::test]
    async fn device_lifecycle_and_auth() {
        let st = state();
        let app = router(st.clone());

        let (code, reg) = call(
            &app,
            "POST",
            "/v1/devices",
            None,
            serde_json::json!({"fcm_token": "fcm-1"}),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        let token = reg["device_token"].as_str().unwrap().to_owned();

        // No or wrong token: refused.
        let (code, _) = call(
            &app,
            "PUT",
            "/v1/device",
            None,
            serde_json::json!({"fcm_token": "x"}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);
        let (code, _) = call(
            &app,
            "PUT",
            "/v1/device",
            Some("nope"),
            serde_json::json!({"fcm_token": "x"}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);

        let (code, _) = call(
            &app,
            "PUT",
            "/v1/device",
            Some(&token),
            serde_json::json!({"fcm_token": "fcm-2"}),
        )
        .await;
        assert_eq!(code, StatusCode::NO_CONTENT);

        // An account that cannot log in is not stored.
        let key = base64::engine::general_purpose::STANDARD.encode([9u8; 32]);
        let (code, err) = call(
            &app,
            "POST",
            "/v1/accounts",
            Some(&token),
            serde_json::json!({"key": key, "account": account_json()}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(err["error"], "login refused");

        // POP3 refused before any login attempt.
        let mut pop = account_json();
        pop["protocol"] = "pop3".into();
        let (code, _) = call(
            &app,
            "POST",
            "/v1/accounts",
            Some(&token),
            serde_json::json!({"key": key, "account": pop}),
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY);

        let (code, _) = call(
            &app,
            "DELETE",
            "/v1/device",
            Some(&token),
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(code, StatusCode::NO_CONTENT);
        let (code, _) = call(
            &app,
            "POST",
            "/v1/unlock",
            Some(&token),
            serde_json::json!({"key": key}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED, "deleted device is gone");
    }

    #[test]
    fn login_check_errors_tell_only_refused_logins() {
        ferrowl_core::i18n::set(Some("en"));
        let account: Account = serde_json::from_value(account_json()).unwrap();
        let generic = ferrowl_core::mail::connect_error(&account);
        let refused = anyhow::Error::new(ferrowl_core::mail::LoginError("Login failed: NO".into()));
        assert_eq!(login_check_error(&account, &refused), "Login failed: NO");
        let closed = anyhow::anyhow!("Connection refused (os error 61)").context(generic.clone());
        assert_eq!(login_check_error(&account, &closed), generic);
        let local = anyhow::Error::new(crate::addr::NotAllowed);
        assert!(login_check_error(&account, &local).ends_with("mail server address not allowed"));
    }

    #[tokio::test]
    async fn iphones_register_with_an_apns_token() {
        let st = state();
        let app = router(st.clone());
        let (code, reg) = call(
            &app,
            "POST",
            "/v1/devices",
            None,
            serde_json::json!({"apns_token": "AB12cd", "apns_sandbox": true}),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        let id = reg["device_id"].as_str().unwrap();
        assert_eq!(
            st.store.target(id).unwrap(),
            Some(Target {
                platform: Platform::ApnsSandbox,
                token: "ab12cd".into()
            })
        );
        for bad in [
            serde_json::json!({"apns_token": "not hex"}),
            serde_json::json!({}),
            serde_json::json!({"fcm_token": "f", "apns_token": "ab"}),
        ] {
            let (code, _) = call(&app, "POST", "/v1/devices", None, bad).await;
            assert_eq!(code, StatusCode::BAD_REQUEST);
        }
    }

    #[tokio::test]
    async fn unlock_checks_the_key() {
        let st = state();
        let app = router(st.clone());
        let (_, reg) = call(
            &app,
            "POST",
            "/v1/devices",
            None,
            serde_json::json!({"fcm_token": "f"}),
        )
        .await;
        let token = reg["device_token"].as_str().unwrap().to_owned();
        let device = reg["device_id"].as_str().unwrap().to_owned();

        // Store a sealed account directly (as add_account would after a login).
        let key =
            DeviceKey::from_base64(&base64::engine::general_purpose::STANDARD.encode([1u8; 32])).unwrap();
        let plain = serde_json::to_vec(&account_json()).unwrap();
        let (nonce, sealed) = key.seal(&plain, "acc").unwrap();
        st.store
            .add_account(
                &device,
                &SealedAccount {
                    id: "acc".into(),
                    nonce: nonce.to_vec(),
                    sealed,
                },
            )
            .unwrap();

        let wrong = base64::engine::general_purpose::STANDARD.encode([2u8; 32]);
        let (code, _) = call(
            &app,
            "POST",
            "/v1/unlock",
            Some(&token),
            serde_json::json!({"key": wrong}),
        )
        .await;
        assert_eq!(code, StatusCode::UNAUTHORIZED);

        // Right key: accepted (the watcher thread then fails to connect to
        // the closed port and waits to retry; stop it right away).
        let right = base64::engine::general_purpose::STANDARD.encode([1u8; 32]);
        let (code, body) = call(
            &app,
            "POST",
            "/v1/unlock",
            Some(&token),
            serde_json::json!({"key": right}),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(body["watching"], 1);
        st.watchers.stop("acc");

        let (code, _) = call(
            &app,
            "DELETE",
            "/v1/accounts/acc",
            Some(&token),
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(code, StatusCode::NO_CONTENT);
        let (code, _) = call(
            &app,
            "DELETE",
            "/v1/accounts/acc",
            Some(&token),
            serde_json::Value::Null,
        )
        .await;
        assert_eq!(code, StatusCode::NOT_FOUND);
    }
}
