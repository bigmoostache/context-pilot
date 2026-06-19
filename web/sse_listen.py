#!/usr/bin/env python3
"""SSE listener — print all deltas with wall-clock timestamps.

Does NOT send any message. Just listens and prints every delta frame
so we can correlate with TUI-originated messages.
"""
import json, time, urllib.request, sys

API = "http://localhost:7878"
AGENT = sys.argv[1] if len(sys.argv) > 1 else "f3a993c0ff357b41"

def post(path, body):
    req = urllib.request.Request(API + path, data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json"}, method="POST")
    return json.load(urllib.request.urlopen(req, timeout=5))

ticket = post("/api/ticket", {})["ticket"]
url = f"{API}/api/stream?agent={AGENT}&ticket={ticket}"
print(f"SSE listening on agent={AGENT}", flush=True)
print(f"[{time.strftime('%H:%M:%S')}] connected", flush=True)

with urllib.request.urlopen(url, timeout=120) as r:
    ev, data = None, None
    for line in r:
        line = line.decode().rstrip("\n")
        if line.startswith("event:"): ev = line[6:].strip()
        elif line.startswith("data:"): data = line[5:].strip()
        elif line == "":
            if ev == "delta" and data:
                ts = time.strftime('%H:%M:%S')
                ms = int(time.time() * 1000) % 100000
                try:
                    j = json.loads(data)
                    k = j.get("kind", {})
                    kind = k.get("kind", "?")
                    if kind == "message_created":
                        ib = k.get("inline_body")
                        txt = None
                        if ib:
                            try: txt = json.loads(ib).get("text", "")[:80]
                            except: txt = "<unparseable>"
                        print(f"[{ts}.{ms:05d}] message_created tid={k.get('thread_id')} "
                              f"text={txt!r}", flush=True)
                    else:
                        print(f"[{ts}.{ms:05d}] {kind}", flush=True)
                except Exception as e:
                    print(f"[{ts}.{ms:05d}] parse-err {e}", flush=True)
            ev, data = None, None
