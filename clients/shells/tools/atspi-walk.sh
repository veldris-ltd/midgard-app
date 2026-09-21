#!/usr/bin/env bash
# Linux-GTK on-device accessibility audit driver (RT-49 / RC-49).
#
# Launches the built GTK4 shell under Xvfb and walks its AT-SPI2 accessibility tree,
# asserting per-affordance: every actionable node has a role + accessible name
# (screen_reader_labels), a sane focus order (focus_order / keyboard_navigation),
# and hit-target sizes ≥ the guideline minimum. Prints the space-separated list of
# FAILED affordances on stdout (empty ⇒ all pass) for record-audit.sh.
#
# Fails closed if the shell has not been built — never fabricates a pass.
set -uo pipefail
release="${1:?release}"
root="clients/shells/linux-gtk"

if [ ! -e "$root/Cargo.toml" ]; then
    echo "atspi-walk: linux-gtk shell has no buildable UI yet — cannot audit (RC-49)" >&2
    exit 1
fi
# Real steps (run on the CI runner once the shell UI lands):
#   1. cargo build --release      (builds the GTK4 app)
#   2. launch the app under Xvfb
#   3. connect to AT-SPI2 (pyatspi / atspi crate) and walk the tree, checking
#      labels, roles, focus order and hit-target sizes; emit failed affordances.
echo "atspi-walk: audit harness not yet wired to the linux-gtk UI — failing closed (RC-49)" >&2
exit 1
