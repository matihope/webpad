//! HTTP server: embedded controller page at `/`, WebSocket at `/ws`.

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use rust_embed::Embed;
use tokio::time::timeout;

use crate::protocol::{ClientMsg, PROTOCOL_VERSION, RejectReason, ServerMsg};
use crate::session::{JoinError, Sessions};

/// Clients ping every 500 ms. Silence for this long means the phone is gone.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Embed)]
#[folder = "web/"]
struct Assets;

pub fn router(sessions: Arc<Sessions>) -> Router {
    Router::new()
        .route("/", get(|| asset(Path("index.html".to_owned()))))
        .route("/ws", get(ws_upgrade))
        .route("/{*path}", get(asset))
        .with_state(sessions)
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

async fn ws_upgrade(ws: WebSocketUpgrade, State(sessions): State<Arc<Sessions>>) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_socket(socket, &sessions).await {
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

async fn handle_socket(mut socket: WebSocket, sessions: &Sessions) -> anyhow::Result<()> {
    let Some(Ok(ClientMsg::Hello { version, token })) = recv(&mut socket).await else {
        anyhow::bail!("expected hello");
    };
    if version != PROTOCOL_VERSION {
        send(&mut socket, ServerMsg::Reject(RejectReason::BadVersion)).await?;
        anyhow::bail!("client speaks protocol v{version}, server v{PROTOCOL_VERSION}");
    }
    let lease = match sessions.join(token) {
        Ok(lease) => lease,
        Err(JoinError::Full) => {
            send(&mut socket, ServerMsg::Reject(RejectReason::Full)).await?;
            anyhow::bail!("server full");
        }
        Err(JoinError::Backend(e)) => {
            tracing::error!("{} backend could not create a pad: {e:#}", sessions.backend_name());
            send(&mut socket, ServerMsg::Reject(RejectReason::BackendError)).await?;
            return Err(e);
        }
    };

    let result = async {
        send(&mut socket, ServerMsg::Welcome { slot: lease.slot, token: lease.token }).await?;
        while let Some(msg) = recv(&mut socket).await {
            match msg {
                Ok(ClientMsg::State(state)) => sessions.update(&lease, &state),
                Ok(ClientMsg::Ping(t)) => send(&mut socket, ServerMsg::Pong(t)).await?,
                Ok(ClientMsg::Hello { .. }) => anyhow::bail!("duplicate hello"),
                Err(e) => tracing::warn!("P{}: bad frame: {e:#}", lease.slot + 1),
            }
        }
        Ok(())
    }
    .await;
    sessions.leave(&lease);
    result
}
