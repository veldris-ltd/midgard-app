# Midgard launch kit

A 15–20 minute launch presentation for a corporate audience, with the screenshots
and demo video it uses, and the pipeline that produced them.

| File | What it is |
|---|---|
| `Midgard-Launch.pptx` | 18 slides, 16:9, dark. Speaker notes on every slide with a timing. The demo video is embedded on slide 14. |
| `SPEAKER_NOTES.md` | The same speaker notes as a run-sheet, one section per slide. |
| `midgard-launch-demo.mp4` | The demo video: 3 min 35 s, 1920×1080, H.264, silent, captioned. |
| `screenshots/` | 23 stills at 1920×1080 taken during the recording, numbered in the order they appear. |
| `site/` | The static website for https://demo.midgard.dev: both decks and both videos behind one dark/light switch. |
| `pipeline/` | Everything that produced the above. See below. |

Every deliverable exists in both of the platform's colour modes. The light set carries a `-light` suffix and is otherwise identical in content, running order and speaker notes:

| Dark (platform default) | Light |
|---|---|
| `Midgard-Launch.pptx` | `Midgard-Launch_light.pptx` |
| `midgard-launch-demo.mp4` | `midgard-launch-demo-light.mp4` |
| `screenshots/` | `screenshots-light/` |
| `slides/` | `slides-light/` |

Both palettes come from `ui/design-system/src/tokens/colors.css`: dark is the `:root` default, light is the `.mode-light` re-pointing, where accent text steps darker on paper. Pick the set that suits the room: light reads better on a washed-out projector, dark on a good display.

The light video runs 3 min 43 s against the dark one's 3 min 35 s. The beats, captions and clicks are the same; the two takes differ only in how long each page took to settle.

## What the demo shows, and what is real

The Midgard console in the video is talking to the **real enclosure edge**:
`http-gateway` running in development mode (`--dev`), which listens on loopback
only. The login returns a genuine ML-DSA-signed token, the object
fabric returns the four seeded aircraft after per-object marking checks, the
"Ground aircraft" action is refused until an operator authorises it, the policy
dry-run is evaluated by the real policy decision point, and every row in the
audit lineage is a real ledger entry re-verified as ML-DSA-65.

The other six consoles (Forge, Mimir, Heimdall, Bifrost, Admin, client shells)
are the shipping builds running on their `MockGateway` fixtures, because their
services do not expose console routes yet. The video says so on its closing card,
the deck says so on the demo slide and on the product slides' captions, and the
"where we are" slide lists it as a known limit. Keep it that way when presenting.

## Running order

| Slide | Section | Time |
|---|---|---|
| 1–2 | Title, agenda | 1.5 min |
| 3 | The problem | 2 min |
| 4–6 | The platform: suite, principles, architecture | 4 min |
| 7–13 | Enclosure, governance, Forge, Mimir, Heimdall, Bifrost, admin and clients | 6 min |
| 14 | Demo video | 4–5 min |
| 15–16 | Post-quantum by default, proof | 3 min |
| 17–18 | Where we are, next steps | 3 min |

Every figure on the proof slide is taken from `README.md`, `CHANGELOG.md`,
`docs/releases/v0.1.0-rc.1.md` and `docs/compliance/GA_READINESS.md` at the time
of writing. The "where we are" slide repeats the GA blockers from
`GA_READINESS.md` rather than softening them.

If the embedded video does not play in the room, play `midgard-launch-demo.mp4`
beside the deck. Fonts are Arial, Calibri and Consolas, so the deck renders the
same on any Office install.

## The launch site (`site/`, for https://demo.midgard.dev)

`site/` is a complete static site: upload its **contents** to the web root of
`demo.midgard.dev`. One switch in the header selects the dark or the light set and swaps
the page theme, the slide viewer and the demo video together.

- **Deck viewer.** All 18 slides as images, with speaker notes beside them, thumbnails,
  arrow keys, swipe, full screen, and a download button for the PowerPoint file.
- **Demo video.** The recording for the selected mode, with its poster, length and size.
- **Links you can share.** `?mode=light` or `?mode=dark` opens a given set, and
  `#slide-7` opens a given slide, for example `https://demo.midgard.dev/?mode=light#slide-16`.
  Without a query the visitor's last choice is remembered (`localStorage` key `mg-mode`,
  the same key the consoles use); the default is dark.
- **No third parties.** Fonts are the design system's self-hosted OFL files. The page
  sets a `default-src 'self'` content security policy, uses no inline script or style,
  and sets no cookies.

What the host needs to do:

| Requirement | Why |
|---|---|
| Serve `index.html` at `/` | the page uses relative paths, so a sub-path also works |
| Honour HTTP `Range` requests for `.mp4` | seeking in the video; every common static host does |
| Send `video/mp4`, `image/webp`, `font/ttf` and the `.pptx` MIME type | most hosts do; check `.webp` and `.pptx` on older servers |
| Long cache lifetimes for `media/` and `assets/fonts/`, short for `index.html`, `assets/*.js`, `assets/*.css` | the media is about 80 MB and never changes between builds of the same kit |

