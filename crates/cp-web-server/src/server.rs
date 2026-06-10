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
use crate::projects;
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
    /// Projects root (`None` = projects API disabled).
    projects_root: Option<std::path::PathBuf>,
    /// `.env` file for the API-keys endpoints (`None` = disabled).
    env_file: Option<std::path::PathBuf>,
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
    /// Projects root (`--projects-dir`).
    pub projects_root: Option<std::path::PathBuf>,
    /// `.env` file (`--env-file`).
    pub env_file: Option<std::path::PathBuf>,
}

/// Run the axum server until the process exits.
pub(crate) async fn serve(args: ServeArgs) {
    let ServeArgs { listener, auth, dist_dir, frames, events, projects_root, env_file } = args;
    let shared =
        Arc::new(Shared { auth, frames, events, next_conn: AtomicU64::new(1), projects_root, env_file });

    let index = dist_dir.join("index.html");
    let spa = ServeDir::new(&dist_dir).fallback(ServeFile::new(index));

    let app = Router::new()
        .route("/api/login", post(login))
        .route("/api/devices", get(list_devices))
        .route("/api/devices/revoke", post(revoke_device))
        .route("/api/projects", get(projects_list).post(projects_create))
        .route("/api/projects/switch", post(projects_switch))
        .route("/api/projects/archive", post(projects_archive))
        .route("/api/projects/delete", post(projects_delete))
        .route("/api/projects/defaults", get(defaults_get).post(defaults_set))
        .route("/api/system/info", get(system_info))
        .route("/api/system/wifi", get(wifi_status))
        .route("/api/system/wifi/connect", post(wifi_connect))
        .route("/api/system/env", get(env_list).post(env_set))
        .route("/api/system/restart", post(system_restart))
        .route("/api/system/reboot", post(system_reboot))
        .route("/api/auth/password", post(change_password))
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

// ─── Projects API ───────────────────────────────────────────────────────────

/// Map a [`projects::ProjectError`] to an HTTP response.
fn project_error_response(err: projects::ProjectError) -> Response {
    use projects::ProjectError as PE;
    match err {
        PE::BadName => (StatusCode::BAD_REQUEST, "invalid project name").into_response(),
        PE::NotFound => (StatusCode::NOT_FOUND, "project not found").into_response(),
        PE::Exists => (StatusCode::CONFLICT, "project already exists").into_response(),
        PE::IsCurrent => (StatusCode::CONFLICT, "project is currently active — switch first").into_response(),
        PE::BadConfirm => (StatusCode::BAD_REQUEST, "confirmation does not match").into_response(),
        PE::Io(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
    }
}

/// Auth + projects-enabled guard shared by all project handlers.
fn projects_guard(shared: &Shared, headers: &HeaderMap) -> Result<std::path::PathBuf, Box<Response>> {
    if let Err(code) = check_bearer(shared, headers) {
        return Err(Box::new(code.into_response()));
    }
    shared.projects_root.clone().ok_or_else(|| {
        Box::new((StatusCode::NOT_IMPLEMENTED, "projects disabled (no --projects-dir)").into_response())
    })
}

/// `GET /api/projects` — registry listing + current project.
async fn projects_list(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let root = match projects_guard(&shared, &headers) {
        Ok(root) => root,
        Err(resp) => return *resp,
    };
    let current = projects::read_current(&root);
    axum::Json(serde_json::json!({ "projects": projects::list(&root), "current": current })).into_response()
}

/// `POST /api/projects` — create (and optionally clone) a project.
/// The clone runs on a blocking thread; the request returns when it's done.
async fn projects_create(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: axum::Json<projects::CreateRequest>,
) -> Response {
    let root = match projects_guard(&shared, &headers) {
        Ok(root) => root,
        Err(resp) => return *resp,
    };
    let projects::CreateRequest { name, git_url } = body.0;
    let result =
        tokio::task::spawn_blocking(move || projects::create(&root, &name, git_url.as_deref())).await;
    match result {
        Ok(Ok(())) => StatusCode::CREATED.into_response(),
        Ok(Err(err)) => project_error_response(err),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("join error: {e}")).into_response(),
    }
}

/// `POST /api/projects/switch` — hand over to the core (pointer + restart).
async fn projects_switch(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: axum::Json<projects::NameRequest>,
) -> Response {
    let root = match projects_guard(&shared, &headers) {
        Ok(root) => root,
        Err(resp) => return *resp,
    };
    if !projects::valid_name(&body.name) {
        return project_error_response(projects::ProjectError::BadName);
    }
    if !root.join(&body.name).is_dir() {
        return project_error_response(projects::ProjectError::NotFound);
    }
    if shared.events.send(WebEvent::SwitchProject { name: body.name.clone() }).is_err() {
        return (StatusCode::SERVICE_UNAVAILABLE, "core loop unavailable").into_response();
    }
    StatusCode::ACCEPTED.into_response()
}

/// `POST /api/projects/archive` — move a project to `.archive/`.
async fn projects_archive(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: axum::Json<projects::NameRequest>,
) -> Response {
    let root = match projects_guard(&shared, &headers) {
        Ok(root) => root,
        Err(resp) => return *resp,
    };
    match projects::archive(&root, &body.name) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => project_error_response(err),
    }
}

/// `POST /api/projects/delete` — permanent removal, typed confirmation.
async fn projects_delete(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: axum::Json<projects::DeleteRequest>,
) -> Response {
    let root = match projects_guard(&shared, &headers) {
        Ok(root) => root,
        Err(resp) => return *resp,
    };
    match projects::delete(&root, &body.name, &body.confirm) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => project_error_response(err),
    }
}

// ─── Paramètres généraux ────────────────────────────────────────────────────

