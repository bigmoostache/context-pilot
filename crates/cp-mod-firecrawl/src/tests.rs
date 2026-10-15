//! Regression tests: retry policy, crawl deadline/cancel/pagination, location,
//! output-path checks. HTTP goes to a loopback mock server; no live Firecrawl call.

use std::io::{BufRead as _, BufReader, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::api::{FirecrawlClient, ScrapeParams, location_json};
use crate::crawl::{CrawlJob, Timing, build_markdown, check_output_path, run_crawl};
use crate::http::{Failure, Policy, retry_delay, should_retry};
use crate::types::CrawlPageData;

/// Scripted reply: status, extra header lines (CRLF-terminated), JSON body.
type Reply = (u16, &'static str, String);
/// Route handler: (API base, method, path, nth hit on this server) -> reply.
type Route = dyn Fn(&str, &str, &str, usize) -> Reply + Send + Sync;

/// Loopback HTTP server answering every request via a route, logging `METHOD path`.
struct Mock {
    /// API root to hand to the client.
    base: String,
    /// Requests received, in order.
    log: Arc<Mutex<Vec<String>>>,
}

impl Mock {
    /// Start serving `route` on an ephemeral port.
    fn start(route: Box<Route>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}/v2", listener.local_addr().expect("addr"));
        let log = Arc::new(Mutex::new(Vec::new()));
        let hits = AtomicUsize::new(0);
        let server = Server { route, base: base.clone(), log: Arc::clone(&log) };
        drop(std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let n = hits.fetch_add(1, Ordering::SeqCst);
                serve(stream, &server, n);
            }
        }));
        Self { base, log }
    }

    /// Client pointed at this server.
    fn client(&self) -> FirecrawlClient {
        FirecrawlClient::with_base_url("test-key".to_owned(), &self.base).expect("client")
    }

    /// Number of logged requests starting with `prefix`.
    fn count(&self, prefix: &str) -> usize {
        self.log.lock().expect("log").iter().filter(|h| h.starts_with(prefix)).count()
    }
}

/// Mock server-side state, owned by the accept thread.
struct Server {
    /// Route handler.
    route: Box<Route>,
    /// API root, passed to the route.
    base: String,
    /// Shared request log.
    log: Arc<Mutex<Vec<String>>>,
}

/// Answer one connection (`n` = nth hit on this server).
fn serve(stream: TcpStream, server: &Server, n: usize) {
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut first = String::new();
    if reader.read_line(&mut first).is_err() {
        return;
    }
    let mut len = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            len = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; len];
    drop(reader.read_exact(&mut body));
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("").to_owned();
    let path = parts.next().unwrap_or("").to_owned();
    server.log.lock().expect("log").push(format!("{method} {path}"));
    let (status, headers, json) = (server.route)(&server.base, &method, &path, n);
    let resp = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{json}",
        json.len()
    );
    let mut out = stream;
    drop(out.write_all(resp.as_bytes()));
}

/// Fast timing for crawl tests.
const fn timing(budget_ms: u64) -> Timing {
    Timing { interval: Duration::from_millis(20), budget: Duration::from_millis(budget_ms) }
}

/// Crawl job writing to a unique temp file.
fn job(tag: &str) -> CrawlJob {
    let output = std::env::temp_dir().join(format!("cp-firecrawl-test-{tag}-{}.md", std::process::id()));
    CrawlJob {
        url: "https://example.com".to_owned(),
        output,
        limit: 10,
        max_depth: None,
        include_paths: None,
        exclude_paths: None,
        allow_subdomains: false,
    }
}

/// Read and delete a crawl output file.
fn take_output(job: &CrawlJob) -> String {
    let md = std::fs::read_to_string(&job.output).expect("output written");
    drop(std::fs::remove_file(&job.output));
    md
}

