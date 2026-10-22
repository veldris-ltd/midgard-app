#!/usr/bin/env bash
# Start the live feeds runner in the background (log: feeds.log, pid: feeds.pid).
cd "$(dirname "$0")"
if [ -f feeds.pid ] && kill -0 "$(cat feeds.pid)" 2>/dev/null; then
  echo "already running (pid $(cat feeds.pid))"; exit 0
fi
nohup ../.venv/bin/python feeds.py >> feeds.out 2>&1 &
echo $! > feeds.pid
echo "started pid $(cat feeds.pid)"
