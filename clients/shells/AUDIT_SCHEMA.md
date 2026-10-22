# Shell accessibility audit records (RT-49 / RC-49)

`accessibility.toml` is a **static conformance declaration** — what each shell
*claims* to support (AC-C-11's completeness gate). This file defines the
**per-release audit record** that proves the claim was *measured on a real
device/runner*: the missing half RC-49 requires.

## Where records live

```
clients/shells/<shell>/audits/<release>.toml
```

One record per shell per release. They are **written by CI** — the on-device
audit jobs in [`.github/workflows/shell-audit.yml`](../../.github/workflows/shell-audit.yml)
— never hand-authored. A record asserts that the shell's UI was driven on a real
runner by the platform's audit tool and passed every required affordance.

## Record format

```toml
# Accessibility audit record — <shell> — release <version>
# WRITTEN BY CI on a real runner. Do not hand-edit results.
[audit]
shell        = "apple"                       # must match the shell directory
release      = "1.4.0"                        # the release tag this audit gates
commit       = "<40-hex git sha>"             # the commit audited
runner       = "macos-14"                     # the REAL runner the audit executed on
device       = "iPhone 15 (iOS 17.5)"         # concrete device / emulator / browser
tool         = "XCUITest + XCUIAccessibilityAudit"
tool_version = "Xcode 15.4"
timestamp    = "2026-09-09T12:00:00Z"         # RFC3339, when the audit ran
overall      = "pass"                          # pass | fail

# Measured (not declared) result for every required affordance.
[results]
dynamic_type         = "pass"                  # pass | fail
screen_reader_labels = "pass"
contrast_aa          = "pass"
focus_order          = "pass"
reduced_motion       = "pass"
keyboard_navigation  = "pass"
hit_target_size      = "pass"

[evidence]
report = "shell-audit/apple-1.4.0.json"        # raw tool report, uploaded as a CI artifact
issues = 0                                      # count of violations found
```

## The per-release gate

[`deploy/acceptance/verify-shell-audits.sh <release>`](../../deploy/acceptance/verify-shell-audits.sh)
**fails closed** unless *every* shell has a record for `<release>` that:

- names a real `runner`, `device`, and `tool` (no placeholders),
- carries `overall = "pass"`, and
- shows every affordance in `accessibility.toml` measured `pass` in `[results]`.

The release workflow runs this gate after the audit jobs record their results, so
a release cannot ship without a fresh, passing on-device audit for all six shells.
On a release where the audits have not run, the gate fails closed — by design.

### Pre-release waivers

A **pre-release** (a version with a `-` suffix, such as `0.1.0-rc.1`) may be waived by
one line in [`audit-waivers.txt`](audit-waivers.txt) naming that exact version, who
approved it, the date and the reason. The gate still lists every missing or failing
record, then reports `WAIVED` and passes. A waiver covers only the version it names,
a line without `approved-by:` and `reason:` is refused, and a **final release can never
be waived**. The list must be empty at GA.

## Per-platform audit tools

| Shell | Runner | Tool |
|---|---|---|
| `apple` | `macos-14` + iOS simulator | XCUITest + `XCUIAccessibilityAudit` |
| `android` | `macos`/`ubuntu` + emulator | Espresso + Accessibility Test Framework / Scanner |
| `web` | `ubuntu` + Chromium | Playwright + `@axe-core/playwright` (WCAG 2.1 AA) |
| `extension` | `ubuntu` + Chromium | Playwright + axe (popup + injected UI) |
| `desktop-tauri` | `ubuntu`/`windows` | Tauri WebDriver + axe on the webview |
| `linux-gtk` | `ubuntu` | AT-SPI2 introspection + `accerciser`-style audit |

The web-family audit is the concrete reference implementation
([`web/a11y/axe-audit.mjs`](web/a11y/axe-audit.mjs)); the native audits follow the
same record contract.
