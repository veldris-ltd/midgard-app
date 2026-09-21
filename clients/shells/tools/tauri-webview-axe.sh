#!/usr/bin/env bash
# Desktop-Tauri on-device accessibility audit driver (RT-49 / RC-49).
#
# Drives the built Tauri app's real WebKitGTK webview via tauri-driver (WebDriver)
# and runs axe-core against it, printing the space-separated list of FAILED
# affordances on stdout (empty ⇒ all pass) for record-audit.sh to record.
#
# Fails closed (non-zero, no output consumed as "pass") if the shell has not been
# built — an audit is never fabricated for a UI that did not run.
set -uo pipefail
release="${1:?release}"
root="clients/shells/desktop-tauri"

if [ ! -e "$root/src-tauri/tauri.conf.json" ]; then
    echo "tauri-webview-axe: desktop-tauri shell has no Tauri project yet — cannot audit (RC-49)" >&2
    exit 1
fi
# Real steps (run on the CI runner once the shell UI lands):
#   1. cargo tauri build  (produces the app + webview)
#   2. tauri-driver &      (WebDriver bridge to the running webview)
#   3. axe-core over the webdriver session, mapped to the seven affordances
# Until the driver harness lands with the shell UI, fail closed rather than emit a
# fabricated pass.
echo "tauri-webview-axe: audit harness not yet wired to the desktop-tauri UI — failing closed (RC-49)" >&2
exit 1
