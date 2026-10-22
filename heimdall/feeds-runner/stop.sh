#!/usr/bin/env bash
# Stop the live feeds runner started by start.sh.
cd "$(dirname "$0")"
if [ ! -f feeds.pid ]; then echo "not running (no feeds.pid)"; exit 0; fi
pid=$(cat feeds.pid)
if kill -0 "$pid" 2>/dev/null; then
  kill "$pid"
  for _ in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || break; sleep 0.5; done
  kill -0 "$pid" 2>/dev/null && kill -9 "$pid"
  echo "stopped pid $pid"
else
  echo "pid $pid was not running"
fi
rm -f feeds.pid
