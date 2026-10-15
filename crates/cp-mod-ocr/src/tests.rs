//! Unit + mocked-HTTP tests. No call ever reaches the paid Datalab API: the
//! client is pointed at a scripted local server.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

use crate::cache;
use crate::client::{
    Attempt, DatalabClient, OcrMode, Poll, Timing, WATCHER_TIMEOUT, WORKER_BUDGET, classify_status, is_ocr_extension,
    parse_poll_body, parse_submit_response,
};
use crate::tools::{JobSlot, MAX_CONCURRENT_JOBS};

/// One scripted reply; `None` drops the connection (network error).
type Reply = Option<(u16, String)>;

/// Scripted HTTP server: answers requests in order, repeating the last reply.
struct Mock {
    /// `http://127.0.0.1:<port>`.
    url: String,
    /// Requests served so far.
    hits: Arc<AtomicUsize>,
}

impl Mock {
    fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        let script = Mutex::new(replies);
        let _h = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut conn) = stream else { return };
                drain_request(&mut conn);
                let n = counter.fetch_add(1, Ordering::SeqCst);
                let reply = {
                    let s = script.lock().unwrap();
                    s.get(n).or_else(|| s.last()).cloned().flatten()
                };
                if let Some((status, body)) = reply {
                    let head = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _r = conn.write_all(head.as_bytes()).and_then(|()| conn.write_all(body.as_bytes()));
                }
            }
        });
        Self { url, hits }
    }

    fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

/// Consume one request (headers + `Content-Length` or chunked body).
fn drain_request(stream: &mut std::net::TcpStream) {
    let mut reader = BufReader::new(stream);
    let mut len = 0usize;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(v) = lower.strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
        chunked |= lower.starts_with("transfer-encoding:") && lower.contains("chunked");
    }
    if chunked {
        let mut tail = Vec::new();
        while !tail.ends_with(b"0\r\n\r\n") {
            let mut b = [0u8; 1];
            if reader.read(&mut b).unwrap_or(0) == 0 {
                break;
            }
            tail.push(b[0]);
        }
    } else {
        let mut body = vec![0u8; len];
        let _r = reader.read_exact(&mut body);
    }
}

fn ok(body: &serde_json::Value) -> (u16, String) {
    (200, body.to_string())
}

fn retryable(status: u16) -> bool {
    matches!(classify_status(status, String::new()), Attempt::Retry(_))
}

fn fatal<T>(r: &Result<T, Attempt>) -> bool {
    matches!(r, Err(Attempt::Fatal(_)))
}

/// Fresh scratch dir under the system temp dir.
fn scratch(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("cp-ocr-test-{}-{tag}-{n}", std::process::id()));
    let _r = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fast(budget_ms: u64) -> Timing {
    Timing {
        budget: Duration::from_millis(budget_ms),
        request: Duration::from_secs(2),
        poll_initial: Duration::from_millis(5),
        poll_max: Duration::from_millis(20),
        submit_initial: Duration::from_millis(5),
    }
}

/// Client on `mock` + a document with unique content.
fn setup(mock: &Mock, tag: &str, budget_ms: u64) -> (DatalabClient, PathBuf, PathBuf) {
    let dir = scratch(tag);
    let doc = dir.join("scan.PDF");
    std::fs::write(&doc, format!("%PDF fake {tag} {:?}", Instant::now())).unwrap();
    let cache_dir = dir.join("cache");
    let client = DatalabClient::with_endpoint("k", &mock.url, cache_dir.clone(), fast(budget_ms)).unwrap();
    (client, doc, cache_dir)
}

#[test]
fn extensions_are_case_insensitive() {
    assert!(is_ocr_extension("PDF"));
    assert!(is_ocr_extension("Jpg"));
    assert!(is_ocr_extension("tif"));
    assert!(!is_ocr_extension("docx"));
}

#[test]
fn status_classification() {
    assert!(retryable(429));
    assert!(retryable(503));
    assert!(!retryable(400));
    assert!(!retryable(404));
}

#[test]
fn submit_response_parsing() {
    assert_eq!(parse_submit_response(200, &json!({"request_id": "abc"})), Ok("abc".to_owned()));
    let via_url = json!({"request_check_url": "https://x/api/v1/convert/r42"});
    assert_eq!(parse_submit_response(202, &via_url), Ok("r42".to_owned()));
    assert!(fatal(&parse_submit_response(200, &json!({}))));
    assert!(matches!(parse_submit_response(502, &serde_json::Value::Null), Err(Attempt::Retry(_))));
}

