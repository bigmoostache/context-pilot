//! Context Pilot web transport (Nestor).
//!
//! Pure transport crate: an axum HTTP + WebSocket server that
//! - authenticates browsers (password → per-device revocable token),
//! - serves the SPA bundle,
//! - relays [`protocol::WebEvent`]s to the synchronous core loop, and
//! - fans pre-serialized [`protocol::WireFrame`]s out to connected clients.
//!
//! It knows nothing about `State`, `Action` or panels — the binary owns the
//! domain mapping (`build_web_state()`, command → `Action`). This keeps the
//! browser contract in one place and the transport reusable.
//!
//! Threading: the server runs a tokio runtime on its own thread; the core
//! stays synchronous. Bridges: `std::sync::mpsc` inbound, a tokio
//! `broadcast` channel outbound.

/// Password and token management.
pub mod auth;
/// Workspace registry (projects on disk).
pub mod projects;
/// Wire protocol types (commands, queries, frames).
pub mod protocol;
/// Axum routes and WebSocket plumbing.
mod server;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use protocol::{WebEvent, WireFrame};

/// Configuration for the web server.
#[derive(Debug, Clone)]
pub struct WebServerConfig {
    /// Explicit bind address. `0.0.0.0` is refused unless `allow_any_bind`.
    pub bind: SocketAddr,
    /// Opt-in escape hatch for binding to an unspecified address.
    pub allow_any_bind: bool,
    /// Directory containing the built SPA (served as static files).
    pub dist_dir: PathBuf,
    /// Path of the auth state file (e.g. `.context-pilot/web-auth.json`).
    pub auth_path: PathBuf,
    /// Password used to initialize the auth file on first start
    /// (from `CP_WEB_PASSWORD`). Ignored when the file already exists.
    pub initial_password: Option<String>,
    /// Projects root (`--projects-dir`). `None` disables the projects API.
    pub projects_root: Option<PathBuf>,
}

/// Handle returned by [`start`]: the outbound frame channel.
#[derive(Debug, Clone)]
pub struct WebHandle {
    /// Push frames here; every connected client receives broadcast frames,
    /// targeted frames reach only their connection.
    pub frames: tokio::sync::broadcast::Sender<WireFrame>,
}

impl WebHandle {
    /// Send a frame, ignoring "no receiver" errors (no client connected yet).
    pub fn send(&self, frame: WireFrame) {
        let _r = self.frames.send(frame);
    }
}

/// Errors that prevent the server from starting.
#[derive(Debug)]
pub enum StartError {
    /// Refused `0.0.0.0`/`::` without the explicit opt-in flag.
    UnspecifiedBind,
    /// No auth file and no `CP_WEB_PASSWORD` to create one.
    NoPassword,
    /// The TCP listener could not bind.
    Bind(std::io::Error),
    /// The tokio runtime could not be created.
    Runtime(std::io::Error),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnspecifiedBind => {
                write!(f, "refusing to bind 0.0.0.0 — pass an explicit LAN address (or --web-bind-any)")
            }
            Self::NoPassword => {
                write!(f, "no web password configured — set CP_WEB_PASSWORD for the first start")
            }
            Self::Bind(e) => write!(f, "could not bind web server: {e}"),
            Self::Runtime(e) => write!(f, "could not start tokio runtime: {e}"),
        }
    }
}

/// Outbound broadcast capacity (frames). Slow clients that lag past this
/// simply miss deltas and can reconnect for a fresh snapshot.
const BROADCAST_CAPACITY: usize = 1024;

/// Start the web server on a dedicated thread.
///
/// Returns the outbound frame handle. Inbound events (commands, queries,
/// connections) arrive on `events_tx`.
///
/// # Errors
///
/// See [`StartError`] — bad bind address, missing password, bind failure.
pub fn start(config: &WebServerConfig, events_tx: Sender<WebEvent>) -> Result<WebHandle, StartError> {
    if config.bind.ip().is_unspecified() && !config.allow_any_bind {
        return Err(StartError::UnspecifiedBind);
    }

    let auth = auth::Store::open(config.auth_path.clone(), config.initial_password.as_deref())
        .ok_or(StartError::NoPassword)?;

    // Bind synchronously so startup errors surface before the thread detaches.
    let listener = std::net::TcpListener::bind(config.bind).map_err(StartError::Bind)?;
    listener.set_nonblocking(true).map_err(StartError::Bind)?;

    let (frames_tx, _frames_rx) = tokio::sync::broadcast::channel::<WireFrame>(BROADCAST_CAPACITY);
    let handle = WebHandle { frames: frames_tx.clone() };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(StartError::Runtime)?;

    let args = server::ServeArgs {
        listener,
        auth,
        dist_dir: config.dist_dir.clone(),
        frames: frames_tx,
        events: events_tx,
        projects_root: config.projects_root.clone(),
    };
    drop(std::thread::Builder::new().name("cp-web-server".to_string()).spawn(move || {
        runtime.block_on(server::serve(args));
    }));

    log::info!("[web] serving on http://{}", config.bind);
    Ok(handle)
}
