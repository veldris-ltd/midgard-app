#!/usr/bin/env bash
# Tests for the host firewall rule mg-install adds so the gateway's edge network has no
# route out of the server. [VLD-ANB-MIDGARD-0061] [NFR-SEC-1]
#
#   bash tools/ci/tests/edge-egress.test.sh
#
# The claims under test: compose gives the edge bridge the fixed interface name the
# rule targets; the installer puts a RETURN for replies and then a DROP for anything
# forwarded from that bridge to another interface at the top of DOCKER-USER, tagged so
# it can be found; applying twice adds them once; uninstalling removes them, the unit
# and the helper, and no other rule; and a server without iptables or without the
# DOCKER-USER chain fails the step rather than going on unprotected.
#
# Nothing here needs root or touches the real firewall: iptables, ip6tables and
# systemctl are recording fakes on PATH, and the functions are taken out of the
# installer rather than the installer run.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
root="$here/../../.."
compose_dir="$root/deploy/compose"
installer="$root/deploy/installer/mg-install"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

# ---- the fakes -------------------------------------------------------------------------
# A fake iptables that keeps each chain as a file of rule lines in $FAKE_FW/<name>/,
# where <name> is the command it was called as. It understands what the installer
# uses: -n -L CHAIN, -S CHAIN, -C CHAIN RULE, -I CHAIN N RULE and -D CHAIN RULE. Every
# call is appended to $FAKE_FW/calls.
bin="$work/bin"
mkdir -p "$bin"
cat >"$bin/iptables" <<'FAKE'
#!/usr/bin/env bash
set -u
dir="$FAKE_FW/$(basename "$0")"
echo "$(basename "$0") $*" >>"$FAKE_FW/calls"
mkdir -p "$dir"
case "$1" in
    -n) [ "$2" = -L ] && [ -f "$dir/$3" ] ;;
    -L) [ -f "$dir/$2" ] ;;
    -S)
        [ -f "$dir/$2" ] || exit 1
        echo "-N $2"
        sed "s/^/-A $2 /" "$dir/$2"
        ;;
    -C)
        chain="$2"; shift 2
        [ -f "$dir/$chain" ] && grep -qxF -- "$*" "$dir/$chain"
        ;;
    -I)
        chain="$2" pos="$3"; shift 3
        [ -f "$dir/$chain" ] || exit 1
        { head -n $((pos - 1)) "$dir/$chain"; echo "$*"; tail -n +"$pos" "$dir/$chain"; } >"$dir/.new"
        mv "$dir/.new" "$dir/$chain"
        ;;
    -D)
        chain="$2"; shift 2
        # iptables-legacy lists a comment in quotes; the rule is the same unquoted.
        line="$(tr -d '"' <"$dir/$chain" | grep -nxF -- "$*" | head -1 | cut -d: -f1)"
        [ -n "$line" ] || exit 1
        sed -i "${line}d" "$dir/$chain"
        ;;
    *) exit 2 ;;
esac
FAKE
cp "$bin/iptables" "$bin/ip6tables"
cat >"$bin/systemctl" <<'FAKE'
#!/usr/bin/env bash
echo "systemctl $*" >>"$FAKE_FW/calls"
FAKE
chmod +x "$bin/iptables" "$bin/ip6tables" "$bin/systemctl"
mkdir -p "$work/empty"   # a PATH with nothing on it

# fresh [CHAINS...]: an empty firewall where iptables (and ip6tables) have DOCKER-USER
# with one rule of the site's own, which nothing here may touch
site_rule='-s 192.0.2.0/24 -j DROP'
fresh() {
    rm -rf "$work/fw" "$work/etc"
    mkdir -p "$work/fw" "$work/etc"
    local t
    for t in "$@"; do
        mkdir -p "$work/fw/$t"
        echo "$site_rule" >"$work/fw/$t/DOCKER-USER"
    done
    : >"$work/fw/calls"
}
export FAKE_FW="$work/fw"