The site weighs about 83 MB, nearly all of it the two videos and two decks. Those four
files are committed once, at the top of this folder; their copies under `site/media/` are
git-ignored, and so are the fonts under `site/assets/fonts/`, which the builder copies
from `ui/design-system/src/fonts`. **After a fresh clone, run the builder before uploading `site/`**, or the
video, the download links and the fonts will 404. It also regenerates the slide images, posters,
fonts and `site/assets/kit.js`:

```bash
cd docs/launch/pipeline
python build-site-assets.py ../site      # slides -> WebP, thumbnails, posters, decks, videos, fonts, kit.js
node serve-site.js                        # preview on http://localhost:4180 (supports Range)
```

`build-site-assets.py` uses a fresh pipeline build when one exists (`out/` and `out-light/`,
with the `png/` slide exports from `export-slides.ps1`). Otherwise it uses the committed
decks, videos and `slides*/` PNGs in this folder, and reproduces the site exactly. The page itself is
`site/index.html`, `site/assets/site.css`, `site/assets/site.js` and `site/assets/mode.js`.
The published light deck keeps the file name `Midgard-Launch_light.pptx`.

## Re-recording and rebuilding

The pipeline needs Node ≥ 22, Docker, Python 3 with Pillow, an ffmpeg with
libx264 (`pip install imageio-ffmpeg` provides one), Microsoft Edge (the
recorder drives it headlessly through Playwright) and, only for exporting slide
PNGs, PowerPoint.

```bash
cd docs/launch/pipeline
npm install                                              # playwright-core, pptxgenjs, sharp, react-icons
python prep-images.py                                    # deck fallbacks from docs/demo/screenshots

# 1. the real edge, in development mode: --dev, on loopback only (MG-SEC-06), so it
#    runs on this machine rather than behind a published container port
(cd ../../.. && just gateway-http) &                     # http://127.0.0.1:7710
#    or, on a Linux host, from the compose image on the host's network:
#    docker run -d --name mg-launch-gw --network host midgard/dev:local http-gateway --dev 127.0.0.1:7710

# 2. the Midgard console built for the edge (the six others use their checked-in mock builds)
(cd ../../../products/midgard/ui && VITE_GATEWAY=http npx vite build --outDir ../../../docs/launch/pipeline/pages/midgard --emptyOutDir)

# 3. serve the repo on the one origin the edge's CORS allowlist accepts, then record
node server.js &                                         # http://localhost:5173
node record.js                                           # -> out/raw/*.webm, out/screens/*.png  (≈4 min)

# 4. encode, then build the deck around the video and the stills
ffmpeg -i out/raw/*.webm -c:v libx264 -crf 20 -pix_fmt yuv420p -r 30 -movflags +faststart out/midgard-launch-demo.mp4
node build-deck.js                                       # -> out/Midgard-Launch.pptx
python extract-notes.py out/Midgard-Launch.pptx out/SPEAKER_NOTES.md
powershell -File export-slides.ps1 -Deck "$PWD/out/Midgard-Launch.pptx" -OutDir "$PWD/out/png"   # optional QA renders
```

Without Edge or PowerPoint: `CHROME_PATH=/path/to/chromium node record.js` records with any
Chromium build, and LibreOffice exports the slide PNGs:

```bash
soffice --headless --convert-to pdf --outdir out/pdf out/Midgard-Launch.pptx
pdftoppm -png -scale-to-x 1920 -scale-to-y 1080 out/pdf/Midgard-Launch.pdf out/png/p   # then rename p-N.png to SlideN.PNG
```

For the light set, run the same steps with `MG_MODE=light`. The recorder then sets
`mg-mode` to `light` before any page script runs, the cards and caption bar switch
palette, and output goes to `out-light/`:

```bash
MG_MODE=light node record.js
ffmpeg -i out-light/raw/*.webm -c:v libx264 -crf 20 -pix_fmt yuv420p -r 30 -movflags +faststart out-light/midgard-launch-demo-light.mp4
MG_MODE=light DEMO_LEN="3 MIN 43 S" node build-deck.js     # -> out-light/Midgard-Launch-light.pptx
```

`prep-images.py` also fills `img-light/` from `docs/demo/screenshots/light`.

Notes on the recorder:

- It records at 1920×1080 with a visible cursor and a caption bar injected into
  every page, and saves a caption-free still at each stop.
- The browser resolves no hostname except `localhost`, which it pins to
  127.0.0.1. The marketing site's React and Babel come from `docs/demo/vendor`,
  so the recording makes no outbound request.
- The edge's CORS allowlist contains exactly `http://localhost:5173`. If another
  process holds that port on `::1`, the recorder still works because of the pin
  above, but `curl` against `localhost` may hit the other process.
- The beats and their captions are in `record.js`; the title, terminal and
  closing cards are `pages/*.html`. The terminal card runs the real `curl`
  sequence against the edge from inside the page and prints the live responses.

`build-deck.js` prefers the stills in `out/screens/`; where a beat was not
recorded it falls back to the film captures under `img/`.
