//! HTTP server: pages and assets from `web/`, the phone WebSocket at `/ws`, admin routes.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use rust_embed::Embed;
use serde::Deserialize;
use tokio::time::timeout;

use crate::admin::{self, AdminAuth};
use crate::protocol::{ClientMsg, PROTOCOL_VERSION, RejectReason, ServerMsg};
use crate::session::{Control, JoinError, Joined, Sessions};

/// Clients ping every 500 ms. Silence for this long means the phone is gone.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Embed)]
#[folder = "web/"]
struct Assets;

pub struct AppState {
    pub sessions: Arc<Sessions>,
    pub auth: AdminAuth,
    /// Players must send this to join, if set.
    pub join_code: Option<String>,
    /// Link to the controller page, with the join code filled in.
    pub join_url: String,
}

impl AppState {
    fn code_ok(&self, code: Option<&str>) -> bool {
        match &self.join_code {
            None => true,
            Some(expected) => code.is_some_and(|c| admin::ct_eq(c.as_bytes(), expected.as_bytes())),
        }
    }
}

pub fn router(app: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(|| asset(Path("index.html".to_owned()))))
        .route("/pad", get(|| asset(Path("pad.html".to_owned()))))
        .route("/admin", get(|| asset(Path("admin.html".to_owned()))))
        .route("/ws", get(ws_upgrade))
        .route("/api/info", get(admin::info))
        .route("/api/login", post(admin::login))
        .route("/api/logout", post(admin::logout))
        .route("/admin/ws", get(admin::ws_upgrade))
        .route("/admin/qr.svg", get(admin::qr_svg))
        .route("/{*path}", get(asset))
        .with_state(app)
}

async fn asset(Path(path): Path<String>) -> Response {
    let Some(file) = Assets::get(&path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype().to_owned()),
            // Phones cache aggressively; always revalidate so a rebuilt server is picked up.
            (header::CACHE_CONTROL, "no-cache".to_owned()),
        ],
        file.data,
    )
        .into_response()
}

#[derive(Deserialize)]
struct WsQuery {
    code: Option<String>,
}

async fn ws_upgrade(ws: WebSocketUpgrade, Query(query): Query<WsQuery>, State(app): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_socket(socket, &app, query.code).await {
            tracing::debug!("connection ended: {e:#}");
        }
    })
}

async fn send(socket: &mut WebSocket, msg: ServerMsg) -> anyhow::Result<()> {
    socket.send(Message::Binary(msg.encode().into())).await?;
    Ok(())
}

/// Next binary frame, or `None` when the connection closed or went silent.
async fn recv(socket: &mut WebSocket) -> Option<anyhow::Result<ClientMsg>> {
    loop {
        match timeout(HEARTBEAT_TIMEOUT, socket.recv()).await {
            Ok(Some(Ok(Message::Binary(data)))) => return Some(ClientMsg::decode(&data)),
            Ok(Some(Ok(Message::Close(_)) | Err(_)) | None) => return None,
            Ok(Some(Ok(_))) => continue, // text and WS-level ping/pong frames
            Err(_) => {
                tracing::debug!("heartbeat timeout");
                return None;
            }
        }
    }
}

async fn handle_socket(mut socket: WebSocket, app: &AppState, code: Option<String>) -> anyhow::Result<()> {
    let sessions = &app.sessions;
    let Some(Ok(ClientMsg::Hello { version, token })) = recv(&mut socket).await else {
        anyhow::bail!("expected hello");
    };
    let reject = |reason| ServerMsg::Reject(reason);
    if version != PROTOCOL_VERSION {
        send(&mut socket, reject(RejectReason::BadVersion)).await?;
        anyhow::bail!("client speaks protocol v{version}, server v{PROTOCOL_VERSION}");
    }
    if !app.code_ok(code.as_deref()) {
        send(&mut socket, reject(RejectReason::BadCode)).await?;
        anyhow::bail!("wrong join code");
    }
    let Joined { lease, slot, mut control } = match sessions.join(token) {
        Ok(joined) => joined,
        Err(JoinError::Full) => {
            send(&mut socket, reject(RejectReason::Full)).await?;
            anyhow::bail!("server full");
        }
        Err(JoinError::Locked) => {
            send(&mut socket, reject(RejectReason::Locked)).await?;
            anyhow::bail!("joining is locked");
        }
        Err(JoinError::Backend(e)) => {
            tracing::error!("{} backend could not create a pad: {e:#}", sessions.backend_name());
            send(&mut socket, reject(RejectReason::BackendError)).await?;
            return Err(e);
        }
    };

    let result = async {
        send(&mut socket, ServerMsg::Welcome { slot, token: lease.token }).await?;
        loop {
            tokio::select! {
                msg = recv(&mut socket) => match msg {
                    None => break,
                    Some(Ok(ClientMsg::State(state))) => sessions.update(&lease, &state),
                    Some(Ok(ClientMsg::Ping(t))) => send(&mut socket, ServerMsg::Pong(t)).await?,
                    Some(Ok(ClientMsg::Name(name))) => sessions.set_name(&lease, name),
                    Some(Ok(ClientMsg::Hello { .. })) => anyhow::bail!("duplicate hello"),
                    Some(Err(e)) => tracing::warn!("bad frame from a phone: {e:#}"),
                },
                ctl = control.recv() => match ctl {
                    Some(Control::Send(msg)) => send(&mut socket, msg).await?,
                    Some(Control::Close(msg)) => {
                        send(&mut socket, msg).await?;
                        break;
                    }
                    None => break, // a newer connection took over this slot
                },
            }
        }
        Ok(())
    }
    .await;
    sessions.leave(&lease);
    result
}
