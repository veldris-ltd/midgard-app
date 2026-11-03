#!/usr/bin/env bash
# Tests for the guards in deploy/installer/mg-install: the install folder it fills as
# root and `uninstall --purge` deletes, the Docker apt key it trusts, and the file the
# unattended install reads the first password from. The installer is sourced, not run,
# and its functions are called unprivileged; they only look, and every real folder used
# here is under a temporary one. [RC-33] [NFR-SEC-1] [AC-40.1] [AC-40.2] [AC-40.3]
# [AC-40.4] [AC-40.5] (VLD-ANB-MIDGARD-0048)
#
#   bash tools/ci/tests/mg-install.test.sh
set -uo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../.." && pwd)"
installer="$repo/deploy/installer/mg-install"
work="$(mktemp -d)"
trap 'chmod -R u+w "$work" 2>/dev/null; rm -rf "$work"' EXIT
failures=0
me="$(id -u)"

# call_in SCRIPT FUNCTION [ARG...]: source an installer in a fresh bash and call one of
# its functions. $0 is not the script, so its main does not run.
call_in() { bash -c '. "$1"; shift; "$@"' mg-install-test "$@"; }
call() { call_in "$installer" "$@"; }

pass() { echo "ok    $1"; }
fail() { echo "FAIL  $1"; failures=$((failures + 1)); }

# refuses NAME COMMAND...: COMMAND fails and says why on stderr.
refuses() {
    local name="$1" err rc
    shift
    err="$("$@" 2>&1 >/dev/null)"; rc=$?
    if [ "$rc" -ne 0 ] && [ -n "$err" ]; then pass "$name: $err"
    else fail "$name: wanted a refusal with a reason, got status $rc and '$err'"; fi
}

# accepts NAME COMMAND...: COMMAND succeeds.
accepts() {
    local name="$1" out rc
    shift
    out="$("$@" 2>&1)"; rc=$?
    if [ "$rc" -eq 0 ]; then pass "$name"
    else fail "$name: wanted success, got status $rc and '$out'"; fi
}

# expect NAME WANT COMMAND...: COMMAND's output, stdout and stderr, is WANT.
expect() {
    local name="$1" want="$2" got
    shift 2
    got="$("$@" 2>&1)"
    if [ "$got" = "$want" ]; then pass "$name"
    else fail "$name: wanted '$want', got '$got'"; fi
}

# --- the script can be sourced, and still runs as before ------------------------------ [AC-40.5]
accepts "the installer parses" bash -n "$installer"
expect "sourcing it runs nothing" "" call true
expect "--help still prints the usage" "mg-install 0.1.0: install and manage the Midgard Suite on one server." \
    bash -c 'bash "$0" --help | head -n 1' "$installer"

# --- install folders that are refused ----------------------------------------------- [AC-40.1]
for dir in / /home /usr /opt /root /tmp /var /etc /opt/ //opt; do
    refuses "validate_install_dir refuses $dir" call validate_install_dir "$dir"
done
refuses "validate_install_dir refuses an empty value" call validate_install_dir ""
refuses "validate_install_dir refuses a relative path" call validate_install_dir relative/dir
refuses "validate_install_dir refuses /opt/../" call validate_install_dir /opt/../
refuses "validate_install_dir refuses /opt/midgard/../.." call validate_install_dir /opt/midgard/../..
refuses "validate_install_dir refuses a name with a newline" call validate_install_dir $'/opt/mid\ngard'
refuses "validate_install_dir refuses the installer's own folder" call validate_install_dir "$repo/deploy/installer"
refuses "validate_install_dir refuses the repository root" call validate_install_dir "$repo"
refuses "validate_install_dir refuses a folder inside the repository" call validate_install_dir "$repo/install"

# --- install folders that are accepted, and their traps ------------------------------ [AC-40.2]
accepts "validate_install_dir accepts /opt/midgard" call validate_install_dir /opt/midgard
mkdir "$work/midgard"
accepts "validate_install_dir accepts a dedicated folder" call validate_install_dir "$work/midgard"
accepts "validate_install_dir accepts it with a trailing slash" call validate_install_dir "$work/midgard/"
ln -s "$work/midgard" "$work/link"
refuses "validate_install_dir refuses a symbolic link to it" call validate_install_dir "$work/link"
refuses "validate_install_dir refuses the link with a trailing slash" call validate_install_dir "$work/link/"
mkdir -p "$work/open/midgard"
chmod 1777 "$work/open"
refuses "validate_install_dir refuses a folder whose parent anyone can write" \
    call validate_install_dir "$work/open/midgard"
touch "$work/a-file"
refuses "validate_install_dir refuses a file" call validate_install_dir "$work/a-file"

# The installed layout: run as /usr/local/bin/mg-install, the repository is DIR/src.
mkdir -p "$work/opt/midgard/src/deploy/installer"
cp "$installer" "$work/opt/midgard/src/deploy/installer/mg-install"
accepts "the installed copy accepts its own install folder" \
    call_in "$work/opt/midgard/src/deploy/installer/mg-install" validate_install_dir "$work/opt/midgard"
