use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use cp_base::panels::now_ms;

use crate::ring_buffer::RingBuffer;
use crate::types::ProcessStatus;

use super::manager::server_request;
use cp_base::cast::SafeCast;

/// File poller: reads new bytes from a log file into a ring buffer.
pub(crate) fn file_poller(path: PathBuf, buffer: RingBuffer, stop: Arc<AtomicBool>) {
    file_poller_from_offset(path, buffer, stop, 0);
}

#[expect(clippy::needless_pass_by_value)]
pub(crate) fn file_poller_from_offset(path: PathBuf, buffer: RingBuffer, stop: Arc<AtomicBool>, mut offset: u64) {
    use std::io::{Read, Seek, SeekFrom};

    loop {
        if stop.load(Ordering::Relaxed) {
            // Grace period: read any final bytes after process exit
            std::thread::sleep(std::time::Duration::from_millis(300));
            if let Ok(mut f) = fs::File::open(&path)
                && f.seek(SeekFrom::Start(offset)).is_ok()
            {
                let mut buf = vec![0u8; 64 * 1024];
                while let Ok(n) = f.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    buffer.write(&buf[..n]);
                }
            }
            break;
        }

        if let Ok(mut f) = fs::File::open(&path)
            && f.seek(SeekFrom::Start(offset)).is_ok()
        {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match f.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        buffer.write(&buf[..n]);
                        offset += n.to_u64();
                    }
                }
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Periodically poll the server for process status.
#[expect(clippy::needless_pass_by_value)]
pub(crate) fn poll_server_status(
    key: String,
    status: Arc<Mutex<ProcessStatus>>,
    finished_at: Arc<Mutex<Option<u64>>>,
    stop_polling: Arc<AtomicBool>,
) {
    loop {
        if stop_polling.load(Ordering::Relaxed) {
            break;
        }

        let req = serde_json::json!({"cmd": "status", "key": key});
        if let Ok(resp) = server_request(&req) {
            let st = resp.get("status").and_then(|v| v.as_str()).unwrap_or("");
            if st.starts_with("exited") {
                let code = resp.get("exit_code").and_then(serde_json::Value::as_i64).unwrap_or(-1).to_i32();
                let mut s = status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if !s.is_terminal() {
                    *s = if code == 0 { ProcessStatus::Finished(code) } else { ProcessStatus::Failed(code) };
                }
                let mut fin = finished_at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if fin.is_none() {
                    *fin = Some(now_ms());
                }
                stop_polling.store(true, Ordering::Relaxed);
                break;
            }
        } else {
            // Server unreachable — mark as dead
            let mut s = status.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if !s.is_terminal() {
                *s = ProcessStatus::Failed(-1);
            }
            let mut fin = finished_at.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if fin.is_none() {
                *fin = Some(now_ms());
            }
            stop_polling.store(true, Ordering::Relaxed);
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}
