use super::super::super::ContentBlock;
use super::*;

/// Helper: create a simple text content block.
fn make_text_block(text: &str) -> ContentBlock {
    ContentBlock::Text { text: text.to_owned() }
}

/// Helper: create N alternating user/assistant messages, one text block each.
fn make_messages(block_count: usize) -> Vec<ApiMessage> {
    let mut msgs: Vec<ApiMessage> = Vec::new();
    for idx in 0..block_count {
        let role = if idx & 1 == 0 { "user" } else { "assistant" };
        msgs.push(ApiMessage { role: role.to_owned(), content: vec![make_text_block(&format!("block_{idx}"))] });
    }
    msgs
}

#[test]
fn accumulated_hashes_are_chained() {
    let msgs = make_messages(3);
    let infos = compute_accumulated_hashes(&msgs);
    assert_eq!(infos.len(), 3);
    // Each hash should be different (chained) — compare adjacent pairs via windows
    for pair in infos.windows(2) {
        let left = pair.first().map(|x| &x.acc_hash);
        let right = pair.get(1).map(|x| &x.acc_hash);
        assert_ne!(left, right);
    }
    // Cumulative tokens should be non-decreasing
    for pair in infos.windows(2) {
        let left = pair.first().map(|x| x.cumulative_tokens);
        let right = pair.get(1).map(|x| x.cumulative_tokens);
        assert!(left <= right);
    }
}

#[test]
fn same_content_produces_same_hash() {
    let msgs1 = make_messages(3);
    let msgs2 = make_messages(3);
    let infos1 = compute_accumulated_hashes(&msgs1);
    let infos2 = compute_accumulated_hashes(&msgs2);
    for (hash_a, hash_b) in infos1.iter().zip(infos2.iter()) {
        assert_eq!(hash_a.acc_hash, hash_b.acc_hash);
    }
}

#[test]
fn tool_use_different_inputs_produce_different_hashes() {
    let msg_a = vec![ApiMessage {
        role: "assistant".to_owned(),
        content: vec![ContentBlock::ToolUse {
            id: "call_1".to_owned(),
            name: "Edit".to_owned(),
            input: serde_json::json!({"file_path": "/foo.rs", "old_string": "aaa", "new_string": "bbb"}),
        }],
    }];
    let msg_b = vec![ApiMessage {
        role: "assistant".to_owned(),
        content: vec![ContentBlock::ToolUse {
            id: "call_1".to_owned(),
            name: "Edit".to_owned(),
            input: serde_json::json!({"file_path": "/bar.rs", "old_string": "xxx", "new_string": "yyy"}),
        }],
    }];

    let infos_a = compute_accumulated_hashes(&msg_a);
    let infos_b = compute_accumulated_hashes(&msg_b);

    assert_eq!(infos_a.len(), 1);
    assert_eq!(infos_b.len(), 1);
    let hash_a = infos_a.first().map(|x| &x.acc_hash);
    let hash_b = infos_b.first().map(|x| &x.acc_hash);
    assert_ne!(hash_a, hash_b, "ToolUse with same name but different inputs must hash differently");
}

#[test]
fn tool_use_different_ids_produce_different_hashes() {
    let input = serde_json::json!({"query": "hello"});
    let msg_a = vec![ApiMessage {
        role: "assistant".to_owned(),
        content: vec![ContentBlock::ToolUse {
            id: "call_aaa".to_owned(),
            name: "brave_search".to_owned(),
            input: input.clone(),
        }],
    }];
    let msg_b = vec![ApiMessage {
        role: "assistant".to_owned(),
        content: vec![ContentBlock::ToolUse { id: "call_bbb".to_owned(), name: "brave_search".to_owned(), input }],
    }];

    let infos_a = compute_accumulated_hashes(&msg_a);
    let infos_b = compute_accumulated_hashes(&msg_b);

    let hash_a = infos_a.first().map(|x| &x.acc_hash);
    let hash_b = infos_b.first().map(|x| &x.acc_hash);
    assert_ne!(hash_a, hash_b, "ToolUse with same name+inputs but different IDs must hash differently");
}

#[test]
fn prune_removes_old_entries() {
    let mut engine = CacheEngine::default();
    let now = 1_000_000u64;
    engine.breakpoints.push(BreakpointEntry {
        acc_hash: "old".to_owned(),
        timestamp_ms: now.saturating_sub(CACHE_TTL_MS).saturating_sub(1),
    });
    engine.breakpoints.push(BreakpointEntry { acc_hash: "fresh".to_owned(), timestamp_ms: now });
    engine.prune(now);
    assert_eq!(engine.breakpoints.len(), 1);
    assert_eq!(engine.breakpoints.first().map(|bp| bp.acc_hash.as_str()), Some("fresh"));
}

#[test]
fn frontier_detection() {
    let msgs = make_messages(10);
    let infos = compute_accumulated_hashes(&msgs);

    let mut engine = CacheEngine::default();
    let hash_at_4 = infos.get(4).map(|x| x.acc_hash.clone()).unwrap_or_default();
    engine.breakpoints.push(BreakpointEntry { acc_hash: hash_at_4, timestamp_ms: 999_999 });

    let frontier = engine.find_cache_frontier(&infos);
    assert_eq!(frontier, Some(4));
}

