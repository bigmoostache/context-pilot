//! Monitor unit tests: parameter validation, engine batching/flood/spill, ring-buffer cursor.

use serde_json::json;

use super::engine::{
    BATCH_WINDOW_MS, Budget, Engine, FLOOD_MAX_EVENTS, FLOOD_WINDOW_MS, Filter, INLINE_MAX_BYTES, render_payload,
};
use super::spec::{self, Frequency, Mode, Source};
use crate::ring_buffer::{Matcher, RingBuffer};

/// Keep every non-blank line.
const LINE: Filter<'static> = Filter { mode: Mode::Line, pattern: None };

/// Parse error text, panicking on success.
fn err(input: &serde_json::Value) -> String {
    match spec::parse(input) {
        Ok(r) => panic!("expected error, got {r:?}"),
        Err(e) => e,
    }
}

#[test]
fn parse_console_line_ongoing() {
    let r = spec::parse(&json!({"type":"console","id":"P3","description":"d","mode":"line","frequency":"ongoing"}))
        .expect("valid");
    assert_eq!(r.source, Source::Console);
    assert_eq!(r.mode, Mode::Line);
    assert_eq!(r.frequency, Frequency::Ongoing);
}

#[test]
fn exit_ongoing_rejected() {
    let e = err(&json!({"type":"console","id":"P3","description":"d","mode":"exit","frequency":"ongoing"}));
    assert!(e.contains("frequency='once'"), "{e}");
}

#[test]
fn ws_requires_line() {
    let e = err(
        &json!({"type":"ws","ws_url":"ws://x","description":"d","mode":"pattern","pattern":"a","frequency":"once"}),
    );
    assert!(e.contains("mode='line'"), "{e}");
}

#[test]
fn pattern_required_and_validated() {
    let base = json!({"type":"console","id":"P3","description":"d","mode":"pattern","frequency":"once"});
    assert!(err(&base).contains("requires a 'pattern'"));
    let mut bad = base;
    let _prev = bad.as_object_mut().expect("object").insert("pattern".to_owned(), json!("(unclosed"));
    assert!(err(&bad).contains("Invalid regex"));
}

#[test]
fn pattern_outside_pattern_mode_rejected() {
    let e = err(&json!({"type":"console","id":"P3","description":"d","mode":"line","pattern":"x","frequency":"once"}));
    assert!(e.contains("only valid with mode='pattern'"), "{e}");
}

#[test]
fn type_params_enforced() {
    let missing = err(&json!({"type":"command","description":"d","mode":"exit","frequency":"once"}));
    assert!(missing.contains("requires a 'command'"), "{missing}");
    let foreign =
        err(&json!({"type":"console","id":"P1","command":"ls","description":"d","mode":"exit","frequency":"once"}));
    assert!(foreign.contains("'command' is only valid with type='command'"), "{foreign}");
    let bogus = err(&json!({"type":"bogus","description":"d","mode":"exit","frequency":"once"}));
    assert!(bogus.contains("Invalid type"), "{bogus}");
    let nodesc = err(&json!({"type":"console","id":"P1","mode":"exit","frequency":"once"}));
    assert!(nodesc.contains("'description'"), "{nodesc}");
}

#[test]
fn engine_batches_within_window() {
    let mut en = Engine::default();
    en.ingest(b"a\nb\n", LINE, 1_000);
    assert!(en.take_batch(1_000 + BATCH_WINDOW_MS - 1, false).is_none());
    en.ingest(b"c\n", LINE, 1_100);
    let batch = en.take_batch(1_000 + BATCH_WINDOW_MS, false).expect("flushed");
    assert_eq!(batch, vec!["a", "b", "c"]);
    assert!(en.take_batch(10_000, true).is_none());
}

#[test]
fn engine_keeps_partial_line() {
    let mut en = Engine::default();
    en.ingest(b"hel", LINE, 0);
    assert!(en.take_batch(1_000, true).is_none());
    en.ingest(b"lo\r\nwor", LINE, 0);
    assert_eq!(en.take_batch(1_000, true).expect("line"), vec!["hello"]);
    en.flush_partial(LINE, 0);
    assert_eq!(en.take_batch(1_000, true).expect("partial"), vec!["wor"]);
}

#[test]
fn engine_pattern_filters_per_line() {
    let re = regex::Regex::new("^MSG#\\d+").expect("re");
    let mut en = Engine::default();
    en.ingest(b"MSG#1 x\nnoise\n  MSG#2\nMSG#3\n", Filter { mode: Mode::Pattern, pattern: Some(&re) }, 0);
    assert_eq!(en.take_batch(1_000, false).expect("batch"), vec!["MSG#1 x", "MSG#3"]);
}

#[test]
fn engine_line_skips_blank() {
    let mut en = Engine::default();
    en.ingest(b"\n  \nx\n", LINE, 0);
    assert_eq!(en.take_batch(1_000, false).expect("batch"), vec!["x"]);
}

