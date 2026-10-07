#!/usr/bin/env python3
"""A JSON-RPC stand-in for a LEZ sequencer, for testing the CLI's read-only
commands without a network.

  rpc-fixture.py record UPSTREAM FIXTURE [--port N]
      Forwards every request to UPSTREAM and saves each (method, params) with
      its answer in FIXTURE.
  rpc-fixture.py replay FIXTURE [--port N]
      Answers from FIXTURE only. A request it has no answer for gets a JSON-RPC
      error and is printed on stderr, so a command that asks for something the
      recording does not hold fails rather than silently reading nothing.

The fixture is a JSON object keyed by the canonical JSON of [method, params].
Answers are kept whole (the "result" or "error" member), so a replay returns
what the sequencer returned, byte for byte apart from the request id.
"""
import json
import sys
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def key(req):
    return json.dumps([req.get("method"), req.get("params", [])], sort_keys=True, separators=(",", ":"))


def main():
    args = sys.argv[1:]
    port = 3040
    if "--port" in args:
        i = args.index("--port")
        port = int(args[i + 1])
        del args[i : i + 2]
    if not args or args[0] not in ("record", "replay"):
        sys.exit(__doc__)
    mode = args[0]
    if mode == "record":
        upstream, path = args[1], args[2]
        table = {}
    else:
        path = args[1]
        upstream = None
        table = json.load(open(path))

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
            req = json.loads(body)
            batch = isinstance(req, list)
            out = [self.answer(r) for r in (req if batch else [req])]
            data = json.dumps(out if batch else out[0]).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def answer(self, req):
            k = key(req)
            if upstream:
                fwd = json.dumps({"jsonrpc": "2.0", "id": 1, "method": req["method"], "params": req.get("params", [])})
                r = urllib.request.Request(upstream, fwd.encode(), {"Content-Type": "application/json"})
                with urllib.request.urlopen(r, timeout=30) as resp:
                    got = json.load(resp)
                table[k] = {m: got[m] for m in ("result", "error") if m in got}
                with open(path, "w") as f:
                    json.dump(table, f, indent=1, sort_keys=True)
                    f.write("\n")
            ans = table.get(k)
            if ans is None:
                print(f"rpc-fixture: no recorded answer for {k}", file=sys.stderr, flush=True)
                ans = {"error": {"code": -32000, "message": f"not in the fixture: {k}"}}
            return {"jsonrpc": "2.0", "id": req.get("id"), **ans}

    srv = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"rpc-fixture: {mode} on http://127.0.0.1:{port}", file=sys.stderr, flush=True)
    srv.serve_forever()


if __name__ == "__main__":
    main()
