# Demo film

A 1:54 introduction to Midgard for non-technical audiences, assembled entirely
from screens captured off the shipping product consoles. No mockups, no
illustrative renders: every frame is a real console.

```bash
python docs/demo/render.py                 # -> docs/demo/midgard-intro.mp4
python docs/demo/render.py --mode light    # -> docs/demo/midgard-intro-light.mp4
```

| | |
|---|---|
| Runtime | 1:54 (114s), 16 beats |
| Output | 1920×1080, 30 fps, H.264 / yuv420p, silent |
| Size | ~46 MB at the default CRF 17; ~22 MB at `--crf 23` |
| Render | ~90s on a modern laptop |

---

## What is here

```
docs/demo/
├── render.py        ← compositor + encoder (Pillow → ffmpeg)
├── capture.mjs      ← recaptures screenshots/, both colour modes, with no network
├── slides.json      ← the cut: 16 beats, each with duration, accent and copy
├── screenshots/
│   ├── dark/        ← 28 captures, 3840×2400 PNG (the film's default)
│   └── light/       ← the same 28 captures in light mode
└── vendor/          ← the site pages' scripts, fetched on demand, not committed
```

## Colour modes

The film exists in both of the platform's colour modes. `--mode` picks the
capture set (`screenshots/dark/` or `screenshots/light/`) and the film's own
chrome together, so light screens sit in a light frame and dark screens in a
dark one. Both palettes come from `ui/design-system/src/tokens/colors.css`:
dark is the `:root` default, light is the `.mode-light` re-pointing. Dark is
the default, as it is for the platform.

## The cut

`slides.json` is the edit decision list. One object per beat, in order:

| field | meaning |
|---|---|
| `img` | basename in `screenshots/<mode>/`, without `.png` |
| `sec` | how long the beat holds, in seconds |
| `acc` | product accent — `midgard`, `forge`, `mimir`, `bifrost`, `heimdall` |
| `ch` | chapter label grouping the beats into acts; `render.py` does not draw it |
| `eyebrow` | the small mono label above the caption |
| `head` | one-line headline, auto-shrunk from 38px to fit a single line |
| `body` | the caption sentence, wrapped to at most three lines |

Re-order, re-time or re-word by editing that file and re-running. Nothing else
needs to change; the progress bar weights itself by duration and the chrome
re-colours itself from `acc`.

Fourteen of the sixteen beats are product consoles. Two — `site-home` and
`site-arch` — are pages from `docs/design/site`, used as the open and the close.
The cut uses 16 of the 28 captures; the rest are there to swap in.

## Screenshots

`capture.mjs` takes all 28 screens, once per colour mode, into
`screenshots/dark/` and `screenshots/light/` under the same filename. 26 are
console screens, each the default state of one route; two are the site pages.
`node docs/demo/capture.mjs --list` prints every screen and the path it is
captured from.

The screens are only as current as the build, so rebuild the consoles first:

```bash
VITE_GATEWAY=mock pnpm --filter @midgard/console-midgard build
pnpm --filter @midgard/console-forge --filter @midgard/console-mimir \
     --filter @midgard/console-heimdall --filter @midgard/console-bifrost \
     --filter @midgard/console-admin --filter @midgard/client-shells build

node docs/demo/capture.mjs --fetch-vendor               # once
node docs/demo/capture.mjs                              # both modes, all 28
node docs/demo/capture.mjs --mode light --only site-home,midgard-fabric
```

The Midgard console is the one console that can run against the HTTP gateway,
and it does unless built with `VITE_GATEWAY=mock`. The other six always use
their mock gateways.

What a capture guarantees:

- **No network.** Every hostname fails to resolve inside the browser; the page
  reaches only the static server the script runs on `127.0.0.1`. The consoles
  and the site need nothing else, because their fonts are self-hosted in
  `ui/design-system/src/fonts` and `docs/design/fonts`. The site pages reference
  React and Babel on unpkg; those requests are answered from `vendor/`.
- **The right mode.** The consoles and the site read `localStorage('mg-mode')`
  before first paint, so the script sets it before any page script runs. It
  also sets `mg-consent`, so the site's one-time storage notice stays closed,
  as it is for a returning visitor.
- **A real render or nothing.** A screen fails, and its previous capture is
  kept, on any failed request, HTTP error, uncaught page exception, blank page
  or wrong colour mode. The script exits non-zero if any screen failed.

`--fetch-vendor` fills `vendor/` once: React and React DOM are copied from
`node_modules`, and Babel standalone is downloaded from unpkg. It prints each
file's sha256. That download is the only outbound call anything in this
directory makes, and it is never made during a capture or a render.

The viewport is **1600×1000 CSS pixels at 2.4× device pixel ratio**. The CSS
size is what the consoles lay out against, so changing it changes the layout;
the scale factor (`VIEW` in `capture.mjs`) only sets resolution. Keep 1600×1000
if you want frames that match this cut.

`render.py` downscales each source to 1600×1000 once at startup, so every
per-frame zoom crop is a downscale and stays sharp.

Two honest notes about what the screens show:

- The consoles run on their `MockGateway` fixture sets, so the figures are each
  console's **built-in sample data**, not a live enclosure. The film states this
  on every frame in the footer strip.
- `heimdall-picture` carries the product's own `SYNTHETIC PICTURE · REFERENCE
  LAYOUT ONLY` marking. That is left visible deliberately — it is what the
  console displays.

## Fonts

The film is set in the platform's own three faces — Oxanium, Space Grotesk and
JetBrains Mono. `render.py` reads them from `ui/design-system/src/fonts`: the
same self-hosted OFL-1.1 files the consoles ship, with their licences and
sources recorded beside them. Nothing fetches fonts.

## Requirements

- **To render:** Pillow, and ffmpeg — either on `PATH`, or
  `pip install imageio-ffmpeg` for a bundled build. `render.py` prefers `PATH`
  and falls back to the package.
- **To capture:** Node ≥ 22, the consoles built as above, and a Chromium
  browser (Edge or Chrome). The script finds one on the usual install paths;
  otherwise pass `--browser PATH` or set `CHROME_PATH`.

## Options

`render.py`:

```
--mode dark|light colour mode: capture set and chrome   (default: dark)
--slides PATH     edit decision list           (default: ./slides.json)
--shots  DIR      screenshot directory         (default: ./screenshots/<mode>)
--fonts  DIR      font directory               (default: ui/design-system/src/fonts)
--out    PATH     output file                  (default: ./midgard-intro.mp4,
                                                ./midgard-intro-light.mp4 in light)
--fps N           frame rate                   (default: 30)
--crf N           quality; 17 master, 23 web   (default: 17)
--preset NAME     x264 preset                  (default: slow)
--probe           write a contact sheet of six frames and exit
                  (probe.jpg, or probe-light.jpg in light mode)
```

`--probe` renders six representative frames into a single JPEG in a second or
two. Use it to check a layout or copy change before committing to a full encode.

`capture.mjs`:

```
--mode dark|light|both   colour mode(s) to capture       (default: both)
--only a,b,...           capture just these screens      (default: all 28)
--out DIR                writes DIR/<mode>/<name>.png    (default: ./screenshots)
--browser PATH           Chromium browser                (default: $CHROME_PATH, else found)
--fetch-vendor           fill vendor/ for the site pages and exit
--list                   print the screens and their paths and exit
```
