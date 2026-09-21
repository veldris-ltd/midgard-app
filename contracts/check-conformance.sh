#!/usr/bin/env bash
# RC-36 conformance: every RPC the client SDK can call (`GatewayClient`) MUST have a
# governing proto contract. This runs OFFLINE (no buf/network) so the completeness
# guarantee is enforced in the air-gapped gate too; `buf breaking` (CI) separately
# gates changes to those contracts. Fails closed on any uncovered client RPC. [RC-36]
set -euo pipefail
cd "$(dirname "$0")/.."

PROTO_DIR="contracts/proto"
CLIENT="sdk/ts/src/client.ts"
# Client-side methods that are NOT server RPCs (session-local state).
CLIENT_SIDE="policy"

# Every RPC name defined across the protos (ObjectFabric + Gateway).
rpcs="$(grep -rhoE 'rpc [A-Za-z0-9]+' "$PROTO_DIR" | awk '{print $2}' | sort -u)"

# Every method on the GatewayClient interface (a 2-space-indented `name(`).
methods="$(grep -oE '^  [a-zA-Z][a-zA-Z0-9]*\(' "$CLIENT" | sed -E 's/^  //; s/\($//' | sort -u)"

missing=0
for m in $methods; do
  case " $CLIENT_SIDE " in *" $m "*) continue ;; esac
  # PascalCase the method to the RPC name (query -> Query, listKeys -> ListKeys).
  rpc="$(printf '%s' "${m:0:1}" | tr '[:lower:]' '[:upper:]')${m:1}"
  if ! grep -qx "$rpc" <<<"$rpcs"; then
    echo "MISSING: GatewayClient.$m() has no governing RPC '$rpc' in $PROTO_DIR/**" >&2
    missing=1
  fi
done

if [ "$missing" != 0 ]; then
  echo "RC-36 FAIL: a client RPC lacks a governing contract. Add it to contracts/proto/midgard/v1/, or (if it is session-local) to CLIENT_SIDE in this script." >&2
  exit 1
fi
echo "RC-36 OK: every GatewayClient RPC has a governing proto contract ($(wc -w <<<"$rpcs") RPCs defined)."