#[test]
fn poll_body_parsing() {
    let md = json!({"status": "complete", "markdown": "# hi"});
    assert_eq!(parse_poll_body(&md, OcrMode::Markdown), Ok(Poll::Done("# hi".to_owned())));
    let boxes = json!({"status": "complete", "json": {"blocks": []}});
    assert_eq!(parse_poll_body(&boxes, OcrMode::TextBoxes), Ok(Poll::Done(r#"{"blocks":[]}"#.to_owned())));
    let missing = json!({"status": "complete"});
    assert!(fatal(&parse_poll_body(&missing, OcrMode::Markdown)));
    let pending = json!({"status": "processing"});
    assert_eq!(parse_poll_body(&pending, OcrMode::Markdown), Ok(Poll::Pending("processing".to_owned())));
    assert!(fatal(&parse_poll_body(&json!({"status": "failed"}), OcrMode::Markdown)));
}

#[test]
fn transient_errors_are_retried_then_cached() {
    let mock = Mock::start(vec![
        Some((503, "{}".to_owned())),
        Some(ok(&json!({"request_id": "r1"}))),
        Some((500, "oops".to_owned())),
        None,
        Some((429, "{}".to_owned())),
        Some(ok(&json!({"status": "processing"}))),
        Some(ok(&json!({"status": "complete", "markdown": "text \u{e9}"}))),
    ]);
    let (client, doc, cache_dir) = setup(&mock, "retry", 10_000);
    let first = client.convert(&doc, OcrMode::Markdown).unwrap();
    assert_eq!(first.text, "text \u{e9}");
    assert!(!first.cached);
    assert_eq!(mock.hits(), 7);

    let again = client.convert(&doc, OcrMode::Markdown).unwrap();
    assert!(again.cached);
    assert_eq!(mock.hits(), 7, "cache hit must not touch the API");
    assert_eq!(std::fs::read_dir(&cache_dir).unwrap().count(), 1, "no temp file left behind");
}

#[test]
fn client_error_on_submit_is_not_retried() {
    let mock = Mock::start(vec![Some((400, r#"{"detail":"bad file"}"#.to_owned()))]);
    let (client, doc, _cache) = setup(&mock, "fatal", 10_000);
    let e = client.convert(&doc, OcrMode::Markdown).unwrap_err();
    assert!(e.contains("bad file"), "{e}");
    assert_eq!(mock.hits(), 1);
}

#[test]
fn missing_result_field_is_an_error_and_not_cached() {
    let mock = Mock::start(vec![Some(ok(&json!({"request_id": "r2"}))), Some(ok(&json!({"status": "complete"})))]);
    let (client, doc, cache_dir) = setup(&mock, "nofield", 10_000);
    let e = client.convert(&doc, OcrMode::Markdown).unwrap_err();
    assert!(e.contains("r2"), "error names the request: {e}");
    assert!(!cache_dir.exists() || std::fs::read_dir(&cache_dir).unwrap().count() == 0);
}

#[test]
fn job_never_outlives_its_budget() {
    let mock = Mock::start(vec![Some(ok(&json!({"request_id": "r3"}))), Some(ok(&json!({"status": "processing"})))]);
    let (client, doc, _cache) = setup(&mock, "budget", 300);
    let start = Instant::now();
    let e = client.convert(&doc, OcrMode::Markdown).unwrap_err();
    assert!(start.elapsed() < Duration::from_secs(2), "took {:?}", start.elapsed());
    assert!(e.contains("timed out") && e.contains("r3"), "{e}");
}

#[test]
fn watcher_outlasts_worker_budget() {
    assert!(WATCHER_TIMEOUT >= WORKER_BUDGET + Duration::from_secs(30));
}

#[test]
fn empty_cache_entry_is_a_miss() {
    let dir = scratch("empty");
    std::fs::write(dir.join("k.txt"), "").unwrap();
    assert_eq!(cache::read(&dir, "k"), None);
    cache::write(&dir, "k", "x");
    assert_eq!(cache::read(&dir, "k").as_deref(), Some("x"));
}

#[test]
fn job_slots_are_capped_and_released() {
    let slots: Vec<JobSlot> = std::iter::from_fn(JobSlot::acquire).take(MAX_CONCURRENT_JOBS + 1).collect();
    assert_eq!(slots.len(), MAX_CONCURRENT_JOBS);
    assert!(JobSlot::acquire().is_none());
    drop(slots);
    assert!(JobSlot::acquire().is_some());
}
