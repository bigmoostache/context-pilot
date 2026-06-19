#!/usr/bin/env python3
"""T268 reconnect-replay probe — proves the SSE resume query-param name.

The web client disables EventSource's native auto-reconnect, so the
`Last-Event-ID` *header* is never sent; resume rides a query param. The
backend reads `last_rev`. This probe connects with each candidate param and
reports whether the backend replays the deltas after a given rev — i.e. it
deterministically observes the bug (wrong name => no replay => the missed
message only surfaces on the 15s backstop poll => the 5-10s delay) and, after
the fix, confirms replay works with the name the client actually sends.

Usage: python3 sse_reconnect_probe.py [AGENT_ID]
"""
import json
import sys
import time
import urllib.request

BASE = "http://localhost:7878"
AGENT = sys.argv[1] if len(sys.argv) > 1 else "f3a993c0ff357b41"


def mint() -> str:
    req = urllib.request.Request(f"{BASE}/api/ticket", method="POST")
    with urllib.request.urlopen(req, timeout=5) as r:
        return json.load(r)["ticket"]


def head_rev() -> int:
    with urllib.request.urlopen(f"{BASE}/api/agent/{AGENT}/metrics", timeout=5) as r:
        return json.load(r)["rev"]["oplogHead"]


def count_replayed(param: str, resume_from: int, window: float = 2.0) -> int:
    """Connect with ?<param>=<resume_from>; count delta ids > resume_from seen
    within `window` seconds of opening (i.e. immediate replay, not future)."""
    ticket = mint()
    url = f"{BASE}/api/stream?agent={AGENT}&ticket={ticket}&{param}={resume_from}"
    seen = 0
    deadline = time.time() + window
    # Short per-read socket timeout so an idle stream (no further data) ends the
    # window promptly instead of blocking; replay deltas arrive immediately on
    # connect, so a couple hundred ms of quiet means replay is done.
    r = urllib.request.urlopen(url, timeout=window + 2)
    r.fp.raw._sock.settimeout(0.4)
    cur_id = None
    try:
        while time.time() < deadline:
            try:
                line = r.readline()
            except Exception:
                break  # idle (socket read timeout) => replay window over
            if not line:
                break
            s = line.decode("utf-8", "replace").rstrip("\n")
            if s.startswith("id:"):
                cur_id = s[3:].strip()
            elif s.startswith("event:") and "delta" in s:
                if cur_id and cur_id.isdigit() and int(cur_id) > resume_from:
                    seen += 1
    finally:
        r.close()
    return seen


def main() -> None:
    h = head_rev()
    resume = max(0, h - 5)
    print(f"agent={AGENT} oplogHead={h} resume_from={resume}")
    print("(counting delta ids > resume seen in the first 2s = immediate replay)")
    for param in ("last_event_id", "last_rev"):
        n = count_replayed(param, resume)
        verdict = "REPLAYED ✅" if n > 0 else "NO REPLAY ❌ (cold seed at head)"
        print(f"  ?{param:<14}= {resume}  ->  {n} replayed delta(s)  {verdict}")


if __name__ == "__main__":
    main()
