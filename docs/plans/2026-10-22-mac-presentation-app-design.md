# Midgard presentation app for macOS — design

Date: 2026-10-22 · Status: agreed, not yet built · Branch: `md_mixedfeeds_3d`

## Purpose

Commercial teams (non-technical) present Midgard from their own Macs: Intel Core i5
MacBooks and Apple Silicon (M2) MacBook Airs. They need to install it without help,
open it with a double-click, and show the suite working, with no internet, Docker,
Terminal or browser.

## Decisions

| Question | Decision |
|---|---|
| Data | Baked-in snapshot only; the app never uses the network |
| Snapshot | A recorded hour of real data, replayed on a loop as if live |
| Products | Midgard, Heimdall, Mimir, Forge (the four that run for real). Bifrost, Admin and Clients are left out |
| Hardware | One universal app: Intel (x86_64) and Apple Silicon (arm64), macOS 13.3+ (what llama.cpp's macOS builds need), 8 GB+ |
| Mimir | Real model shipped, plus prepared real answers for the scripted moments |
| Experience | One app with its own window; home screen with four product tiles; signed in automatically |
| Signing | No Apple Developer account: unsigned, with a one-page "Open Anyway" install sheet. Build ready to sign and notarise later |
| Approach | A: native app with the real services inside (not Docker, not a recorded front-end only) |

## 1. What is inside the app and how it starts

`Midgard.app`, delivered as `Midgard-1.0.dmg` (drag to Applications), contains:

- **Shell**: a Tauri app (window, home screen, service supervisor).
- **Services**: `http-gateway`, `heimdall-feed`, `mimir-server`, `forge-worker`, built for
  macOS and merged into universal binaries.
- **Model**: `llama-server` (pinned official llama.cpp macOS releases, both chip types) and
  the Granite 3.3 2B Q4_K_M model with its signed manifest and trust key.
- **Replay**: `midgard-replay`, a new service that plays the recorded hour back.
- **Consoles**: the four consoles as static builds, served inside the window.
- **Starting state**: the enclosure (ontology, policy, 18 actions, demo users, service
  identities, slow-changing objects such as airports, runways, airlines, regions).

First launch copies the starting state to `~/Library/Application Support/Midgard`; the
bundle is never written to. Each launch starts the services on private loopback ports,
waits for health (20–40 s on an i5, behind a "Starting Midgard…" screen), signs the
presenter in and shows the home screen. Quitting stops every service. The consoles share
one origin inside the window, so one sign-in serves all four; "Switch user" changes
between the analyst (INTERNAL) and controller (RESTRICTED) demo users.

## 2. The recorded hour and its replay

**Recording** (once, on the development Mac, while the live feeds run): every Heimdall
feed batch with its offset in the hour; every Forge job's source file when it changes;
the reference data (utility networks, airspace, wind farms, platforms) once; Midgard's
slow-changing objects into the starting state. Choose an hour with power cuts, weather
warnings and flood alerts.

**Replay** (`midgard-replay`): a clock looping every hour.

- Heimdall: each batch is sent at its offset through the real signed, authenticated
  ingest path, times shifted to now, fresh observation ids each loop, so Heimdall's real
  rules raise and clear alerts.
- Forge: each recorded file is put in place at its offset, times shifted; the real
  `forge-worker` syncs it into the fabric on its schedule.
- Reference data is sent once at start-up.

The loop seam is invisible: items present at the end simply continue. The recording is
compressed (est. 50–100 MB); aircraft and ship positions are thinned to at most one every
30 s so an i5 stays smooth. Heimdall opens in 2D on Intel Macs, 3D on Apple Silicon.

## 3. Mimir and prepared answers

Mimir runs for real: ~15 s per answer on an M2 (Metal), minutes on an i5 (with the
existing "Mimir is thinking · N s" counter). For instant scripted moments, before
building we run Mimir against the recorded hour for ~6 map questions (the example buttons
in Heimdall's Mimir box) and decision support on ~5 alerts that stay open most of the
hour. Their real answers ship in the app (not browser storage, never expiring) and are
keyed by the question and the alert's **subject item**, since alert numbers change each
loop. A "Presenter's guide" panel on the home screen lists the instant questions and
which alerts to click. Replayed answers are not new model calls and do not appear in the
Mimir console's session traces.

## 4. Reliability

- **Offline**: nothing is fetched from the internet. Fonts, map backgrounds, place names
  and terrain are bundled; Heimdall's online map tiles (if any) are replaced by bundled
  ones. The smoke test asserts no outbound request.
- **Supervision**: a stopped service is restarted within seconds; the home screen shows a
  status line. Busy ports are avoided by choosing free ones. A second launch focuses the
  first window. Sleep and wake are handled by the replay clock and normal expiry.
- **Reset demo** (with confirmation) restores the starting state: data, actions,
  approvals, ledgers.
- **Disk**: existing caps (Heimdall batch audit and rotation, gateway ledger cap, fabric
  compaction and retention). ~2.5 GB installed, up to ~1 GB working data. Logs in
  `~/Library/Logs/Midgard`.
- **Install**: unsigned, so a one-page sheet (with screenshots, also inside the DMG)
  shows System Settings → Privacy & Security → Open Anyway. Signing and notarisation can
  be added later without other changes.
- **Unsupported Macs** (older macOS, less memory) get a clear message, not a failure.

## 5. Build, test, deliver

- **Build** on the development Mac: Rust toolchain on the host with both macOS targets,
  `lipo` to universal binaries; pinned llama.cpp releases; Tauri for the shell and DMG.
  One command, `tools/mac-app/build.sh`, produces the DMG. Temporary build space (~6 GB)
  is removed afterwards to respect the suite's 3.5 GB storage budget.
- **Automated smoke test** after each build: services healthy, sign-in, picture fills
  within two minutes, Forge shows its jobs, an action and its audit entry, each prepared
  answer resolves, no outbound requests.
- **Manual test** on one Intel i5 Mac and one M2 Air following the Presenter's guide.
- **Deliver** via OneDrive/Google Drive/SharePoint (~2.5 GB) with the install sheet.

## Phases

1. Native macOS builds of the services, run from a folder without Docker.
2. Recorder and `midgard-replay`.
3. Shell: home screen, auto sign-in, offline assets, supervision, Reset demo.
4. Prepared Mimir answers and Presenter's guide.
5. Universal packaging, install sheet, testing on both Macs.

The existing development setup (Docker containers, live feeds, consoles on their ports)
is unchanged; the app is built from the same code.

## Phase 5 notes (signing, notarisation, delivery)

From the phase 3 build (`tools/mac-app/build-app.sh`, ad-hoc signed; service binaries in
`Contents/Helpers`, Apple's place for helper executables):

- **Notarisation** needs every Mach-O (the shell, each helper, bundled llama-server and its
  dylibs) signed with a Developer ID, the hardened runtime (`--options runtime`) and a secure
  timestamp (`--timestamp`), innermost first, then the bundle (no `--deep` when signing).
- **Entitlements**: none expected. WKWebView runs its JavaScript JIT in WebKit's own
  processes, so the shell needs no `allow-jit`. Sign the bundled llama dylibs with the same
  team as the executables, so `disable-library-validation` is not needed.
- **Unsigned or ad-hoc distribution**: on macOS 15 (Sequoia) and later the presenter sees
  "“Midgard” Not Opened – Apple could not verify…"; the route is System Settings › Privacy &
  Security › Open Anyway (Control-click › Open no longer bypasses it on Sequoia). The
  install sheet must show this path for each supported macOS version.
- **"Midgard is damaged and can't be opened"** means the signature's seal broke in transit
  (files changed, extended attributes or symlinks mangled), not a missing signature.
- **Packaging**: zip with `ditto -c -k --keepParent Midgard.app Midgard.zip` (keeps the
  bundle's structure and metadata), not Finder's Compress or third-party zip tools; or a DMG.
  APFS clones in the build folder become full copies in the archive (~1.6 GB).
- **First launch of a quarantined copy**: Gatekeeper hashes the whole bundle, including the
  1.5 GB model, so "Verifying “Midgard”…" shows for a while (longer on an Intel i5). Say so
  in the install sheet.
- **Test plan**: round-trip the app through a `ditto` zip, set the quarantine attribute
  (`xattr -w com.apple.quarantine "0081;$(printf %x $(date +%s));Safari;" Midgard.app`, or
  download it through a browser), open it on macOS 13.3, 14 and 15, and record exactly what
  each shows (dialogs, the Open Anyway path, verification time).
