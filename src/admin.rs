//! Admin side: optional password login, the admin WebSocket (live snapshots + actions),
//! and the join QR code. Auth model: docs/decisions/0004-auth-model.md.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use qrcode::QrCode;
use qrcode::render::svg;
use serde::Deserialize;
use serde_json::json;
use tokio::time::MissedTickBehavior;

use crate::server::AppState;

const COOKIE: &str = "webpad_admin";
/// Admin views get at most this many snapshots per second (enough for the live input preview).
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(66);

pub struct AdminAuth {
    password: Option<String>,
    /// Session tokens handed out by a successful login. Kept in memory: a restart logs everyone out.
    tokens: Mutex<HashSet<String>>,
}

impl AdminAuth {
    pub fn new(password: Option<String>) -> Self {
        Self { password, tokens: Mutex::default() }
    }

    pub fn required(&self) -> bool {
        self.password.is_some()
    }

    /// Without a password, everyone is admin.
    pub fn is_admin(&self, headers: &HeaderMap) -> bool {
        self.password.is_none()
            || cookie(headers, COOKIE).is_some_and(|t| self.tokens.lock().unwrap().contains(t))
    }

    fn login(&self, password: &str) -> Option<String> {
        let expected = self.password.as_deref()?;
        if !ct_eq(password.as_bytes(), expected.as_bytes()) {
            return None;
        }
        let token = hex(&rand::random::<[u8; 16]>());
        self.tokens.lock().unwrap().insert(token.clone());
        Some(token)
    }

    fn logout(&self, headers: &HeaderMap) {
        if let Some(t) = cookie(headers, COOKIE) {
            self.tokens.lock().unwrap().remove(t);
        }
    }
}

/// Constant-time comparison, so response timing doesn't reveal how much of a secret matched.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

/// Reject cross-site WebSocket requests: browsers attach cookies to them, and no CORS check applies.
fn same_origin(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true; // not a browser
    };
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or_default();
    origin.split_once("://").is_some_and(|(_, rest)| rest == host)
}

pub async fn info(State(app): State<Arc<AppState>>, headers: HeaderMap) -> Json<serde_json::Value> {
    Json(json!({
        "codeRequired": app.join_code.is_some(),
        "adminPasswordRequired": app.auth.required(),
        "admin": app.auth.is_admin(&headers),
    }))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    password: String,
}

pub async fn login(State(app): State<Arc<AppState>>, Json(req): Json<LoginRequest>) -> Response {
    if !app.auth.required() {
        return StatusCode::NO_CONTENT.into_response();
    }
    match app.auth.login(&req.password) {
        Some(token) => (
            StatusCode::NO_CONTENT,
            [(header::SET_COOKIE, format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/"))],
        )
            .into_response(),
        None => {
            tracing::warn!("failed admin login");
            // Slow down password guessing.
            tokio::time::sleep(Duration::from_secs(1)).await;
            StatusCode::UNAUTHORIZED.into_response()
        }
    }
}

pub async fn logout(State(app): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    app.auth.logout(&headers);
    (
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, format!("{COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0"))],
    )
        .into_response()
}

