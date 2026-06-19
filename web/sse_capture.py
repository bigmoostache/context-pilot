#!/usr/bin/env python3
"""T123 SSE capture — isolate backend push plane from frontend.

Mint a ticket, open the SSE stream, send a user message, and print every
`delta` frame with latency. Tells us whether the backend emits a
`message_created` delta fast (with inline_body+text) — if so, any slow
appearance is a FRONTEND apply failure, not a backend one.
"""
import json, time, threading, urllib.request, sys

API = "http://localhost:7878"
AGENT = "f3a993c0ff357b41"
MARKER = f"sse-capture-{int(time.time()*1000)}"

def post(path, body):
    req = urllib.request.Request(API + path, data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"}, method="POST")
    return json.load(urllib.request.urlopen(req, timeout=5))

def get(path):
    return json.load(urllib.request.urlopen(API + path, timeout=5))

# pick a thread (first non-archived)
raw = get(f"/api/agent/{AGENT}/threads")
threads = raw if isinstance(raw, list) else raw.get("threads", [])
tid = next(t["id"] for t in threads if not t.get("archived"))
print(f"thread={tid} marker={MARKER}", flush=True)

ticket = post("/api/ticket", {})["ticket"]
t0 = None
deltas = []

def stream():
    url = f"{API}/api/stream?agent={AGENT}&ticket={ticket}"
    with urllib.request.urlopen(url, timeout=30) as r:
        ev, data = None, None
        for line in r:
            line = line.decode().rstrip("\n")
            if line.startswith("event:"): ev = line[6:].strip()
            elif line.startswith("data:"): data = line[5:].strip()
            elif line == "":
                if ev == "delta" and data and t0 is not None:
                    dt = (time.time() - t0) * 1000
                    try:
                        j = json.loads(data)
                        k = j.get("kind", {})
                        if k.get("kind") == "message_created":
                            ib = k.get("inline_body")
                            txt = None
                            if ib:
                                try: txt = json.loads(ib).get("text")
                                except Exception: txt = "<unparseable>"
                            print(f"[{dt:8.1f}ms] message_created tid={k.get('thread_id')} "
                                  f"inline_body={'YES' if ib else 'NO'} text={txt!r}", flush=True)
                            deltas.append((dt, k))
                        else:
                            print(f"[{dt:8.1f}ms] {k.get('kind')}", flush=True)
                    except Exception as e:
                        print(f"[{dt:8.1f}ms] parse-err {e}", flush=True)
                ev, data = None, None

th = threading.Thread(target=stream, daemon=True); th.start()
time.sleep(0.8)  # let stream connect
t0 = time.time()
env = {"schema_version":1,"id":MARKER,"seq":0,"dedup_token":MARKER,
       "kind":{"kind":"send_message","thread_id":tid,"content":MARKER}}
post(f"/api/agent/{AGENT}/command", env)
print(f"[    0.0ms] POST send_message sent", flush=True)
time.sleep(12)
print("\n== summary ==", flush=True)
mc = [d for d in deltas if d[1].get("thread_id")==tid]
if mc:
    print(f"first message_created for our thread @ {mc[0][0]:.1f}ms, inline_body={'YES' if mc[0][1].get('inline_body') else 'NO'}", flush=True)
else:
    print("NO message_created delta for our thread within 12s — backend push plane did NOT emit", flush=True)
print("DONE", flush=True)