# ---- the installer's functions ---------------------------------------------------------
fns="$(tr -d '\r' <"$installer" | sed -n \
    -e '/^remove_edge_egress_rules() {/,/^}/p' -e '/^apply_edge_egress() {/,/^}/p' \
    -e '/^install_edge_egress() {/,/^}/p' -e '/^remove_edge_egress() {/,/^}/p')"
eval "$fns"
for f in remove_edge_egress_rules apply_edge_egress install_edge_egress remove_edge_egress; do
    if ! declare -F "$f" >/dev/null; then
        fail "mg-install defines $f"
        echo "$failures failure(s)"
        exit 1
    fi
done
# The installer's own values, with the files it writes moved under $work.
eval "$(tr -d '\r' <"$installer" | grep -E '^EDGE_(BRIDGE|RULE_TAG)=')"
EDGE_HELPER="$work/etc/midgard-edge-egress"
EDGE_UNIT="$work/etc/midgard-edge-egress.service"
if [ "${EDGE_BRIDGE:-}" = mg-edge0 ] && [ "${EDGE_RULE_TAG:-}" = midgard-edge-egress ]; then
    pass "the installer's rules target mg-edge0 and are tagged midgard-edge-egress"
else
    fail "the installer's rules target mg-edge0 and are tagged midgard-edge-egress: got '${EDGE_BRIDGE:-}' '${EDGE_RULE_TAG:-}'"
fi
if grep -q 'run "[^"]*" install_edge_egress' <(tr -d '\r' <"$installer") &&
    tr -d '\r' <"$installer" | sed -n '/^install_now() {/,/^}/p' |
    awk '/install_edge_egress/ {f=NR} /dc up -d --wait/ {u=NR} /dc run --rm init/ {i=NR} END {exit !(f && f < i && f < u)}'; then
    pass "the install runs the firewall step before it creates or starts the enclosure"
else
    fail "the install runs the firewall step before it creates or starts the enclosure"
fi
if tr -d '\r' <"$installer" | sed -n '/^cmd_uninstall() {/,/^}/p' | grep -q 'remove_edge_egress'; then
    pass "uninstall removes the firewall rule"
else
    fail "uninstall removes the firewall rule"
fi

keep="-i mg-edge0 -m conntrack --ctstate RELATED,ESTABLISHED -m comment --comment midgard-edge-egress -j RETURN"
drop="-i mg-edge0 ! -o mg-edge0 -m comment --comment midgard-edge-egress -j DROP"
expected="$(printf '%s\n%s\n%s' "$keep" "$drop" "$site_rule")"

# ---- applying --------------------------------------------------------------------------
fresh iptables ip6tables
if (PATH="$bin:$PATH" install_edge_egress) >"$work/out" 2>&1; then
    pass "install_edge_egress succeeds with iptables and a DOCKER-USER chain"
else
    fail "install_edge_egress succeeds with iptables and a DOCKER-USER chain: $(tr '\n' ' ' <"$work/out")"
fi
if [ "$(cat "$work/fw/iptables/DOCKER-USER")" = "$expected" ]; then
    pass "DOCKER-USER: replies return, then anything from mg-edge0 to another interface drops, above the site's rules"
else
    fail "DOCKER-USER holds the rules in order: got $(tr '\n' '|' <"$work/fw/iptables/DOCKER-USER")"
fi
if [ "$(cat "$work/fw/ip6tables/DOCKER-USER")" = "$expected" ]; then
    pass "the same rules are in ip6tables where Docker manages it"
else
    fail "the same rules are in ip6tables: got $(tr '\n' '|' <"$work/fw/ip6tables/DOCKER-USER")"
fi
if grep -qxF "iptables -I DOCKER-USER 1 $drop" "$work/fw/calls" &&
    grep -qxF "iptables -I DOCKER-USER 1 $keep" "$work/fw/calls"; then
    pass "the rules are inserted at the top of DOCKER-USER"
