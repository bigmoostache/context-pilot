//! Password → per-device revocable token authentication.
//!
//! The web token unlocks full control of the Pi, so the design is deliberately
//! conservative: the password is stored as an argon2id hash, tokens are
//! 256-bit random values stored hashed (SHA-256), and every device entry is
//! individually revocable. State lives in `.context-pilot/web-auth.json`.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher as _, PasswordVerifier as _, SaltString};
use rand_core::OsRng;
use rand::RngCore as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

/// Minimum delay between two password attempts (anti brute-force).
const LOGIN_THROTTLE: Duration = Duration::from_secs(1);

/// One authenticated device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    /// Stable identifier used for revocation.
    pub id: String,
    /// Human-readable name supplied at login.
    pub name: String,
    /// SHA-256 of the session token, hex-encoded.
    pub token_sha256: String,
    /// Creation timestamp (ms since UNIX epoch).
    pub created_ms: u64,
    /// Last successful WS authentication (ms since UNIX epoch).
    pub last_seen_ms: u64,
}

/// Device summary exposed by `GET /api/devices` (no token material).
#[derive(Debug, Clone, Serialize)]
pub struct DeviceInfo {
    /// Stable identifier used for revocation.
    pub id: String,
    /// Human-readable name supplied at login.
    pub name: String,
    /// Creation timestamp (ms since UNIX epoch).
    pub created_ms: u64,
    /// Last successful WS authentication (ms since UNIX epoch).
    pub last_seen_ms: u64,
}

/// On-disk auth state.
#[derive(Debug, Default, Serialize, Deserialize)]
struct AuthFile {
    /// Argon2id PHC string of the web password.
    password_hash: String,
    /// Authenticated devices.
    #[serde(default)]
    devices: Vec<Device>,
}

/// Authentication store: password verification + token lifecycle.
pub struct Store {
    /// Persisted state, guarded for concurrent axum handlers.
    inner: Mutex<AuthFile>,
    /// Path of the JSON file.
    path: PathBuf,
    /// Last password attempt, for throttling.
    last_attempt: Mutex<Option<Instant>>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Opaque on purpose: never leak hashes or token digests into logs.
        f.debug_struct("Store").field("path", &self.path).finish_non_exhaustive()
    }
}

/// Errors surfaced by the auth store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// Wrong password or unknown/revoked token.
    Denied,
    /// A password attempt arrived during the throttle window.
    Throttled,
    /// The auth file could not be read or written.
    Storage,
}

