#!/usr/bin/env python3
"""Measure TUI→SSE message latency by comparing message timestamp vs arrival time.

Listens to SSE deltas. For each message_created, extracts the `ts` field
from inline_body and compares it with the wall-clock arrival time.
This reveals whether the delay is in the TUI→oplog→SSE chain.
"""
import json, time, urllib.request, sys, re
from datetime import datetime

API = "http://localhost:7878"
AGENT = sys.argv[1] if len(sys.argv) > 1 else "f3a993c0ff357b41"

def post(path, body):
    req = urllib.request.Request(API + path, data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"}, method="POST")
    return json.load(urllib.request.urlopen(req, timeout=5))

ticket = post("/api/ticket", {})["ticket"]
url = f"{API}/api/stream?agent={AGENT}&ticket={ticket}"
print(f"SSE latency probe — agent={AGENT}", flush=True)
print(f"Listening for message_created deltas...", flush=True)
print(f"{'arrival':>12} {'msg_ts':>20} {'delta_ms':>10} {'text'}", flush=True)
print("-" * 80, flush=True)

with urllib.request.urlopen(url, timeout=300) as r:
    ev, data = None, None
    for line in r:
        line = line.decode().rstrip("\n")
        if line.startswith("event:"): ev = line[6:].strip()
        elif line.startswith("data:"): data = line[5:].strip()
        elif line == "":
            if ev == "delta" and data:
                arrival_ms = int(time.time() * 1000)
                arrival_str = time.strftime('%H:%M:%S')
                try:
                    j = json.loads(data)
                    k = j.get("kind", {})
                    kind = k.get("kind", "?")
                    rev = j.get("rev", "?")
                    if kind == "message_created":
                        ib = k.get("inline_body")
                        txt, msg_ts = None, None
                        if ib:
                            try:
                                body = json.loads(ib)
                                txt = (body.get("text") or "")[:60]
                                msg_ts = body.get("ts", "")
                            except: pass
                        # Try to parse msg_ts as epoch ms or as a datetime string
                        delta_ms = "?"
                        if msg_ts:
                            try:
                                # Try epoch ms
                                ts_ms = int(msg_ts)
                                delta_ms = arrival_ms - ts_ms
                            except (ValueError, TypeError):
                                try:
                                    # Try ISO format or common datetime format
                                    # e.g. "2026-06-19 15:30:01" or "2026-06-19T15:30:01"
                                    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S",
                                                "%Y-%m-%d %H:%M:%S%.f", "%H:%M:%S"]:
                                        try:
                                            dt = datetime.strptime(str(msg_ts), fmt)
                                            # If only time, assume today
                                            if fmt == "%H:%M:%S":
                                                now = datetime.now()
                                                dt = dt.replace(year=now.year, month=now.month, day=now.day)
                                            delta_ms = int((datetime.now() - dt).total_seconds() * 1000)
                                            break
                                        except: continue
                                except: pass
                        
                        print(f"{arrival_str:>12} {str(msg_ts):>20} {str(delta_ms):>10} {txt!r}", flush=True)
                except Exception as e:
                    print(f"parse-err: {e}", flush=True)
            ev, data = None, None
