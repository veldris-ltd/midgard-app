#!/usr/bin/env bash
# The scheduled advisory refresh proposes a change when, and only when, a vendored
# snapshot moved, and the proposal carries the scan diff. [AC-P4.5] [NFR-OSS-1]
# (VLD-ANB-MIDGARD-0076)
#
#   bash tools/ci/tests/advisory-refresh.test.sh
#
# run.sh, lib.sh and in-toolchain.sh are copied into a scratch repository with a bare
# `origin`. just, gh, curl, unzip, docker and cargo are stubs on PATH, and git is a
# wrapper that records every call and hands everything but `push` to the real git.
# The `just` stub moves the snapshots only when STUB_REFRESH says so, and makes the
# scan report a new finding once they have moved.
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
src="$here/../../.."
failures=0
real_git="$(command -v git)"

pass() { echo "ok    $1"; }
fail() {
    echo "FAIL  $1"
    failures=$((failures + 1))
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# --- the stubs ---------------------------------------------------------------------
bin="$tmp/bin"
mkdir -p "$bin"
cat > "$bin/just" <<'EOF'
#!/usr/bin/env bash
echo "just $*" >> "$STUB_LOG"
case "$1" in
    advisory-db-refresh)
        [ "${STUB_REFRESH:-none}" = none ] || sed -i 's/^date = .*/date = "2026-11-02"/' vendor/advisory-db/snapshot.toml ;;
    advisory-db-npm-refresh)
        [ "${STUB_REFRESH:-none}" = both ] && sed -i 's/^advisory_count = .*/advisory_count = 93/' vendor/advisory-db-npm/snapshot.toml ;;
    sbom) ;;
    scan-advisory)
        if grep -q 2026-11-02 vendor/advisory-db/snapshot.toml; then
            echo "note   vuln@1.0.0  advisory-medium  RUSTSEC-2026-9999"
            echo "mg-scan: 1 finding(s), 0 blocking a release"
        else
            echo "mg-scan: 0 finding(s), 0 blocking a release"
        fi ;;
esac
exit 0
EOF
cat > "$bin/gh" <<'EOF'
#!/usr/bin/env bash
echo "gh $*" >> "$STUB_LOG"
while [ "$#" -gt 0 ]; do
    [ "$1" = --body-file ] && cp "$2" "$STUB_BODY"
    shift
done
EOF
cat > "$bin/curl" <<'EOF'
#!/usr/bin/env bash
echo "curl $*" >> "$STUB_LOG"
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) : > "$2"; shift ;;
        --data-urlencode) case "$2" in description@*) cp "${2#description@}" "$STUB_BODY" ;; esac; shift ;;
    esac
    shift
done
EOF
cat > "$bin/unzip" <<'EOF'
#!/usr/bin/env bash
echo "unzip $*" >> "$STUB_LOG"
EOF
cat > "$bin/cargo" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
# in-toolchain.sh runs `docker run ... IMAGE COMMAND...`: run COMMAND here instead.
cat > "$bin/docker" <<'EOF'
#!/usr/bin/env bash
[ "$1" = run ] || exit 0
while [ "$#" -gt 0 ] && [ "$1" != "$TOOLCHAIN_IMAGE" ]; do shift; done
shift
exec "$@"
EOF
cat > "$bin/git" <<EOF
#!/usr/bin/env bash
echo "git \$*" >> "\$STUB_LOG"
for a in "\$@"; do
    case "\$a" in
        push) exit 0 ;;
        clone) mkdir -p "\${@: -1}"; exit 0 ;;
    esac
