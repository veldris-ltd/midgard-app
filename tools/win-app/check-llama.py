#!/usr/bin/env python3
"""The Windows llama-server starts, loads a model and answers, as the shell runs it (stdlib only).

    python -I tools/win-app/check-llama.py --server server/llama-server.exe --build b11669 \
        --model stories260K.gguf --report report/llama-check.txt [--log report/llama-server.log]

For the windows-llama CI job (docs/plans/2026-10-24-windows-app-w4-package.md, Task 1), on the
folder tools/win-app/fetch-llama.sh lays out (helpers\\llama\\ in the package):
  1. `llama-server --version` names the pinned build;
  2. the server started with the shell's own arguments (apps/windows/Midgard.Core/ModelServer.cs:
     the alias, loopback, an 8192-token context, one slot, the sampling and JSON grammar, no
     GPU layers; two threads) on a tiny test model, ready on /health within 120 s;
  3. /props reports the alias and one slot;
  4. a streamed /v1/completions, the call mimir-server's sidecar client makes, returns text
     pieces, the first one opening a JSON object (the grammar applied), then `[DONE]`;
  5. the server log names the CPU backend ggml loaded (ggml-cpu-<variant>.dll), so a missing
     DLL or one this CPU cannot run shows here.
The model is a test model only (never shipped). The server is stopped at the end, pass or
fail; its output goes to --log. Prints the report and writes it to --report; exit 1 on failure.
"""
import argparse
import json
import re
import socket
import subprocess
import sys
import time
import urllib.request

ALIAS = "granite-3.3-2b-instruct-q4km"  # ModelServer.Alias


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def get(url, timeout=10):
    with urllib.request.urlopen(url, timeout=timeout) as r:
        return r.status, r.read()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--server", required=True)
    ap.add_argument("--build", required=True, help="the lock's build, b<number>")
    ap.add_argument("--model", required=True)
    ap.add_argument("--report", required=True)
    ap.add_argument("--log", default=None)
    a = ap.parse_args()

    lines, failed = [], False

    def note(ok, what):
        nonlocal failed
        failed |= not ok
        line = f"{'ok  ' if ok else 'FAIL'} {what}"
        lines.append(line)
        print(line, flush=True)

    # 1. --version
    v = subprocess.run([a.server, "--version"], capture_output=True, text=True, timeout=60)
    version = (v.stdout + v.stderr).strip()
    number = a.build.lstrip("b")
    note(v.returncode == 0 and f"(build {number}," in version,
         f"--version (exit {v.returncode}): {' | '.join(version.splitlines())}")

    # 2. started as the shell starts it
    port = free_port()
    args = [a.server, "-m", a.model, "--alias", ALIAS, "--host", "127.0.0.1", "--port", str(port),
            "--ctx-size", "8192", "--parallel", "1", "--temp", "0.2", "--json-schema", '{"type":"object"}',
            "--n-gpu-layers", "0", "--threads", "2"]
    log = open(a.log, "w", encoding="utf-8", errors="replace") if a.log else subprocess.DEVNULL
    proc = subprocess.Popen(args, stdout=log, stderr=subprocess.STDOUT)
    base = f"http://127.0.0.1:{port}"
    try:
        start, ready = time.monotonic(), False
        while time.monotonic() - start < 120 and proc.poll() is None:
            try:
                ready = get(base + "/health", timeout=2)[0] == 200
            except OSError:
                ready = False
            if ready:
                break
            time.sleep(0.25)
        took = time.monotonic() - start
        note(ready, f"ready on /health in {took:.1f} s" if ready
             else f"not ready on /health (exit code {proc.poll()}) after {took:.1f} s")

        if ready:
            # 3. /props
            props = json.loads(get(base + "/props")[1])
            note(props.get("model_alias") == ALIAS and props.get("total_slots") == 1,
                 f"/props: alias {props.get('model_alias')}, slots {props.get('total_slots')}, "
                 f"n_ctx {props.get('default_generation_settings', {}).get('n_ctx')}")

            # 4. a streamed completion
            body = json.dumps({"model": ALIAS, "prompt": "Once upon a time", "max_tokens": 16,
                               "stream": True}).encode()
            req = urllib.request.Request(base + "/v1/completions", data=body,
                                         headers={"Content-Type": "application/json"})
            pieces, done = [], False
            t0 = time.monotonic()
            with urllib.request.urlopen(req, timeout=120) as r:
                for raw in r:
                    line = raw.decode("utf-8", "replace").strip()
                    if not line.startswith("data:"):
                        continue
                    data = line[5:].strip()
                    if data == "[DONE]":
                        done = True
                        break
                    for c in json.loads(data).get("choices", []):
                        if c.get("text"):
                            pieces.append(c["text"])
            text = "".join(pieces)
            note(done and len(pieces) > 0 and text.lstrip().startswith("{"),
                 f"/v1/completions streamed {len(pieces)} pieces in {time.monotonic() - t0:.1f} s, "
                 f"[DONE] {'seen' if done else 'missing'}: {text!r}")
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=20)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
        if log is not subprocess.DEVNULL:
            log.close()

    # 5. the CPU backend
    if a.log:
        text = open(a.log, encoding="utf-8", errors="replace").read()
        m = re.findall(r"loaded CPU backend from ([^\r\n]+)", text)
        note(bool(m), f"CPU backend: {m[-1] if m else 'none named in the log'}")

    result = "FAIL" if failed else "PASS"
    lines.append(f"{result}: llama-server {a.build} on Windows")
    print(lines[-1])
    with open(a.report, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