pub async fn qr_svg(State(app): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !app.auth.is_admin(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Ok(code) = QrCode::new(&app.join_url) else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(256, 256)
        .dark_color(svg::Color("#000"))
        .light_color(svg::Color("#fff"))
        .build();
    ([(header::CONTENT_TYPE, "image/svg+xml"), (header::CACHE_CONTROL, "no-cache")], svg).into_response()
}

pub async fn ws_upgrade(ws: WebSocketUpgrade, State(app): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if !same_origin(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if !app.auth.is_admin(&headers) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_socket(socket, &app).await {
            tracing::debug!("admin connection ended: {e:#}");
        }
    })
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum Action {
    Swap { a: usize, b: usize },
    Kick { slot: usize },
    Unplug { slot: usize },
    UnplugStale,
    Identify { slot: usize },
    Lock { locked: bool },
}

fn snapshot_json(app: &AppState) -> String {
    let snap = app.sessions.snapshot();
    json!({
        "type": "state",
        "slots": snap.slots,
        "locked": snap.locked,
        "joinUrl": app.join_url,
        "joinCode": app.join_code,
        "maxPlayers": app.sessions.max_players(),
        "backend": app.sessions.backend_name(),
    })
    .to_string()
}

fn apply(app: &AppState, action: Action) -> anyhow::Result<()> {
    let sessions = &app.sessions;
    match action {
        Action::Swap { a, b } => sessions.swap(a, b)?,
        Action::Kick { slot } => sessions.kick(slot),
        Action::Unplug { slot } => sessions.unplug(slot),
        Action::UnplugStale => (0..sessions.max_players()).for_each(|i| sessions.unplug(i)),
        Action::Identify { slot } => sessions.identify(slot),
        Action::Lock { locked } => sessions.set_locked(locked),
    }
    Ok(())
}

async fn handle_socket(mut socket: WebSocket, app: &AppState) -> anyhow::Result<()> {
    let mut version = app.sessions.subscribe();
    let mut throttle = tokio::time::interval(SNAPSHOT_INTERVAL);
    throttle.set_missed_tick_behavior(MissedTickBehavior::Delay);
    // Grace countdowns change without a session event, so refresh at least once a second.
    let mut refresh = tokio::time::interval(Duration::from_secs(1));
    let mut dirty = true;
    loop {
        tokio::select! {
            _ = throttle.tick(), if dirty => {
                socket.send(Message::Text(snapshot_json(app).into())).await?;
                dirty = false;
            }
            changed = version.changed(), if !dirty => {
                changed?;
                dirty = true;
            }
            _ = refresh.tick() => dirty = true,
            msg = socket.recv() => match msg {
                Some(Ok(Message::Text(text))) => {
                    let result = serde_json::from_str(&text).map_err(anyhow::Error::from).and_then(|a| apply(app, a));
                    if let Err(e) = result {
                        let msg = json!({ "type": "error", "message": format!("{e:#}") }).to_string();
                        socket.send(Message::Text(msg.into())).await?;
                    }
                }
                Some(Ok(Message::Close(_)) | Err(_)) | None => return Ok(()),
                Some(Ok(_)) => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn constant_time_compare() {
        assert!(ct_eq(b"hunter2", b"hunter2"));
        assert!(!ct_eq(b"hunter2", b"hunter3"));
        assert!(!ct_eq(b"hunter", b"hunter2"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn parses_cookies() {
        let mut h = HeaderMap::new();
        h.append(header::COOKIE, HeaderValue::from_static("a=1; webpad_admin=abc ;b=2"));
        assert_eq!(cookie(&h, "webpad_admin"), Some("abc"));
        assert_eq!(cookie(&h, "b"), Some("2"));
        assert_eq!(cookie(&h, "missing"), None);
    }

    #[test]
    fn login_flow() {
        let auth = AdminAuth::new(Some("pw".into()));
        let mut h = HeaderMap::new();
        assert!(!auth.is_admin(&h));
        assert!(auth.login("wrong").is_none());
        let token = auth.login("pw").unwrap();
        h.insert(header::COOKIE, format!("{COOKIE}={token}").parse().unwrap());
        assert!(auth.is_admin(&h));
        auth.logout(&h);
        assert!(!auth.is_admin(&h));
        assert!(AdminAuth::new(None).is_admin(&HeaderMap::new()), "no password: open");
    }

    #[test]
    fn origin_check() {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, HeaderValue::from_static("192.168.0.5:8080"));
        assert!(same_origin(&h));
        h.insert(header::ORIGIN, HeaderValue::from_static("http://192.168.0.5:8080"));
        assert!(same_origin(&h));
        h.insert(header::ORIGIN, HeaderValue::from_static("http://evil.example"));
        assert!(!same_origin(&h));
    }
}