else
    fail "the rules are inserted at the top of DOCKER-USER"
fi
if grep -qx 'systemctl daemon-reload' "$work/fw/calls" &&
    grep -qx 'systemctl enable midgard-edge-egress.service' "$work/fw/calls"; then
    pass "the unit is loaded and enabled"
else
    fail "the unit is loaded and enabled: $(grep systemctl "$work/fw/calls" | tr '\n' ';')"
fi
unit="$(cat "$EDGE_UNIT" 2>/dev/null)"
if grep -qx 'After=docker.service' <<<"$unit" && grep -qx 'PartOf=docker.service' <<<"$unit" &&
    grep -qx 'Type=oneshot' <<<"$unit" && grep -qx "ExecStart=$EDGE_HELPER" <<<"$unit" &&
    grep -qx 'WantedBy=multi-user.target' <<<"$unit"; then
    pass "the unit is a oneshot after and part of docker.service, wanted at boot, running the helper"
else
    fail "the unit is a oneshot after and part of docker.service, wanted at boot: $(tr '\n' '|' <<<"$unit")"
fi

# The helper the unit runs stands alone and is idempotent.
if [ -x "$EDGE_HELPER" ] && bash -n "$EDGE_HELPER" &&
    PATH="$bin:$PATH" bash "$EDGE_HELPER" >"$work/out" 2>&1 &&
    [ "$(cat "$work/fw/iptables/DOCKER-USER")" = "$expected" ]; then
    pass "the unit's helper runs on its own and leaves the rules as they were"
else
    fail "the unit's helper runs on its own and leaves the rules as they were: $(tr '\n' ' ' <"$work/out")"
fi

: >"$work/fw/calls"
(PATH="$bin:$PATH" apply_edge_egress) >"$work/out" 2>&1
if [ "$(cat "$work/fw/iptables/DOCKER-USER")" = "$expected" ] && ! grep -q -- ' -I ' "$work/fw/calls"; then
    pass "applying again adds nothing"
else
    fail "applying again adds nothing: $(tr '\n' '|' <"$work/fw/iptables/DOCKER-USER")"
fi

# One rule lost (say, deleted by hand): applying restores both, once each.
sed -i '2d' "$work/fw/iptables/DOCKER-USER"
(PATH="$bin:$PATH" apply_edge_egress) >"$work/out" 2>&1
if [ "$(cat "$work/fw/iptables/DOCKER-USER")" = "$expected" ]; then
    pass "a half-present set is replaced by the whole set, once"
else
    fail "a half-present set is replaced by the whole set, once: $(tr '\n' '|' <"$work/fw/iptables/DOCKER-USER")"
fi

# No IPv6 chain: iptables is still done, ip6tables skipped.
fresh iptables
if (PATH="$bin:$PATH" apply_edge_egress) >"$work/out" 2>&1 &&
    [ "$(cat "$work/fw/iptables/DOCKER-USER")" = "$expected" ] &&
    ! grep -q '^ip6tables -I' "$work/fw/calls"; then
    pass "without an IPv6 DOCKER-USER chain only iptables is changed"
else
    fail "without an IPv6 DOCKER-USER chain only iptables is changed: $(tr '\n' ' ' <"$work/out")"
fi

# ---- uninstalling ----------------------------------------------------------------------
fresh iptables ip6tables
(PATH="$bin:$PATH" install_edge_egress) >/dev/null 2>&1
: >"$work/fw/calls"
if (PATH="$bin:$PATH" remove_edge_egress) >"$work/out" 2>&1 &&
    [ "$(cat "$work/fw/iptables/DOCKER-USER")" = "$site_rule" ] &&
    [ "$(cat "$work/fw/ip6tables/DOCKER-USER")" = "$site_rule" ]; then
    pass "uninstalling removes exactly the tagged rules, in both, and leaves the site's"