/// `GET /api/system/info` — hostname, RAM, disque, température, uptime.
async fn system_info(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if let Err(code) = check_bearer(&shared, &headers) {
        return code.into_response();
    }
    axum::Json(crate::system::info(shared.projects_root.as_deref(), env!("CARGO_PKG_VERSION"))).into_response()
}

/// `GET /api/system/wifi` — réseau actuel + scan.
async fn wifi_status(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if let Err(code) = check_bearer(&shared, &headers) {
        return code.into_response();
    }
    match tokio::task::spawn_blocking(crate::system::wifi_status).await {
        Ok(status) => axum::Json(status).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("join error: {e}")).into_response(),
    }
}

/// Body of `POST /api/system/wifi/connect`.
#[derive(Debug, Deserialize)]
struct WifiConnectRequest {
    /// Target network SSID.
    ssid: String,
    /// WPA passphrase (omitted for open networks or known connections).
    #[serde(default)]
    password: Option<String>,
}

/// `POST /api/system/wifi/connect` — nmcli connect (peut couper l'accès !).
async fn wifi_connect(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: axum::Json<WifiConnectRequest>,
) -> Response {
    if let Err(code) = check_bearer(&shared, &headers) {
        return code.into_response();
    }
    let WifiConnectRequest { ssid, password } = body.0;
    let result =
        tokio::task::spawn_blocking(move || crate::system::wifi_connect(&ssid, password.as_deref())).await;
    match result {
        Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
        Ok(Err(msg)) => (StatusCode::BAD_GATEWAY, msg).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("join error: {e}")).into_response(),
    }
}

/// Auth + env-file-enabled guard.
fn env_guard(shared: &Shared, headers: &HeaderMap) -> Result<std::path::PathBuf, Box<Response>> {
    if let Err(code) = check_bearer(shared, headers) {
        return Err(Box::new(code.into_response()));
    }
    shared
        .env_file
        .clone()
        .ok_or_else(|| Box::new((StatusCode::NOT_IMPLEMENTED, "env file not configured (no --env-file)").into_response()))
}

/// `GET /api/system/env` — clés connues + présence, valeurs masquées.
async fn env_list(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let path = match env_guard(&shared, &headers) {
        Ok(path) => path,
        Err(resp) => return *resp,
    };
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    axum::Json(crate::system::env_list(&path, home.as_deref())).into_response()
}

/// Body of `POST /api/system/env` (`value: null` = suppression).
#[derive(Debug, Deserialize)]
struct EnvSetRequest {
    /// Env key name (`[A-Z][A-Z0-9_]*`).
    key: String,
    /// New value, or `null` to remove the key.
    #[serde(default)]
    value: Option<String>,
}

/// `POST /api/system/env` — upsert/suppression d'une clé (appliqué au restart).
async fn env_set(State(shared): State<Arc<Shared>>, headers: HeaderMap, body: axum::Json<EnvSetRequest>) -> Response {
    let path = match env_guard(&shared, &headers) {
        Ok(path) => path,
        Err(resp) => return *resp,
    };
    match crate::system::env_set(&path, &body.key, body.value.as_deref()) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(msg) => (StatusCode::BAD_REQUEST, msg).into_response(),
    }
}

/// `GET /api/projects/defaults` — défauts des nouveaux projets.
async fn defaults_get(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    let root = match projects_guard(&shared, &headers) {
        Ok(root) => root,
        Err(resp) => return *resp,
    };
    axum::Json(crate::system::defaults_read(&root)).into_response()
}

/// `POST /api/projects/defaults` — enregistre les défauts.
async fn defaults_set(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: axum::Json<crate::system::ProjectDefaults>,
) -> Response {
    let root = match projects_guard(&shared, &headers) {
        Ok(root) => root,
        Err(resp) => return *resp,
    };
    match crate::system::defaults_write(&root, &body.0) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg).into_response(),
    }
}

/// `POST /api/system/restart` — redémarre le service (relit le .env).
async fn system_restart(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if let Err(code) = check_bearer(&shared, &headers) {
        return code.into_response();
    }
    crate::system::restart_service();
    StatusCode::ACCEPTED.into_response()
}

/// `POST /api/system/reboot` — redémarre la Pi.
async fn system_reboot(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    if let Err(code) = check_bearer(&shared, &headers) {
        return code.into_response();
    }
    crate::system::reboot();
    StatusCode::ACCEPTED.into_response()
}

/// Body of `POST /api/auth/password`.
#[derive(Debug, Deserialize)]
struct ChangePasswordRequest {
    /// Current password (verified before any change).
    current: String,
    /// New password.
    new_password: String,
    /// Revoke every other device token.
    #[serde(default)]
    revoke_others: bool,
}

/// `POST /api/auth/password` — change le mot de passe web.
async fn change_password(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    body: axum::Json<ChangePasswordRequest>,
) -> Response {
    // Le token appelant sert de « survivant » si on révoque les autres.
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(ToString::to_string);
    let Some(token) = token else { return StatusCode::UNAUTHORIZED.into_response() };
    if shared.auth.verify_token(&token).is_err() {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if body.new_password.len() < 8 {
        return (StatusCode::BAD_REQUEST, "mot de passe trop court (8 min)").into_response();
    }
    let keep = body.revoke_others.then_some(token.as_str());
    match shared.auth.change_password(&body.current, &body.new_password, keep) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(StoreError::Denied) => (StatusCode::FORBIDDEN, "mot de passe actuel incorrect").into_response(),
        Err(StoreError::Throttled) => (StatusCode::TOO_MANY_REQUESTS, "retry in 1s").into_response(),
        Err(StoreError::Storage) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
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
