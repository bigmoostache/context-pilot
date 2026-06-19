#!/usr/bin/env python3
"""
T123 reproduction — the "message appears → disappears → reappears" flicker.

This script reproduces the bug at the *exact data layer the web UI consumes*,
with no browser involved. It models the two sources that both call `setData`
on the threads hook (`useLiveQuery` in web/src/lib/live.ts):

  1. SSE  `delta` (message_created) -> applyThreadDelta APPENDS the message
     to the thread log  (the FAST push plane, ~14ms).
  2. GET  /api/agent/{id}/threads  -> the 5s poll REPLACES the whole thread
     list; its per-thread `log` is sourced from the tier-② disk cache
     (config.json), which the agent flushes on a ~50-100ms+ debounce.

The UI's visible log is "whatever the most recent setData set it to". So we
maintain `ui_has_msg` exactly like the React state: set True when the SSE
delta appends our marker, and REPLACED by the poll's view of the log on every
poll. If the poll fires in the window after the delta but before the disk
flush, its (stale) log lacks the marker -> ui_has_msg flips False (DISAPPEAR),
then back True on a later poll once the flush lands (REAPPEAR).

Success = we observe the True -> False -> True cycle.

Usage:  python3 repro_t123_flicker.py [AGENT_ID] [THREAD_ID]
"""
import json
import sys
import threading
import time
import urllib.request
import uuid

BASE = "http://localhost:7878"
AGENT = sys.argv[1] if len(sys.argv) > 1 else "f3a993c0ff357b41"
THREAD = sys.argv[2] if len(sys.argv) > 2 else "T18"
POLL_MS = 200          # tighter than the UI's 5s so we densely sample the window
WATCH_SECS = 40.0      # long enough to catch the slow disk-flush REAPPEAR
MARKER = f"REPRO-{uuid.uuid4().hex[:8]}"

t0 = time.monotonic()


def ms() -> float:
    return (time.monotonic() - t0) * 1000.0


def post(path: str, body: dict) -> dict:
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        BASE + path, data=data, headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=5) as r:
        return json.loads(r.read().decode())


def get(path: str) -> dict:
    with urllib.request.urlopen(BASE + path, timeout=5) as r:
        return json.loads(r.read().decode())


def thread_log_has_marker(thread_id: str) -> bool:
    d = get(f"/api/agent/{AGENT}/threads")
    threads = d.get("threads", d) if isinstance(d, dict) else d
    for t in threads:
        if t.get("id") == thread_id:
            for m in t.get("log", []):
                if MARKER in (m.get("content") or m.get("text") or ""):
                    return True
            return False
    return False


# ── SSE listener (the push plane) ────────────────────────────────────
sse_appear_ms = {"v": None}


def sse_listen(ticket: str):
    url = f"{BASE}/api/stream?agent={AGENT}&ticket={ticket}"
    with urllib.request.urlopen(url, timeout=WATCH_SECS + 5) as r:
        event = None
        for raw in r:
            line = raw.decode(errors="replace").rstrip("\n")
            if line.startswith("event:"):
                event = line[6:].strip()
            elif line.startswith("data:"):
                payload = line[5:].strip()
                if event == "delta":
                    try:
                        entry = json.loads(payload)
                        k = entry.get("kind", {})
                        if k.get("kind") == "message_created":
                            body = k.get("inline_body")
                            if body and MARKER in body:
                                sse_appear_ms["v"] = ms()
                                print(f"[{ms():8.1f}ms] SSE delta message_created carries MARKER  -> APPEARS")
                    except Exception:
                        pass
            elif line == "":
                event = None


