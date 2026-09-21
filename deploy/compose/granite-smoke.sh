#!/usr/bin/env bash
# Serve the default Granite model through compose, end to end, on throwaway state:
# stage and verify the model, start mimir-server beside llama.cpp, and make a
# certified PQ-mTLS inference call that real generated text must answer. [FR-MI-1]
# [FR-MI-8] [AC-MI-7] [NFR-PERF-1]
#
#   bash deploy/compose/granite-smoke.sh [PATH/TO/granite-3.3-2b-instruct-Q4_K_M.gguf]
#
# Runs as its own compose project, so an enclosure started with compose-up is left
# alone. Everything but the external models and signing-key volumes is removed on exit.
set -euo pipefail
export MSYS_NO_PATHCONV=1

cd "$(dirname "$0")/../.."
COMPOSE=(docker compose -p midgard-granite -f deploy/compose/compose.yaml
         -f deploy/compose/compose.granite.yaml)
cleanup() { "${COMPOSE[@]}" down -v --remove-orphans >/dev/null 2>&1 || true; }
on_exit() {
    local status=$?
    if [ "$status" -ne 0 ]; then
        echo "--- logs (exit $status)"
        "${COMPOSE[@]}" logs --no-color --tail 40 mimir-server mimir-llamacpp 2>&1 || true
    fi
    cleanup
}
trap on_exit EXIT
cleanup

echo "--- building the development image"
"${COMPOSE[@]}" build init
echo "--- staging and verifying the model"
bash deploy/compose/stage-model.sh "$@"

echo "--- starting mimir-server and llama.cpp"
started=$(date +%s)
"${COMPOSE[@]}" up -d --wait --wait-timeout 600 init mimir-server mimir-llamacpp
echo "ready after $(( $(date +%s) - started ))s (verifying 1.5 GB of weights, loading them)"
"${COMPOSE[@]}" logs mimir-server | grep '"serving"' | tail -1

echo "--- a certified inference call"
out="$("${COMPOSE[@]}" run --rm probe \
    --prompt "In one sentence, what does an append-only audit ledger protect against?" \
    --max-tokens 64)"
echo "$out"
head -1 <<<"$out" | grep -q '^model=llama-cpp/granite-3.3-2b-instruct-q4km locality=InEnclosure audit_seq=0 ' || {
    echo "granite smoke: the call was not served by Granite" >&2
    exit 1
}
[ -n "$(sed -n '2,$p' <<<"$out" | tr -d '[:space:]')" ] || {
    echo "granite smoke: the completion is empty" >&2
    exit 1
}
echo "granite smoke: PASS"