/// `POST /v2/crawl` accepted as job `job1`.
fn started() -> Reply {
    (200, "", r#"{"success":true,"id":"job1"}"#.to_owned())
}

/// Not found.
fn missing() -> Reply {
    (404, "", "{}".to_owned())
}

#[test]
fn retry_policy_idempotent() {
    assert!(should_retry(Policy::Idempotent, Failure::Status(503)));
    assert!(should_retry(Policy::Idempotent, Failure::Transfer));
    assert!(!should_retry(Policy::Idempotent, Failure::Status(404)));
}

#[test]
fn retry_policy_billable() {
    assert!(!should_retry(Policy::Billable, Failure::Status(500)), "billed POST 5xx may have started a job");
    assert!(!should_retry(Policy::Billable, Failure::Transfer), "billed POST timeout may have started a job");
    assert!(should_retry(Policy::Billable, Failure::Connect));
    assert!(should_retry(Policy::Billable, Failure::Status(429)));
}

#[test]
fn retry_after_parsing() {
    assert_eq!(retry_delay(Some("3")), Duration::from_secs(3));
    assert_eq!(retry_delay(Some("3600")), Duration::from_secs(10), "capped");
    assert_eq!(retry_delay(Some("Wed, 21 Oct 2015 07:28:00 GMT")), Duration::from_secs(1));
    assert_eq!(retry_delay(None), Duration::from_secs(1));
}

#[test]
fn location_sent_with_country_alone() {
    assert_eq!(location_json(Some("DE"), None), Some(serde_json::json!({"country": "DE"})));
    assert_eq!(location_json(None, Some(&["fr"])), Some(serde_json::json!({"languages": ["fr"]})));
    assert_eq!(location_json(None, Some(&[])), None);
    assert_eq!(location_json(None, None), None);
}

#[test]
fn output_path_must_stay_in_project() {
    let roots = [PathBuf::from("/proj"), PathBuf::from("/proj/.worktrees/x")];
    assert_eq!(check_output_path(&PathBuf::from("/proj/docs/out.md"), &roots), Ok(()));
    drop(check_output_path(&PathBuf::from("/etc/passwd"), &roots).expect_err("outside project"));
    drop(check_output_path(&PathBuf::from("/proj/../etc/x.md"), &roots).expect_err("parent dir"));
    drop(check_output_path(&PathBuf::from("/projector/x.md"), &roots).expect_err("component-wise prefix"));
}

#[test]
fn markdown_flags_partial_results() {
    let pages: Vec<CrawlPageData> = serde_json::from_value(serde_json::json!([
        {"markdown": "hello", "metadata": {"title": "T", "sourceURL": "https://e.com/a"}}
    ]))
    .expect("pages");
    let md = build_markdown("https://e.com", &pages, 7, Some("INCOMPLETE: 1 pages of 3"));
    assert!(md.contains("1 pages crawled, 7 credits used"));
    assert!(md.contains("> WARNING: INCOMPLETE: 1 pages of 3"));
    assert!(md.contains("hello"));
}

#[test]
fn billed_post_not_retried_on_5xx() {
    let mock = Mock::start(Box::new(|_, _, _, _| (500, "", "{}".to_owned())));
    let out = run_crawl(mock.client(), &job("5xx"), timing(2000));
    assert!(out.is_error);
    assert_eq!(mock.count("POST /v2/crawl"), 1, "a retry could start a second billed job");
}

#[test]
fn billed_post_retried_on_429_with_retry_after() {
    let mock = Mock::start(Box::new(|_, _, _, n| {
        if n == 0 {
            (429, "Retry-After: 0\r\n", "{}".to_owned())
        } else {
            (200, "", r#"{"success":true,"data":{"markdown":"ok"}}"#.to_owned())
        }
    }));
    let params = ScrapeParams { url: "https://e.com", formats: vec!["markdown"], country: Some("DE"), languages: None };
    let resp = mock.client().scrape(&params).expect("scrape after 429");
    assert!(resp.success);
    assert_eq!(mock.count("POST /v2/scrape"), 2);
}

#[test]
fn idempotent_poll_retried_on_5xx() {
    let mock = Mock::start(Box::new(|_, method, _, n| match (method, n) {
        ("POST", _) => started(),
        ("GET", 1) => (503, "Retry-After: 0\r\n", "{}".to_owned()),
        _ => (200, "", r#"{"status":"completed","data":[{"markdown":"p1"}]}"#.to_owned()),
    }));
    let j = job("poll5xx");
    let out = run_crawl(mock.client(), &j, timing(5000));
    assert!(!out.is_error, "{}", out.content);
    assert_eq!(mock.count("GET /v2/crawl/job1"), 2);
    assert!(take_output(&j).contains("p1"));
}

#[test]
fn crawl_follows_next_cursor() {
    let mock = Mock::start(Box::new(|base, method, path, _| match (method, path) {
        ("POST", "/v2/crawl") => started(),
        ("GET", "/v2/crawl/job1") => (
            200,
            "",
            format!(
                r#"{{"status":"completed","total":2,"data":[{{"markdown":"p1"}}],"next":"{base}/crawl/job1?skip=1"}}"#
            ),
        ),
        ("GET", "/v2/crawl/job1?skip=1") => {
            (200, "", r#"{"status":"completed","data":[{"markdown":"p2"}]}"#.to_owned())
        }
        _ => missing(),
    }));
    let j = job("next");
    let out = run_crawl(mock.client(), &j, timing(5000));
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("2 pages"), "{}", out.content);
    let md = take_output(&j);
    assert!(md.contains("p1") && md.contains("p2"));
    assert!(!md.contains("WARNING"));
}

#[test]
fn crawl_pagination_failure_keeps_partial_with_warning() {
    let mock = Mock::start(Box::new(|base, method, path, _| match (method, path) {
        ("POST", "/v2/crawl") => started(),
        ("GET", "/v2/crawl/job1") => (
            200,
            "",
            format!(
                r#"{{"status":"completed","total":5,"data":[{{"markdown":"p1"}}],"next":"{base}/crawl/job1?skip=1"}}"#
            ),
        ),
        _ => (500, "Retry-After: 0\r\n", "{}".to_owned()),
    }));
    let j = job("partial");
    let out = run_crawl(mock.client(), &j, timing(5000));
    assert!(!out.is_error, "{}", out.content);
    assert!(out.content.contains("INCOMPLETE: 1 pages of 5"), "{}", out.content);
    assert!(take_output(&j).contains("> WARNING: INCOMPLETE"));
}

#[test]
fn crawl_timeout_cancels_remote_job() {
    let mock = Mock::start(Box::new(|_, method, _, _| match method {
        "POST" => started(),
        "DELETE" => (200, "", r#"{"status":"cancelled"}"#.to_owned()),
        _ => (200, "", r#"{"status":"scraping","completed":3}"#.to_owned()),
    }));
    let j = job("timeout");
    let out = run_crawl(mock.client(), &j, timing(300));
    assert!(out.is_error);
    assert!(out.content.contains("timed out") && out.content.contains("job1"), "{}", out.content);
    assert!(out.content.contains("cancelled on Firecrawl's side"), "{}", out.content);
    assert_eq!(mock.count("DELETE /v2/crawl/job1"), 1);
    assert!(!j.output.exists());
}

#[test]
fn foreign_next_url_refused_without_request() {
    let mock = Mock::start(Box::new(|_, _, _, _| missing()));
    let err = mock.client().crawl_page("https://evil.example/v2/crawl/job1?skip=1").expect_err("foreign host");
    assert!(err.contains("refusing"), "{err}");
    let prefix_trick = format!("{}evil/crawl", mock.base);
    drop(mock.client().crawl_page(&prefix_trick).expect_err("prefix trick"));
    assert_eq!(mock.count(""), 0);
}