#[test]
fn beacon_placed_after_frontier() {
    let msgs = make_messages(40);

    let mut engine = CacheEngine::default();
    let infos = compute_accumulated_hashes(&msgs);
    // Frontier at block 10 → beacon should be at block 30 (10 + 20)
    let hash_at_10 = infos.get(10).map(|x| x.acc_hash.clone()).unwrap_or_default();
    engine.breakpoints.push(BreakpointEntry { acc_hash: hash_at_10, timestamp_ms: 999_999 });

    let plan = engine.compute_breakpoints(&msgs);
    // Beacon at 0-indexed position 30 → msg_idx 30
    let has_beacon_near_30 = plan.positions.iter().any(|(msg_idx, _)| *msg_idx >= 28 && *msg_idx <= 32);
    assert!(has_beacon_near_30, "expected beacon near position 30, got {:?}", plan.positions);
}

#[test]
fn no_frontier_beacon_at_tail() {
    let msgs = make_messages(10);
    let engine = CacheEngine::default(); // no stored BPs

    let plan = engine.compute_breakpoints(&msgs);
    // With no frontier, beacon goes to last block (idx 9)
    let has_tail = plan.positions.iter().any(|(msg_idx, _)| *msg_idx >= 7);
    assert!(has_tail, "expected beacon near tail, got {:?}", plan.positions);
}

#[test]
fn record_and_retrieve() {
    let mut engine = CacheEngine::default();
    let hashes = vec!["hash_a".to_owned(), "hash_b".to_owned()];
    engine.record_breakpoints(&hashes, 1_000_000);
    assert_eq!(engine.breakpoints.len(), 2);

    // Recording same hash again should refresh, not duplicate
    engine.record_breakpoints(&["hash_a".to_owned()], 2_000_000);
    assert_eq!(engine.breakpoints.len(), 2);
    let ts = engine.breakpoints.iter().find(|bp| bp.acc_hash == "hash_a").map(|bp| bp.timestamp_ms);
    assert_eq!(ts, Some(2_000_000));
}

#[test]
fn serialization_roundtrip() {
    let mut engine = CacheEngine::default();
    engine.breakpoints.push(BreakpointEntry { acc_hash: "test_hash".to_owned(), timestamp_ms: 12345 });

    let json = engine.to_json();
    let restored = CacheEngine::from_json(&json);
    assert_eq!(restored.breakpoints.len(), 1);
    assert_eq!(restored.breakpoints.first().map(|bp| bp.acc_hash.as_str()), Some("test_hash"));
    assert_eq!(restored.breakpoints.first().map(|bp| bp.timestamp_ms), Some(12345));
}

#[test]
fn empty_prompt() {
    let engine = CacheEngine::default();
    let plan = engine.compute_breakpoints(&[]);
    assert!(plan.positions.is_empty());
    assert!(plan.bp_hashes.is_empty());
}

#[test]
fn plan_respects_four_bp_limit() {
    let msgs = make_messages(100);
    let engine = CacheEngine::default();

    let plan = engine.compute_breakpoints(&msgs);
    assert!(plan.positions.len() <= 4, "too many BPs: {}", plan.positions.len());
}

#[test]
fn optimizer_spreads_bps() {
    let msgs = make_messages(100);
    let engine = CacheEngine::default();

    let plan = engine.compute_breakpoints(&msgs);
    assert!(!plan.positions.is_empty());

    // BPs should not all be clustered in the same region
    if plan.positions.len() >= 3 {
        let mut msg_indices: Vec<usize> = plan.positions.iter().map(|(m, _)| *m).collect();
        msg_indices.sort_unstable();
        let last = msg_indices.last().copied().unwrap_or(0);
        let first = msg_indices.first().copied().unwrap_or(0);
        let span = last.saturating_sub(first);
        assert!(span > 10, "BPs too clustered: {msg_indices:?}");
    }
}

#[test]
fn alive_bps_become_omega() {
    let msgs = make_messages(60);
    let infos = compute_accumulated_hashes(&msgs);

    let mut engine = CacheEngine::default();
    // Store BPs at positions 15 and 30 — these become Ω
    let hash_15 = infos.get(15).map(|x| x.acc_hash.clone()).unwrap_or_default();
    let hash_30 = infos.get(30).map(|x| x.acc_hash.clone()).unwrap_or_default();
    engine.record_breakpoints(&[hash_15, hash_30], 999_000);

    let plan = engine.compute_breakpoints(&msgs);
    assert_eq!(plan.alive_count, 2);
    assert!(plan.positions.len() <= 4);
    // Optimizer should place cuts respecting the alive BP boundaries
    assert!(!plan.bp_hashes.is_empty());
}

#[test]
fn full_pipeline_with_frontier() {
    let msgs = make_messages(60);
    let infos = compute_accumulated_hashes(&msgs);

    let mut engine = CacheEngine::default();
    let hash_20 = infos.get(20).map(|x| x.acc_hash.clone()).unwrap_or_default();
    engine.record_breakpoints(&[hash_20], 999_000);

    let plan = engine.compute_breakpoints(&msgs);

    assert!(!plan.positions.is_empty());
    assert!(plan.positions.len() <= 4);
    assert_eq!(plan.alive_count, 1);

    // Beacon should be around block 40 (20 + LOOKBACK_WINDOW)
    let has_near_40 = plan.positions.iter().any(|(msg_idx, _)| *msg_idx >= 35 && *msg_idx <= 45);
    assert!(has_near_40, "expected a BP near block 40, got {:?}", plan.positions);
}

#[test]
fn deterministic() {
    let msgs = make_messages(50);
    let engine = CacheEngine::default();

    let r1 = engine.compute_breakpoints(&msgs);
    let r2 = engine.compute_breakpoints(&msgs);
    assert_eq!(r1.positions, r2.positions);
    assert_eq!(r1.bp_hashes, r2.bp_hashes);
}
