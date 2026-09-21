#!/usr/bin/env bash
# Container health check: GET a path on a local port and succeed only on HTTP 200.
# Plain bash, so the runtime image needs no HTTP client. [RC-33]
#
#   midgard-ready <port> <path>
set -euo pipefail
exec 3<>"/dev/tcp/127.0.0.1/$1"
printf 'GET %s HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n' "$2" >&3
IFS= read -r status <&3
case "$status" in
    "HTTP/1.1 200 "*) exit 0 ;;
    *) echo "not ready: $status" >&2; exit 1 ;;
esac