refuses "the installed copy refuses its source folder" \
    call_in "$work/opt/midgard/src/deploy/installer/mg-install" validate_install_dir "$work/opt/midgard/src"

# --- what uninstall --purge may delete ------------------------------------------------ [AC-40.2]
refuses "purge_dir_is_safe refuses / " call purge_dir_is_safe /
refuses "purge_dir_is_safe refuses a folder without install.env" call purge_dir_is_safe "$work/midgard"
mkdir "$work/marker-dir" "$work/marker-dir/install.env"
refuses "purge_dir_is_safe refuses install.env that is a folder" call purge_dir_is_safe "$work/marker-dir"
mkdir "$work/marker-link"
ln -s "$work/a-file" "$work/marker-link/install.env"
refuses "purge_dir_is_safe refuses install.env that is a link" call purge_dir_is_safe "$work/marker-link"
refuses "purge_dir_is_safe refuses the symbolic link" call purge_dir_is_safe "$work/link"
touch "$work/midgard/install.env"
accepts "purge_dir_is_safe accepts a folder with install.env" call purge_dir_is_safe "$work/midgard"
if [ "$me" = 0 ]; then
    expect "as root, the owner check runs silently" "" call purge_dir_is_safe "$work/midgard"
else
    expect "unprivileged, the skipped owner check says so" \
        "not running as root, so the check that $work/midgard is owned by root was skipped" \
        call purge_dir_is_safe "$work/midgard"
fi
grep -n 'purge_dir_is_safe "$INSTALL_DIR"\|rm -rf "$INSTALL_DIR"$' "$installer" | cut -d: -f1 >"$work/lines"
if [ "$(wc -l <"$work/lines")" = 2 ] && [ "$(head -n 1 "$work/lines")" -lt "$(tail -n 1 "$work/lines")" ]; then
    pass "cmd_uninstall checks the folder before rm -rf"
else
    fail "cmd_uninstall checks the folder before rm -rf: lines $(tr '\n' ' ' <"$work/lines")"
fi

# --- the unattended password file ----------------------------------------------------- [AC-40.4]
other=0; [ "$me" = 0 ] && other=1
printf 'correct horse battery\n' >"$work/pw"
chmod 0600 "$work/pw"
refuses "require_private_file refuses a missing file" call require_private_file "$work/none" "$me"
refuses "require_private_file refuses no file at all" call require_private_file "" "$me"
ln -s "$work/pw" "$work/pw-link"
refuses "require_private_file refuses a symbolic link" call require_private_file "$work/pw-link" "$me"
refuses "require_private_file refuses a folder" call require_private_file "$work/midgard" "$me"
refuses "require_private_file refuses mode 0600 owned by another uid" call require_private_file "$work/pw" "$other"
accepts "require_private_file accepts mode 0600 owned by the expected uid" call require_private_file "$work/pw" "$me"
chmod 0400 "$work/pw"
accepts "require_private_file accepts mode 0400" call require_private_file "$work/pw" "$me"
chmod 0644 "$work/pw"
refuses "require_private_file refuses mode 0644" call require_private_file "$work/pw" "$me"
chmod 0640 "$work/pw"
refuses "require_private_file refuses mode 0640" call require_private_file "$work/pw" "$me"
if [ "$me" != 0 ]; then
    chmod 0600 "$work/pw"
    refuses "require_private_file expects root by default" call require_private_file "$work/pw"
fi

# --- the Docker apt key ------------------------------------------------------------------ [AC-40.3]
expect "the Docker key fingerprint is pinned once" 1 \
    grep -c '^DOCKER_APT_KEY_FPR=9DC858229FC7DD38854AE2D88D81803C0EBFCD88$' "$installer"
if command -v gpg >/dev/null; then
    export GNUPGHOME="$work/gnupg"
    mkdir -m 0700 "$GNUPGHOME"
    for who in one two; do
        gpg --batch --quiet --pinentry-mode loopback --passphrase '' \
            --quick-generate-key "mg-install test $who <$who@test.invalid>" ed25519 sign never 2>/dev/null
    done
    fpr_one="$(gpg --with-colons --fingerprint one@test.invalid 2>/dev/null | awk -F: '/^fpr:/ {print $10; exit}')"
    gpg --armor --export one@test.invalid >"$work/one.asc" 2>/dev/null
    gpg --armor --export >"$work/both.asc" 2>/dev/null
    gpgconf --kill gpg-agent 2>/dev/null || true
    unset GNUPGHOME
    refuses "a key that is not Docker's is refused" call docker_key_fingerprint_matches "$work/one.asc"
    accepts "the same key is accepted when its own fingerprint is expected" \
        call docker_key_fingerprint_matches "$work/one.asc" "$fpr_one"
    refuses "a file with a second key is refused, even with the first expected" \
        call docker_key_fingerprint_matches "$work/both.asc" "$fpr_one"
    refuses "a file that is not a key is refused" call docker_key_fingerprint_matches "$work/a-file" "$fpr_one"
else
    echo "skip  the Docker key cases: gpg is not installed"
fi

echo
if [ "$failures" -ne 0 ]; then
    echo "$failures test(s) failed."
    exit 1
fi
echo "All mg-install guard tests passed."