else
    fail "uninstalling removes exactly the tagged rules: $(tr '\n' '|' <"$work/fw/iptables/DOCKER-USER") $(tr '\n' ' ' <"$work/out")"
fi
if [ ! -e "$EDGE_UNIT" ] && [ ! -e "$EDGE_HELPER" ] &&
    grep -qx 'systemctl disable midgard-edge-egress.service' "$work/fw/calls"; then
    pass "uninstalling disables and removes the unit and its helper"
else
    fail "uninstalling disables and removes the unit and its helper"
fi
# iptables-legacy quotes the comment in its listing; the rules are still found.
fresh iptables
printf '%s\n%s\n' "${keep/--comment midgard-edge-egress/--comment \"midgard-edge-egress\"}" "$site_rule" \
    >"$work/fw/iptables/DOCKER-USER"
(PATH="$bin:$PATH" remove_edge_egress_rules iptables) >/dev/null 2>&1
if [ "$(cat "$work/fw/iptables/DOCKER-USER")" = "$site_rule" ] &&
    grep -qxF -- "iptables -D DOCKER-USER $keep" "$work/fw/calls"; then
    pass "a comment listed in quotes is found and deleted"
else
    fail "a comment listed in quotes is found and deleted: $(tr '
' '|' <"$work/fw/iptables/DOCKER-USER")"
fi
if (PATH="$work/empty" remove_edge_egress) >/dev/null 2>&1; then
    pass "uninstalling without iptables does not fail"
else
    fail "uninstalling without iptables does not fail"
fi

# ---- failing closed --------------------------------------------------------------------
fresh
if (PATH="$work/empty" apply_edge_egress) >"$work/out" 2>&1; then
    fail "without iptables the step fails"
elif grep -q 'iptables is not installed' "$work/out"; then
    pass "without iptables the step fails and says why"
else
    fail "without iptables the step fails and says why: $(tr '\n' ' ' <"$work/out")"
fi
fresh
if (PATH="$bin:$PATH" install_edge_egress) >"$work/out" 2>&1; then
    fail "without a DOCKER-USER chain the step fails"
elif grep -q 'no DOCKER-USER chain' "$work/out" && [ ! -e "$EDGE_UNIT" ]; then
    pass "without a DOCKER-USER chain the step fails, says why, and installs no unit"
else
    fail "without a DOCKER-USER chain the step fails and says why: $(tr '\n' ' ' <"$work/out")"
fi

# ---- what compose renders --------------------------------------------------------------
if ! command -v docker >/dev/null 2>&1 || ! docker compose version >/dev/null 2>&1; then
    echo "skip  the compose check: docker compose is not installed here"
elif ! command -v python3 >/dev/null 2>&1; then
    fail "python3 is installed (the rendered configuration is read with it)"
else
    for files in "-f $compose_dir/compose.yaml" \
        "-f $compose_dir/compose.yaml -f $compose_dir/compose.granite.yaml"; do
        # shellcheck disable=SC2086  # the file arguments split on purpose
        name="$(env -u MIDGARD_EDGE_BRIDGE docker compose $files config --format json 2>/dev/null |
            python3 -I -c 'import json,sys; print(json.load(sys.stdin)["networks"]["edge"]["driver_opts"]["com.docker.network.bridge.name"])' 2>/dev/null)"
        if [ "$name" = "$EDGE_BRIDGE" ] && [ "${#name}" -le 15 ]; then
            pass "compose ($(basename "${files##* }")) names the edge bridge $EDGE_BRIDGE, within Linux's 15 characters"
        else
            fail "compose ($(basename "${files##* }")) names the edge bridge $EDGE_BRIDGE: got '$name'"
        fi
    done
fi

if [ "$failures" -eq 0 ]; then
    echo "edge-egress: PASS"
    exit 0
fi
echo "$failures failure(s)"
exit 1