/// Current time in ms since the UNIX epoch.
fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Hex-encoded SHA-256 of a token string.
fn token_digest(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

impl Store {
    /// Load (or initialize) the auth store.
    ///
    /// If the file does not exist, `initial_password` (from `CP_WEB_PASSWORD`)
    /// is hashed and stored. Returns `None` when no file exists and no
    /// password was provided — the server must refuse to start in that case.
    #[must_use]
    pub fn open(path: PathBuf, initial_password: Option<&str>) -> Option<Self> {
        let file = if path.exists() {
            let raw = std::fs::read_to_string(&path).ok()?;
            serde_json::from_str::<AuthFile>(&raw).ok()?
        } else {
            let password = initial_password?;
            let salt = SaltString::generate(&mut OsRng);
            let hash = Argon2::default().hash_password(password.as_bytes(), &salt).ok()?;
            let file = AuthFile { password_hash: hash.to_string(), devices: Vec::new() };
            Self::write_file(&path, &file)?;
            file
        };
        Some(Self { inner: Mutex::new(file), path, last_attempt: Mutex::new(None) })
    }

    /// Atomically persist the auth file (write + rename).
    fn write_file(path: &PathBuf, file: &AuthFile) -> Option<()> {
        let json = serde_json::to_string_pretty(file).ok()?;
        if let Some(parent) = path.parent() {
            let _r = std::fs::create_dir_all(parent);
        }
        let tmp_path = path.with_extension("json.new");
        std::fs::write(&tmp_path, json).ok()?;
        std::fs::rename(&tmp_path, path).ok()
    }

    /// Persist the current in-memory state.
    fn persist(&self, file: &AuthFile) -> Result<(), StoreError> {
        Self::write_file(&self.path, file).ok_or(StoreError::Storage)
    }

    /// Verify the password and mint a new per-device token.
    ///
    /// # Errors
    ///
    /// [`StoreError::Throttled`] within 1 s of the previous attempt,
    /// [`StoreError::Denied`] on a wrong password, [`StoreError::Storage`] on I/O failure.
    pub fn login(&self, password: &str, device_name: &str) -> Result<(String, String), StoreError> {
        // Throttle: at most one password verification per second.
        {
            let Ok(mut last) = self.last_attempt.lock() else { return Err(StoreError::Storage) };
            if let Some(prev) = *last
                && prev.elapsed() < LOGIN_THROTTLE
            {
                return Err(StoreError::Throttled);
            }
            *last = Some(Instant::now());
        }

        let Ok(mut inner) = self.inner.lock() else { return Err(StoreError::Storage) };
        let parsed = PasswordHash::new(&inner.password_hash).map_err(|_e| StoreError::Storage)?;
        Argon2::default().verify_password(password.as_bytes(), &parsed).map_err(|_e| StoreError::Denied)?;

        // Mint a 256-bit token; only its hash is stored.
        let mut bytes = [0_u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);
        let mut id_bytes = [0_u8; 8];
        rand::rng().fill_bytes(&mut id_bytes);
        let device_id = hex::encode(id_bytes);

        let name = if device_name.is_empty() { "unnamed device".to_string() } else { device_name.to_string() };
        inner.devices.push(Device {
            id: device_id.clone(),
            name,
            token_sha256: token_digest(&token),
            created_ms: now_ms(),
            last_seen_ms: now_ms(),
        });
        self.persist(&inner)?;
        Ok((token, device_id))
    }

    /// Check a session token; updates `last_seen_ms` on success.
    ///
    /// # Errors
    ///
    /// [`StoreError::Denied`] for unknown/revoked tokens, [`StoreError::Storage`] on I/O failure.
    pub fn verify_token(&self, token: &str) -> Result<(), StoreError> {
        let digest = token_digest(token);
        let Ok(mut inner) = self.inner.lock() else { return Err(StoreError::Storage) };
        let Some(device) = inner.devices.iter_mut().find(|d| d.token_sha256 == digest) else {
            return Err(StoreError::Denied);
        };
        device.last_seen_ms = now_ms();
        self.persist(&inner)
    }

    /// List devices (without token material).
    #[must_use]
    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.inner.lock().map_or_else(
            |_e| Vec::new(),
            |inner| {
                inner
                    .devices
                    .iter()
                    .map(|d| DeviceInfo {
                        id: d.id.clone(),
                        name: d.name.clone(),
                        created_ms: d.created_ms,
                        last_seen_ms: d.last_seen_ms,
                    })
                    .collect()
            },
        )
    }

    /// Change the web password after verifying the current one.
    /// With `keep_only_token`, every other device token is revoked.
    ///
    /// # Errors
    ///
    /// [`StoreError::Throttled`], [`StoreError::Denied`] (wrong current
    /// password), [`StoreError::Storage`].
    pub fn change_password(
        &self,
        current: &str,
        new_password: &str,
        keep_only_token: Option<&str>,
    ) -> Result<(), StoreError> {
        // Même throttle que le login : la vérification du mdp actuel
        // est une surface de brute-force identique.
        {
            let Ok(mut last) = self.last_attempt.lock() else { return Err(StoreError::Storage) };
            if let Some(prev) = *last
                && prev.elapsed() < LOGIN_THROTTLE
            {
                return Err(StoreError::Throttled);
            }
            *last = Some(Instant::now());
        }
        let Ok(mut inner) = self.inner.lock() else { return Err(StoreError::Storage) };
        let parsed = PasswordHash::new(&inner.password_hash).map_err(|_e| StoreError::Storage)?;
        Argon2::default().verify_password(current.as_bytes(), &parsed).map_err(|_e| StoreError::Denied)?;

        let salt = SaltString::generate(&mut OsRng);
        let hash =
            Argon2::default().hash_password(new_password.as_bytes(), &salt).map_err(|_e| StoreError::Storage)?;
        inner.password_hash = hash.to_string();
        if let Some(token) = keep_only_token {
            let keep = token_digest(token);
            inner.devices.retain(|d| d.token_sha256 == keep);
        }
        self.persist(&inner)
    }

    /// Revoke a device's token by device ID. Returns `true` if found.
    ///
    /// # Errors
    ///
    /// [`StoreError::Storage`] on I/O failure.
    pub fn revoke(&self, device_id: &str) -> Result<bool, StoreError> {
        let Ok(mut inner) = self.inner.lock() else { return Err(StoreError::Storage) };
        let before = inner.devices.len();
        inner.devices.retain(|d| d.id != device_id);
        let removed = inner.devices.len() < before;
        if removed {
            self.persist(&inner)?;
        }
        Ok(removed)
    }
}