done
exec "$real_git" "\$@"
EOF
chmod +x "$bin"/*

# --- a scratch repository ----------------------------------------------------------
setup() {
    local root="$tmp/repo"
    rm -rf "$root" "$tmp/origin.git"
    mkdir -p "$root/tools/ci" "$root/vendor/advisory-db" "$root/vendor/advisory-db-npm"
    cp "$src/tools/ci/run.sh" "$src/tools/ci/lib.sh" "$src/tools/ci/in-toolchain.sh" "$root/tools/ci/"
    printf '#!/usr/bin/env bash\necho "ui-bundle midgard-console-test products/test/ui test.ui.tar @midgard/test"\n' \
        > "$root/tools/ci/js-artefacts.sh"
    printf '[snapshot]\ndate = "2026-10-30"\nadvisory_count = 1375\n' > "$root/vendor/advisory-db/snapshot.toml"
    printf '[snapshot]\ndate = "2026-11-01"\nadvisory_count = 92\n' > "$root/vendor/advisory-db-npm/snapshot.toml"
    "$real_git" init -q -b dev "$root"
    "$real_git" -C "$root" -c core.autocrlf=false -c user.name=t -c user.email=t@example.invalid add -A
    "$real_git" -C "$root" -c core.autocrlf=false -c user.name=t -c user.email=t@example.invalid commit -q -m init
    echo "$root"
}

# run FORGE REFRESH: the job, in a fresh repository, on FORGE (github, gitlab, none)
# with the stubbed refresh moving REFRESH (none, rustsec, both). Sets $out and $code.
run() {
    local root
    root="$(setup)"
    : > "$tmp/log"
    rm -f "$tmp/body"
    out="$(cd "$root" && env -i PATH="$bin:$PATH" HOME="$tmp" STUB_LOG="$tmp/log" STUB_BODY="$tmp/body" \
        STUB_REFRESH="$2" TOOLCHAIN_IMAGE=stub-toolchain \
        $(case "$1" in
            github) echo GITHUB_ACTIONS=true GITHUB_REPOSITORY=veldris/midgard GITHUB_REF_NAME=dev GH_TOKEN=gh-test-token ;;
            gitlab) echo GITLAB_CI=true CI_SERVER_URL=https://git.example.invalid CI_PROJECT_PATH=veldris/midgard \
                         CI_API_V4_URL=https://git.example.invalid/api/v4 CI_PROJECT_ID=7 CI_COMMIT_BRANCH=dev \
                         MIDGARD_MR_TOKEN=mr-test-token ;;
        esac) \
        bash tools/ci/run.sh advisory-refresh 2>&1)"
    code=$?
    repo="$root"
}

today="$(date -u +%F)"
branch="deps/advisory-refresh-$today"

# 1. Nothing moved: no branch, no commit, no push, no pull request.
run github none
if [ "$code" -eq 0 ] && ! grep -Eq '^git (checkout|commit|push)|^gh ' "$tmp/log"; then
    pass "nothing changed: no change is opened"
else
    fail "nothing changed, yet the job did more than look (exit $code): $(grep -E '^(git|gh)' "$tmp/log" | tr '\n' ';') $out"
fi
grep -q "already current" <<<"$out" && pass "nothing changed: the job says so" \
    || fail "nothing changed, and the job did not say so: $out"
grep -q '^just advisory-db-refresh target/advisory-refresh/rustsec$' "$tmp/log" \
    && grep -q '^just advisory-db-npm-refresh target/advisory-refresh/osv$' "$tmp/log" \
    && pass "both refresh recipes ran, from what the runner fetched" \
    || fail "the refresh recipes did not run as expected: $(grep '^just' "$tmp/log" | tr '\n' ';')"

# 2. The RustSec snapshot moved: a branch, one commit, a push and a pull request.
run github rustsec
if [ "$code" -eq 0 ] && grep -q "^gh pr create .*--base dev --head $branch" "$tmp/log"; then
    pass "a snapshot changed: a pull request is opened from $branch"
else
    fail "a snapshot changed, but no pull request was opened (exit $code): $out"
fi
grep -q "^git .*push -q --force https://github.com/veldris/midgard.git HEAD:refs/heads/$branch" "$tmp/log" \
    && pass "the branch is pushed to the repository" || fail "the branch was not pushed: $(grep push "$tmp/log")"
msg="$("$real_git" -C "$repo" log -1 --format=%s 2>/dev/null)"
[ "$msg" = "chore(advisories): refresh snapshots $today" ] && pass "the commit message is the house one" \
    || fail "the commit says '$msg'"
changed="$("$real_git" -C "$repo" show --name-only --format= HEAD 2>/dev/null | tr '\n' ' ')"
[ "$changed" = "vendor/advisory-db/snapshot.toml " ] && pass "the commit holds the snapshot and nothing else" \
    || fail "the commit holds: $changed"
if [ -f "$tmp/body" ] && grep -q '^+note   vuln@1.0.0  advisory-medium  RUSTSEC-2026-9999' "$tmp/body"; then
    pass "the pull request body carries the scan diff"
else
    fail "the pull request body has no scan diff: $(cat "$tmp/body" 2>/dev/null)"
fi
grep -q 'gh-test-token' "$tmp/log" && fail "the token reached a command line" \
    || pass "the token never reaches a command line"

# 3. On GitLab: a merge request through the API, with the project access token.
run gitlab both
if [ "$code" -eq 0 ] && grep -q "^curl .*source_branch=$branch.*/api/v4/projects/7/merge_requests" "$tmp/log"; then
    pass "on GitLab, a merge request is opened through the API"
else
    fail "on GitLab, no merge request was opened (exit $code): $out"
fi
grep -q 'mr-test-token' "$tmp/log" && fail "the GitLab token reached a command line" \
    || pass "the GitLab token never reaches a command line"
changed="$("$real_git" -C "$repo" show --name-only --format= HEAD 2>/dev/null | sort | tr '\n' ' ')"
[ "$changed" = "vendor/advisory-db-npm/snapshot.toml vendor/advisory-db/snapshot.toml " ] \
    && pass "both snapshots are committed when both moved" || fail "the commit holds: $changed"

# 4. Outside a pipeline a change is reported, never pushed.
run none rustsec
if [ "$code" -ne 0 ] && ! grep -Eq '^git .*push|^gh ' "$tmp/log"; then
    pass "outside a pipeline, a change is reported and nothing is pushed"
else
    fail "outside a pipeline (exit $code): $(grep -E '^(git|gh)' "$tmp/log" | tr '\n' ';')"
fi

if [ "$failures" -ne 0 ]; then
    echo "advisory-refresh: $failures check(s) failed" >&2
    exit 1
fi
echo "advisory-refresh: OK"