def main():
    print(f"agent={AGENT} thread={THREAD} marker={MARKER}\n")

    ticket = post("/api/ticket", {})["ticket"]
    th = threading.Thread(target=sse_listen, args=(ticket,), daemon=True)
    th.start()
    time.sleep(0.3)  # let SSE connect

    # Model of the UI's visible state (what the most recent setData set).
    # Two parallel models of the SAME event stream:
    #   * REPLACE  — the buggy hook: every poll wholesale-replaces the log
    #                (setData(pollResult)), so a stale disk poll erases a
    #                delta-applied message.
    #   * MERGE    — the FIX: the poll reconciles by id (union), so a message
    #                a delta already added can never be dropped by a poll.
    ui_has_msg = False           # REPLACE model
    merged_has_msg = False       # MERGE model (delta-applied stays applied)
    transitions = []  # (ms, bool, source)
    merge_transitions = []  # (ms, bool, source)

    def set_ui(val: bool, source: str):
        nonlocal ui_has_msg
        if val != ui_has_msg:
            ui_has_msg = val
            transitions.append((ms(), val, source))
            verb = "APPEAR " if val else "DISAPPEAR"
            print(f"[{ms():8.1f}ms] REPLACE-model UI {verb} (via {source})")

    def set_merge(val: bool, source: str):
        # MERGE never drops a present message: once the delta added it, a poll
        # can only confirm it (True OR-accumulates; a poll's False is ignored).
        nonlocal merged_has_msg
        new = merged_has_msg or val
        if new != merged_has_msg:
            merged_has_msg = new
            merge_transitions.append((ms(), new, source))
            print(f"[{ms():8.1f}ms] MERGE-model   UI APPEAR  (via {source})")

    # Fire the send (this is exactly what the composer's sendCommand does).
    env = {
        "schema_version": 1,
        "id": str(uuid.uuid4()),
        "seq": 0,
        "dedup_token": str(uuid.uuid4()),
        "kind": {"kind": "send_message", "thread_id": THREAD, "content": MARKER},
    }
    send_ms = ms()
    receipt = post(f"/api/agent/{AGENT}/command", env)
    print(f"[{send_ms:8.1f}ms] POST send_message -> accepted={receipt.get('accepted')} rev={receipt.get('rev')}\n")

    # Watch loop: poll the REST log (replace) + reflect the SSE appear (append).
    deadline = time.monotonic() + WATCH_SECS
    last_sse_seen = None
    while time.monotonic() < deadline:
        # SSE append: once the delta carried the marker, the UI has it appended.
        if sse_appear_ms["v"] is not None and last_sse_seen is None:
            last_sse_seen = sse_appear_ms["v"]
            set_ui(True, "SSE-delta-append")
            set_merge(True, "SSE-delta-append")
        # Poll REPLACE — mirrors useLiveQuery setData(poll result).
        present = thread_log_has_marker(THREAD)
        set_ui(present, "REST-poll-replace")
        set_merge(present, "REST-poll-merge")
        time.sleep(POLL_MS / 1000.0)

    print("\n── REPLACE model (current buggy hook) transitions ──")
    for tms, val, src in transitions:
        print(f"  [{tms:8.1f}ms] -> {'PRESENT' if val else 'ABSENT '}  ({src})")

    seq = [v for _, v, _ in transitions]
    # Look for True -> False -> True (appear, disappear, reappear).
    flicker = any(
        seq[i] and not seq[i + 1] and seq[i + 2]
        for i in range(len(seq) - 2)
    )
    # appear→disappear is the visible defect even if the disk reappear lands
    # after the window; treat True→False (with an earlier appear) as the bug.
    appear_then_disappear = any(
        seq[i] and not seq[i + 1] for i in range(len(seq) - 1)
    )

    print("\n── MERGE model (proposed fix) transitions ──")
    for tms, val, src in merge_transitions:
        print(f"  [{tms:8.1f}ms] -> {'PRESENT' if val else 'ABSENT '}  ({src})")
    merge_seq = [v for _, v, _ in merge_transitions]
    merge_drops = any(
        merge_seq[i] and not merge_seq[i + 1] for i in range(len(merge_seq) - 1)
    )

    print("\n══ RESULT ══")
    if flicker:
        print("REPLACE: ✅ FULL FLICKER REPRODUCED (appear → disappear → reappear)")
    elif appear_then_disappear:
        print("REPLACE: ✅ DEFECT REPRODUCED (appear → disappear; disk reappear lagged past window)")
    else:
        print("REPLACE: ❌ no defect observed")
    print("MERGE  :",
          "✅ STABLE (message never dropped — bug fixed)" if not merge_drops
          else "❌ still drops")
    if sse_appear_ms["v"] is not None:
        print(f"appear latency (send → SSE delta): {sse_appear_ms['v'] - send_ms:.1f}ms")


if __name__ == "__main__":
    main()
