#!/usr/bin/env python3
"""A stand-in for llama.cpp's llama-server, for the suite's end-to-end smoke test.

    python tools/win-app/stub_llm.py --port PORT

It answers, on 127.0.0.1 only, exactly what mimir-server's sidecar client calls
(mimir/model-server/src/backends/openai_compat.rs, mimir/mimir-server/src/main.rs):

  GET  /health          200 {"status":"ok"}: the sidecar is ready
  GET  /props           the context size and slot count the Mimir console shows
  POST /v1/completions  {"prompt", "max_tokens", "stream": true, ...}, answered with
                        server-sent events: `data: {"choices":[{"text":...}]}` pieces,
                        a usage event, then `data: [DONE]`

Every completion is ANSWER: a valid A.1 reply (mimir/prompts, heimdall/decision-support
src/ask.rs) that cites the area summary, which every Heimdall question's grounding
carries, so an ask through the suite comes back grounded. A prompt without that
grounding (anything but an ask) gets the same reply; nothing here reads the model.
Each request is logged as one line on standard output, without the prompt.
"""
import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# The citation id of the area summary (heimdall/decision-support/src/ask.rs).
AREA_SUMMARY_ID = "AREA-SUMMARY"
ANSWER_TEXT = "Stub model answer: the fixture's area summary is all this rests on."
ANSWER = {
    "plan": ["read the area summary"],
    "answer": ANSWER_TEXT,
    "citations": [f"obj:{AREA_SUMMARY_ID}"],
    "proposed_actions": [],
    "inferences": [],
    "refusal": {"code": None, "detail": None},
}
MAX_BODY = 4 * 1024 * 1024


class Handler(BaseHTTPRequestHandler):
    server_version = "stub-llm/1"

    def log_message(self, fmt, *args):  # one line per request, on stdout
        print(f"stub-llm: {self.command} {self.path} {fmt % args}", flush=True)

    def reply_json(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/health":
            self.reply_json(200, {"status": "ok"})
        elif self.path == "/props":
            self.reply_json(200, {"default_generation_settings": {"n_ctx": 4096}, "total_slots": 1})
        else:
            self.reply_json(404, {"error": {"message": f"no route {self.path}"}})

    def do_POST(self):
        if self.path != "/v1/completions":
            self.reply_json(404, {"error": {"message": f"no route {self.path}"}})
            return
        try:
            n = int(self.headers.get("Content-Length", "0"))
            if not 0 < n <= MAX_BODY:
                raise ValueError("no body, or one too large")
            req = json.loads(self.rfile.read(n))
            if not isinstance(req.get("prompt"), str) or req.get("stream") is not True:
                raise ValueError("expected a prompt and stream: true")
        except ValueError as e:
            self.reply_json(400, {"error": {"message": str(e)}})
            return
        grounded = f"[obj:{AREA_SUMMARY_ID}]" in req["prompt"]
        print(f"stub-llm: completion for a {len(req['prompt'])}-character prompt"
              f"{' with' if grounded else ' without'} the area summary", flush=True)
        text = json.dumps(ANSWER)
        half = len(text) // 2
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Connection", "close")
        self.end_headers()
        for piece in (text[:half], text[half:]):
            self.wfile.write(f"data: {json.dumps({'choices': [{'text': piece}]})}\n\n".encode())
        usage = {"choices": [{"text": ""}],
                 "usage": {"prompt_tokens": max(1, len(req["prompt"].split())), "completion_tokens": 2}}
        self.wfile.write(f"data: {json.dumps(usage)}\n\ndata: [DONE]\n\n".encode())
        self.wfile.flush()


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--port", type=int, required=True)
    port = ap.parse_args().port
    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    print(f"stub-llm: listening on 127.0.0.1:{server.server_address[1]}", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
