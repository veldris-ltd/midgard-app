#!/usr/bin/env bash
# Tests for the web console's nginx configuration: every location serves the full set
# of security headers from the one shared file. [VLD-ANB-MIDGARD-0085] [AC-13.5]
#
#   bash tools/ci/tests/console-headers.test.sh
#
# The configuration is rendered as console.Dockerfile renders it, with a dummy inline
# script hash and gateway origin. nginx drops a server block's add_header directives
# in a location that adds its own, so the claim under test is that each location that
# adds a header includes the shared file again. With nginx on PATH the rendered
# configuration is also checked with `nginx -t`.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
installer="$here/../../../deploy/installer"
register="$here/../../../docs/compliance/RESIDUAL_RISK_REGISTER.md"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# Render: the placeholders console.Dockerfile fills, and the include path pointed at
# the scratch copy.
headers="$work/console.security-headers.conf"
server="$work/default.conf"
sed -e "s|__INLINE_SCRIPTS__|'sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA='|g" \
    -e "s|__GATEWAY_ORIGIN__|https://edge.example|g" \
    "$installer/console.security-headers.conf" > "$headers"
sed -e "s|/etc/nginx/console.security-headers.conf|$headers|g" \
    "$installer/console.nginx.conf" > "$server"

# 1. The rendered files keep no placeholder.
if grep -q "__[A-Z_]*__" "$headers" "$server"; then
    fail "a placeholder survives rendering: $(grep -h "__[A-Z_]*__" "$headers" "$server")"
else
    pass "every placeholder is filled"
fi

# 2. The shared file carries every header, and the policy the finding asks for.
csp="$(grep -h "Content-Security-Policy" "$headers" || true)"
case "$csp" in
    *"form-action 'self'"*) pass "the policy restricts form-action to 'self'" ;;
    *) fail "the policy has no form-action 'self': $csp" ;;
esac
case "$csp" in
    *"frame-ancestors 'none'"*"always;") pass "the policy keeps frame-ancestors 'none' on every status" ;;
    *) fail "the policy lost frame-ancestors 'none' or 'always': $csp" ;;
esac
for header in Content-Security-Policy Permissions-Policy X-Content-Type-Options \
    Referrer-Policy X-Frame-Options Cross-Origin-Opener-Policy; do
    if grep -Eq "^add_header $header \".+\" always;" "$headers"; then
        pass "the shared file sets $header"
    else
        fail "the shared file does not set $header"
    fi
done
if grep -q 'Permissions-Policy "camera=(), microphone=(), geolocation=(), payment=(), usb=()"' "$headers"; then
    pass "Permissions-Policy denies camera, microphone, geolocation, payment and usb"
else
    fail "Permissions-Policy does not deny every feature"
fi

# 3. The server block and every location that adds a header of its own include the
#    shared file; no location sets a security header by hand.
report="$(awk -v inc="include $headers;" '
    /^[[:space:]]*server[[:space:]]*\{/ { block = "server"; adds[block] = 0; incs[block] = 0; next }
    /^[[:space:]]*location[[:space:]]/ { block = $2; adds[block] = 0; incs[block] = 0; next }
    /^[[:space:]]*\}/ { if (block != "server") block = "server"; next }
    { line = $0; sub(/^[[:space:]]+/, "", line) }
    line ~ /^add_header / { adds[block]++ }
    line == inc { incs[block]++ }
    END { for (b in adds) printf "%s %d %d\n", b, adds[b], incs[b] }
' "$server")"
while read -r block added included; do
    [ -n "$block" ] || continue
    if [ "$block" = "server" ] || [ "$added" -gt 0 ]; then
        if [ "$included" -ge 1 ]; then
            pass "$block includes the shared security headers"
        else
            fail "$block ($added header(s) of its own) does not include the shared security headers"
        fi
    fi
done <<< "$report"
grep -q "location /assets/" "$server" && echo "$report" | grep -q "^/assets/ [1-9][0-9]* [1-9]" \
    && pass "/assets/ sets Cache-Control and still serves the shared headers" \
    || fail "/assets/ must include the shared headers alongside its Cache-Control"
if grep -Eq "add_header (Content-Security-Policy|Permissions-Policy|Referrer-Policy|X-Frame-Options|Cross-Origin-Opener-Policy|X-Content-Type-Options)" "$server"; then
    fail "console.nginx.conf sets a security header by hand rather than through the shared file"
else
    pass "console.nginx.conf sets no security header by hand"
fi

# 4. style-src drops 'unsafe-inline', or the register records why it stays.
case "$csp" in
    *"style-src 'self' 'unsafe-inline'"*)
        if grep -q "unsafe-inline" "$register"; then
            pass "style-src keeps 'unsafe-inline' and the residual risk register records it"
        else
            fail "style-src keeps 'unsafe-inline' with no residual risk register entry"
        fi
        ;;
    *) pass "style-src does not admit 'unsafe-inline'" ;;
esac

# 5. nginx itself accepts the rendered configuration, when it is here to ask.
if command -v nginx >/dev/null 2>&1; then
    mkdir -p "$work/logs"
    cat > "$work/nginx.conf" <<CONF
pid $work/nginx.pid;
error_log $work/logs/error.log;
events {}
http {
    access_log off;
    include $server;
}
CONF
    if nginx -t -q -p "$work" -c "$work/nginx.conf" > "$work/nginx.out" 2>&1; then
        pass "nginx -t accepts the rendered configuration"
    else
        fail "nginx -t refuses the rendered configuration: $(cat "$work/nginx.out")"
    fi
else
    echo "skip  nginx is not on PATH; the grep checks above stand in for nginx -t"
fi

if [ "$failures" -gt 0 ]; then
    echo "$failures failure(s)"
    exit 1
fi
echo "all console-headers tests passed"
