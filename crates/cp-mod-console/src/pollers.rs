use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cp_base::cast::Safe as _;
use cp_base::panels::now_ms;

use super::manager::server_request;
use crate::ring_buffer::RingBuffer;
use crate::types::ProcessStatus;

/// Tails a log file, pushing new bytes into a shared ring buffer.
pub(crate) struct FilePoller {
    /// Path to the log file being tailed.
    pub path: PathBuf,
    /// Shared ring buffer that receives new bytes.
    pub buffer: RingBuffer,
    /// Signal flag to stop the polling loop.
    pub stop: Arc<AtomicBool>,
    /// Current byte offset in the log file.
    pub offset: u64,
}

impl FilePoller {
    /// Consume self and poll until `stop` is set. Designed for `thread::spawn`.
    pub(crate) fn run(mut self) {
        use std::io::{Read as _, Seek as _, SeekFrom};

        loop {
            if self.stop.load(Ordering::Relaxed) {
                // Grace period: read any final bytes after process exit
                std::thread::sleep(std::time::Duration::from_millis(300));
                if let Ok(mut f) = fs::File::open(&self.path)
                    && f.seek(SeekFrom::Start(self.offset)).is_ok()
                {
                    let mut buf = vec![0u8; 64 * 1024];
                    while let Ok(n) = f.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        self.buffer.write(buf.get(..n).unwrap_or_default());
                    }
                }
                break;
            }

            if let Ok(mut f) = fs::File::open(&self.path)
                && f.seek(SeekFrom::Start(self.offset)).is_ok()
            {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match f.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            self.buffer.write(buf.get(..n).unwrap_or_default());
                            self.offset = self.offset.saturating_add(n.to_u64());
                        }
                    }
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// Periodically asks the console server for process status updates.
pub(crate) struct StatusPoller {
    /// Server-side session key used to query status.
    pub key: String,
    /// Shared process status, updated when the process exits.
    pub status: Arc<Mutex<ProcessStatus>>,
    /// Timestamp (ms) when the process finished, if it has.
    pub finished_at: Arc<Mutex<Option<u64>>>,
    /// Signal flag to stop the polling loop.
    pub stop: Arc<AtomicBool>,
}

impl StatusPoller {
    /// Consume self and poll until the process exits or the server becomes unreachable.
    pub(crate) fn run(self) {
        loop {
            if self.stop.load(Ordering::Relaxed) {
                break;
            }

            let req = serde_json::json!({"cmd": "status", "key": self.key});
            if let Ok(resp) = server_request(&req) {
                let st = resp.get("status").and_then(|v| v.as_str()).unwrap_or("");
                if st.starts_with("exited") {
                    let code = resp.get("exit_code").and_then(serde_json::Value::as_i64).unwrap_or(-1).to_i32();
                    {
                        let mut s = self.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        if !s.is_terminal() {
                            *s = if code == 0 { ProcessStatus::Finished(code) } else { ProcessStatus::Failed(code) };
                        }
                    }
                    {
                        let mut fin = self.finished_at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        if fin.is_none() {
                            *fin = Some(now_ms());
                        }
                    }
                    self.stop.store(true, Ordering::Relaxed);
                    break;
                }
            } else {
                // Server unreachable — mark as dead
                {
                    let mut s = self.status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if !s.is_terminal() {
                        *s = ProcessStatus::Failed(-1);
                    }
                }
                {
                    let mut fin = self.finished_at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    if fin.is_none() {
                        *fin = Some(now_ms());
                    }
                }
                self.stop.store(true, Ordering::Relaxed);
                break;
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    }
}
