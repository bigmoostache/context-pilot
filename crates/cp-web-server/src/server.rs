//! Axum routes: login, device management, WebSocket relay, SPA static files.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse as _, Response};
use axum::routing::{any, get, post};
use futures_util::sink::SinkExt as _;
use futures_util::stream::StreamExt as _;
use serde::Deserialize;
use tower_http::services::{ServeDir, ServeFile};

use crate::auth::{Store, StoreError};
use crate::protocol::{ClientMsg, LoginRequest, LoginResponse, WebEvent, WireFrame, error_frame};

/// Shared state for all axum handlers.
struct Shared {
    /// Password/token store.
    auth: Store,
    /// Outbound frames (subscribed per WS connection).
    frames: tokio::sync::broadcast::Sender<WireFrame>,
    /// Inbound events to the core loop.
    events: Sender<WebEvent>,
    /// Monotonic connection ID allocator.
    next_conn: AtomicU64,
}

/// Everything [`serve`] needs, bundled to stay under the argument limit.
pub(crate) struct ServeArgs {
    /// Pre-bound (non-blocking) TCP listener.
    pub listener: std::net::TcpListener,
    /// Password/token store.
    pub auth: Store,
    /// Directory of the built SPA.
    pub dist_dir: std::path::PathBuf,
    /// Outbound frame channel.
    pub frames: tokio::sync::broadcast::Sender<WireFrame>,
    /// Inbound event channel to the core loop.
    pub events: Sender<WebEvent>,
}

/// Run the axum server until the process exits.
pub(crate) async fn serve(args: ServeArgs) {
    let ServeArgs { listener, auth, dist_dir, frames, events } = args;
    let shared = Arc::new(Shared { auth, frames, events, next_conn: AtomicU64::new(1) });

    let index = dist_dir.join("index.html");
    let spa = ServeDir::new(&dist_dir).fallback(ServeFile::new(index));

    let app = Router::new()
        .route("/api/login", post(login))
        .route("/api/devices", get(list_devices))
        .route("/api/devices/revoke", post(revoke_device))
        .route("/ws", any(ws_upgrade))
        .fallback_service(spa)
        .with_state(shared);

    let Ok(tokio_listener) = tokio::net::TcpListener::from_std(listener) else {
        log::error!("[web] failed to adopt listener into tokio");
        return;
    };
    if let Err(e) = axum::serve(tokio_listener, app).await {
        log::error!("[web] server stopped: {e}");
    }
}

/// `POST /api/login` — verify password, mint a device token.
async fn login(State(shared): State<Arc<Shared>>, body: axum::Json<LoginRequest>) -> Response {
    match shared.auth.login(&body.password, &body.device_name) {
        Ok((token, device_id)) => axum::Json(LoginResponse { token, device_id }).into_response(),
        Err(StoreError::Throttled) => (StatusCode::TOO_MANY_REQUESTS, "retry in 1s").into_response(),
        Err(StoreError::Denied) => (StatusCode::UNAUTHORIZED, "wrong password").into_response(),
        Err(StoreError::Storage) => (StatusCode::INTERNAL_SERVER_ERROR, "auth storage error").into_response(),
    }
}

/// Extract and verify the Bearer token from request headers.
fn check_bearer(shared: &Shared, headers: &HeaderMap) -> Result<(), StatusCode> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    shared.auth.verify_token(token).map_err(|_e| StatusCode::UNAUTHORIZED)
}

/// `GET /api/devices` — list authenticated devices.
async fn list_devices(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if let Err(code) = check_bearer(&shared, &headers) {
        return code.into_response();
    }
    axum::Json(shared.auth.devices()).into_response()
}

/// Body of `POST /api/devices/revoke`.
#[derive(Debug, Deserialize)]
struct RevokeRequest {
    /// Device entry to revoke.
    device_id: String,
}

/// `POST /api/devices/revoke` — revoke a device token.
async fn revoke_device(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: axum::Json<RevokeRequest>,
) -> Response {
    if let Err(code) = check_bearer(&shared, &headers) {
        return code.into_response();
    }
    match shared.auth.revoke(&body.device_id) {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(_e) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Query parameters of `GET /ws`.
#[derive(Debug, Deserialize)]
struct WsParams {
    /// Session token minted by `/api/login`.
    token: String,
}

/// `GET /ws?token=…` — authenticate then upgrade to WebSocket.
async fn ws_upgrade(
    State(shared): State<Arc<Shared>>,
    Query(params): Query<WsParams>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if shared.auth.verify_token(&params.token).is_err() {
        return (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    }
    upgrade.on_upgrade(move |socket| handle_socket(shared, socket))
}

/// Per-connection WebSocket plumbing.
///
/// Outbound: a spawned task forwards broadcast frames (filtered to this
/// connection) into the socket. Inbound: this task parses client messages
/// and relays them to the core. Replies born on the inbound path (pong,
/// parse errors) travel through the same broadcast channel as targeted
/// frames — one egress path, no `select!`.
async fn handle_socket(shared: Arc<Shared>, socket: WebSocket) {
    let conn_id = shared.next_conn.fetch_add(1, Ordering::Relaxed);
    let (ws_tx, mut ws_rx) = socket.split();

    let forward = tokio::spawn(forward_frames(shared.frames.subscribe(), ws_tx, conn_id));

    // Ask the core for a snapshot addressed to this connection.
    if shared.events.send(WebEvent::Connected { conn_id }).is_err() {
        shared.send_to(conn_id, error_frame("core loop unavailable"));
        forward.abort();
        return;
    }
    log::info!("[web] client #{conn_id} connected");

    while let Some(Ok(msg)) = ws_rx.next().await {
        if !handle_client_msg(&shared, conn_id, msg) {
            break;
        }
    }

    forward.abort();
    log::info!("[web] client #{conn_id} disconnected");
}

/// Forward broadcast frames into one client's socket until it goes away.
async fn forward_frames(
    mut frames_rx: tokio::sync::broadcast::Receiver<WireFrame>,
    mut ws_tx: futures_util::stream::SplitSink<WebSocket, Message>,
    conn_id: u64,
) {
    loop {
        match frames_rx.recv().await {
            Ok(frame) => {
                let mine = frame.to.is_none() || frame.to == Some(conn_id);
                if mine && ws_tx.send(Message::text(frame.json)).await.is_err() {
                    break; // client gone
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                log::warn!("[web] client #{conn_id} lagged, skipped {skipped} frames");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

impl Shared {
    /// Push a frame addressed to a single connection.
    fn send_to(&self, conn_id: u64, json: String) {
        let _r = self.frames.send(WireFrame::to_conn(conn_id, json));
    }
}

/// Handle one inbound WS message. Returns `false` when the core is gone.
fn handle_client_msg(shared: &Shared, conn_id: u64, msg: Message) -> bool {
    let Message::Text(text) = msg else {
        return true; // ignore binary/ping/pong/close payloads (axum answers pings)
    };
    let event = match serde_json::from_str::<ClientMsg>(text.as_str()) {
        Ok(ClientMsg::Cmd(cmd)) => WebEvent::Command(cmd),
        Ok(ClientMsg::Query { req_id, query }) => WebEvent::Query { conn_id, req_id, query },
        Ok(ClientMsg::Ping) => {
            shared.send_to(conn_id, r#"{"t":"pong"}"#.to_string());
            return true;
        }
        Err(e) => {
            shared.send_to(conn_id, error_frame(&format!("bad message: {e}")));
            return true;
        }
    };
    shared.events.send(event).is_ok()
}