#[test]
fn flood_after_limit_in_window() {
    let mut en = Engine::default();
    for i in 0..FLOOD_MAX_EVENTS {
        assert_eq!(en.record_fire(u64::try_from(i).expect("u64")), Budget::Ok);
    }
    assert_eq!(en.record_fire(100), Budget::Flooded);
}

#[test]
fn flood_window_slides() {
    let mut en = Engine::default();
    for _ in 0..FLOOD_MAX_EVENTS {
        assert_eq!(en.record_fire(0), Budget::Ok);
    }
    assert_eq!(en.record_fire(FLOOD_WINDOW_MS), Budget::Ok);
}

#[test]
fn payload_inline_then_spilled() {
    assert_eq!(render_payload("small", "t_inline"), "small");
    let big = "y".repeat(INLINE_MAX_BYTES + 1);
    let out = render_payload(&big, "cp_monitor_unit_test");
    assert!(out.contains("/tmp/cp-monitor/cp_monitor_unit_test.log"), "{out}");
    assert!(out.len() < 600, "tail only");
    let written = std::fs::read_to_string("/tmp/cp-monitor/cp_monitor_unit_test.log").expect("spill file");
    assert_eq!(written.len(), big.len() + 1);
    let _r = std::fs::remove_file("/tmp/cp-monitor/cp_monitor_unit_test.log");
}

#[test]
fn ring_read_since_cursor() {
    let rb = RingBuffer::new();
    rb.write(b"old\n");
    let start = rb.total_written();
    rb.write(b"new1\nnew2\n");
    let (bytes, cur) = rb.read_since(start);
    assert_eq!(bytes, b"new1\nnew2\n");
    let (none, cur2) = rb.read_since(cur);
    assert!(none.is_empty());
    assert_eq!(cur, cur2);
}

#[test]
fn ring_read_since_across_wrap() {
    let rb = RingBuffer::new();
    let cap = crate::ring_buffer::RING_BUFFER_CAPACITY;
    rb.write(&vec![b'a'; cap - 3]);
    let start = rb.total_written();
    rb.write(b"123456");
    let (bytes, _) = rb.read_since(start);
    assert_eq!(bytes, b"123456");
    // Cursor older than retained data resumes at the oldest held byte.
    let (all, _) = rb.read_since(0);
    assert_eq!(all.len(), cap);
}

#[test]
fn ring_write_slices_match_bytewise() {
    let rb = RingBuffer::new();
    let cap = crate::ring_buffer::RING_BUFFER_CAPACITY;
    rb.write(&vec![b'x'; cap - 2]);
    rb.write(b"abcd");
    let (all, total) = rb.read_since(0);
    assert_eq!(total, u64::try_from(cap + 2).unwrap_or(u64::MAX));
    assert_eq!(all.len(), cap);
    assert!(all.ends_with(b"xxabcd"));
    // Oversized single write keeps only its last `cap` bytes.
    let mut big = vec![b'y'; cap];
    big.extend_from_slice(b"END");
    rb.write(&big);
    let (held, _) = rb.read_since(0);
    assert_eq!(held.len(), cap);
    assert!(held.ends_with(b"yyEND"));
}

#[test]
fn pattern_ignores_output_before_cursor() {
    let rb = RingBuffer::new();
    let m = Matcher::new("error|done");
    rb.write(b"old error\n");
    let cursor = rb.total_written();
    assert_eq!(rb.find_since(cursor, &m), None, "stale match must not fire");
    rb.write(b"build done\n");
    let end = rb.find_since(cursor, &m).expect("new output matches");
    assert_eq!(end, cursor + 10u64, "end lands just past 'build done'");
    // Advancing past the match consumes it.
    assert_eq!(rb.find_since(end, &m), None);
}

#[test]
fn invalid_regex_falls_back_to_literal() {
    let rb = RingBuffer::new();
    rb.write(b"value [x( here");
    assert!(rb.find_since(0, &Matcher::new("[x(")).is_some());
}

#[test]
fn last_n_lines_reads_tail() {
    let rb = RingBuffer::new();
    for i in 0u32..10_000u32 {
        rb.write(format!("line {i}\n").as_bytes());
    }
    assert_eq!(rb.last_n_lines(2), "line 9998\nline 9999");
}

#[test]
fn orphan_owner_parsing() {
    use crate::manager::client::owner_of;
    assert_eq!(owner_of("c_A984_3"), Some("A984"));
    assert_eq!(owner_of("eb_root_12"), Some("root"));
    assert_eq!(owner_of("c_3"), None, "legacy key without owner is never killed");
    assert_eq!(owner_of("c_A1_x"), None);
}

#[test]
fn status_poller_tolerates_brief_outage() {
    use crate::pollers::unreachable_is_fatal;
    assert!(!unreachable_is_fatal(1));
    assert!(!unreachable_is_fatal(5));
    assert!(unreachable_is_fatal(6));
}
