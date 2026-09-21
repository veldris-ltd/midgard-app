#!/usr/bin/env python3
"""Render the Midgard introduction film from the captured console screens.

Composites the captures in `screenshots/<mode>/` into a 1920x1080 H.264 MP4
according to the beat list in `slides.json` — one beat per screen, each with its
own duration, product accent and caption. `--mode` picks both the capture set and
the film's own chrome, so light screens sit in a light frame and dark in dark.
Output is deterministic: same inputs, same film.

Chrome colours and type come from the platform's own design system
(`ui/design-system/src/tokens`): the ink ramp, the per-product accents, and the
Oxanium / Space Grotesk / JetBrains Mono trio.

    python docs/demo/render.py                     # -> docs/demo/midgard-intro.mp4
    python docs/demo/render.py --mode light        # -> docs/demo/midgard-intro-light.mp4
    python docs/demo/render.py --probe             # contact sheet, no encode
    python docs/demo/render.py --out /tmp/cut.mp4 --crf 23

Requires Pillow and an ffmpeg (either `ffmpeg` on PATH or the `imageio-ffmpeg`
package). The type is read from the design system's self-hosted fonts in
`ui/design-system/src/fonts`, so rendering needs no network. See README.md.
"""
import argparse, json, os, shutil, subprocess, sys, time
from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))

W, H  = 1920, 1080
XFADE = 0.5          # seconds of crossfade between screens
ZOOM  = 0.045        # slow push across each beat

# ── palette · ui/design-system/src/tokens/colors.css ────────────────────────
# One set per colour mode. Dark is the platform default (`:root`); light is the
# `.mode-light` re-pointing, where accent text steps darker (-600) on paper
# instead of lighter (-300). `use_mode()` binds a set to the names used below.
PALETTES = {
    "dark": dict(
        INK_900=(10, 11, 13),       # page
        INK_700=(30, 33, 38),       # progress track
        INK_600=(42, 45, 52),       # hairlines
        INK_500=(58, 62, 70),       # progress, beats already played
        GREY_100=(233, 235, 238),   # headings
        GREY_300=(182, 188, 196),   # caption body
        GREY_400=(145, 153, 163),
        GREY_500=(107, 114, 121),
        ACCENT={                    # (mark, text)
            "midgard":  ((202, 29, 37),  (242, 107, 112)),
            "forge":    ((217, 110, 30), (243, 155, 87)),
            "mimir":    ((35, 168, 182), (99, 211, 221)),
            "bifrost":  ((125, 92, 230), (171, 147, 245)),
            "heimdall": ((201, 154, 34), (236, 197, 92)),
        },
    ),
    "light": dict(
        INK_900=(239, 240, 243),    # --bg-page
        INK_700=(214, 217, 222),    # --border
        INK_600=(214, 217, 222),    # --border
        INK_500=(191, 196, 203),    # --border-strong
        GREY_100=(16, 17, 20),
        GREY_300=(56, 61, 68),
        GREY_400=(92, 99, 107),
        GREY_500=(138, 144, 153),
        ACCENT={
            "midgard":  ((202, 29, 37),  (168, 23, 30)),
            "forge":    ((217, 110, 30), (178, 87, 21)),
            "mimir":    ((35, 168, 182), (23, 130, 142)),
            "bifrost":  ((125, 92, 230), (98, 68, 194)),
            "heimdall": ((201, 154, 34), (161, 122, 23)),
        },
    ),
}
SECURE = (33, 192, 122)


def use_mode(mode):
    """Bind the palette for `mode` to the module-level colour names."""
    globals().update(PALETTES[mode])


use_mode("dark")

# ── layout ──────────────────────────────────────────────────────────────────
COL_X, COL_W           = 352, 1216       # content column
MAST_H                 = 70
FR_X, FR_Y, FR_W, FR_H = COL_X, 84, COL_W, 760
EYE_Y, HEAD_Y, LEDE_Y  = 876, 898, 954
BAR_Y, FOOT_Y          = 1040, 1055

FOOTER = ("SHIPPING MIDGARD CONSOLES, CAPTURED WITH NO NETWORK"
          "  ·  FIGURES ARE EACH CONSOLE'S BUILT-IN SAMPLE DATA")
# The design system's three faces: the self-hosted OFL variable fonts the
# consoles ship, so the film is set in exactly the same files.
FONTS_DIR = os.path.normpath(os.path.join(HERE, "..", "..", "ui", "design-system", "src", "fonts"))
FONTS_NEEDED = ("Oxanium", "SpaceGrotesk", "JetBrainsMono")


def parse_args():
    p = argparse.ArgumentParser(description="Render the Midgard introduction film.")
    p.add_argument("--mode",   choices=sorted(PALETTES), default="dark",
                   help="colour mode: picks screenshots/<mode>/ and the film's chrome")
    p.add_argument("--slides", default=os.path.join(HERE, "slides.json"))
    p.add_argument("--shots",  help="capture directory (default: screenshots/<mode>)")
    p.add_argument("--fonts",  default=FONTS_DIR)
    p.add_argument("--out",    help="output file (default: midgard-intro.mp4, "
                                    "or midgard-intro-light.mp4 in light mode)")
    p.add_argument("--fps",    type=int, default=30)
    p.add_argument("--crf",    type=int, default=17, help="17 master, 23 web weight")
    p.add_argument("--preset", default="slow")
    p.add_argument("--probe",  action="store_true",
                   help="write a contact sheet of six frames (probe.jpg, or "
                        "probe-light.jpg in light mode) and exit")
    a = p.parse_args()
    suffix = "" if a.mode == "dark" else "-" + a.mode
    a.shots = a.shots or os.path.join(HERE, "screenshots", a.mode)
    a.out = a.out or os.path.join(HERE, "midgard-intro%s.mp4" % suffix)
    a.probe_name = "probe%s.jpg" % suffix
    return a


def find_ffmpeg():
    exe = shutil.which("ffmpeg")
    if exe:
        return exe
    try:
        import imageio_ffmpeg
        return imageio_ffmpeg.get_ffmpeg_exe()
    except ImportError:
        sys.exit("No ffmpeg found. Install one on PATH, or: pip install imageio-ffmpeg")


class Fonts:
    """Variable-weight faces from the design system, loaded once per size+weight."""

    def __init__(self, root):
        missing = [n for n in FONTS_NEEDED if not os.path.exists(os.path.join(root, n + ".ttf"))]
        if missing:
            sys.exit(
                "Missing font(s) in %s:\n  %s\n\n"
                "The film uses the design system's self-hosted fonts; see\n"
                "ui/design-system/src/fonts/README.md." %
                (root, "\n  ".join(n + ".ttf" for n in missing)))
        self.root, self._cache = root, {}

    def get(self, name, size, weight):
        key = (name, size, weight)
        if key not in self._cache:
            f = ImageFont.truetype(os.path.join(self.root, name + ".ttf"), size)
            f.set_variation_by_axes([weight])
            self._cache[key] = f
        return self._cache[key]

    def disp(self, s, w=700): return self.get("Oxanium", s, w)
    def body(self, s, w=400): return self.get("SpaceGrotesk", s, w)
    def mono(self, s, w=400): return self.get("JetBrainsMono", s, w)


def tw(d, txt, f):
    return d.textbbox((0, 0), txt, font=f)[2]


def track(d, xy, txt, f, fill, sp=0.0):
    """Draw text with letter-spacing; returns the advance width."""
    if sp == 0:
        d.text(xy, txt, font=f, fill=fill)
        return tw(d, txt, f)
    x, y = xy
    for ch in txt:
        d.text((x, y), ch, font=f, fill=fill)
        x += tw(d, ch, f) + sp
    return x - xy[0]


def track_w(d, txt, f, sp):
    return sum(tw(d, c, f) + sp for c in txt)


def wrap(d, txt, f, width):
    lines, cur = [], ""
    for word in txt.split(" "):
        trial = word if not cur else cur + " " + word
        if tw(d, trial, f) <= width:
            cur = trial
        else:
            if cur:
                lines.append(cur)
            cur = word
    if cur:
        lines.append(cur)
    return lines


def fit(d, txt, width, max_lines, sizes, mk):
    """Largest size from `sizes` whose wrap fits inside max_lines."""
    for s in sizes:
        f = mk(s)
        ls = wrap(d, txt, f, width)
        if len(ls) <= max_lines:
            return f, ls
    f = mk(sizes[-1])
    return f, wrap(d, txt, f, width)[:max_lines]


def clock(t):
    return "%d:%02d" % (int(t // 60), int(t % 60))


class Film:
    def __init__(self, args):
        self.a = args
        self.F = Fonts(args.fonts)
        self.slides = json.load(open(args.slides, encoding="utf-8"))
        self.total = sum(s["sec"] for s in self.slides)

        self.spans, acc = [], 0.0
        for s in self.slides:
            self.spans.append((acc, acc + s["sec"]))
            acc += s["sec"]

        print("%d screens, %ds (%s), %d frames"
              % (len(self.slides), self.total, clock(self.total), int(self.total * args.fps)),
              flush=True)

        # Sources are pre-scaled once so every per-frame zoom crop is a downscale.
        self.src = {}
        for s in self.slides:
            if s["img"] in self.src:
                continue
            p = os.path.join(args.shots, s["img"] + ".png")
            if not os.path.exists(p):
                sys.exit("Missing screenshot: %s" % p)
            self.src[s["img"]] = Image.open(p).convert("RGB").resize((1600, 1000), Image.LANCZOS)

        self.bases = {a: self._base(a) for a in ACCENT}
        self.caps = [self._caption(i, s) for i, s in enumerate(self.slides)]
        print("chrome built", flush=True)

    # ── static chrome, one canvas per product accent ────────────────────────
    def _base(self, acc):
        F = self.F
        acc_c, _ = ACCENT[acc]
        im = Image.new("RGB", (W, H), INK_900)
        d = ImageDraw.Draw(im)

        d.rounded_rectangle([COL_X, 22, COL_X + 26, 48], 6, fill=(202, 29, 37))
        d.text((COL_X + 13, 34), "M", font=F.disp(19, 800), fill=(255, 255, 255), anchor="mm")
        track(d, (COL_X + 38, 25), "MIDGARD", F.disp(19, 700), GREY_100, 1.9)
        d.line([COL_X + 158, 24, COL_X + 158, 46], fill=INK_600)
        track(d, (COL_X + 174, 29), "AN INTRODUCTION", F.mono(12, 400), GREY_500, 1.7)

        chip_f = F.mono(11, 500)
        label = "CAPTURED OFFLINE  ·  NO OUTBOUND CALLS"
        cx = COL_X + COL_W - (track_w(d, label, chip_f, 1.4) + 34)
        d.rounded_rectangle([cx, 22, COL_X + COL_W, 48], 13, outline=INK_600)
        d.ellipse([cx + 13, 32, cx + 20, 39], fill=SECURE)
        track(d, (cx + 27, 29), label, chip_f, GREY_400, 1.4)
        d.line([COL_X, MAST_H, COL_X + COL_W, MAST_H], fill=INK_600)

        # stage frame + corner registration marks
        d.rectangle([FR_X - 1, FR_Y - 1, FR_X + FR_W, FR_Y + FR_H], outline=INK_600)
        for x, y, dx, dy in ((FR_X - 1, FR_Y - 1, 1, 1), (FR_X + FR_W, FR_Y + FR_H, -1, -1)):
            d.line([x, y, x + 16 * dx, y], fill=acc_c, width=2)
            d.line([x, y, x, y + 16 * dy], fill=acc_c, width=2)

        track(d, (COL_X, FOOT_Y), FOOTER, F.mono(10, 400), GREY_500, 1.1)
        return im

    # ── caption layer per screen (crossfaded like the stage) ────────────────
    def _caption(self, i, s):
        F = self.F
        _, acc_txt = ACCENT[s["acc"]]
        lay = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        d = ImageDraw.Draw(lay)

        x = COL_X
        x += track(d, (x, EYE_Y), "%02d" % (i + 1), F.mono(13, 600), GREY_500, 2.0) + 12
        x += track(d, (x, EYE_Y), s["eyebrow"], F.mono(13, 500), acc_txt, 2.4) + 16
        d.line([x, EYE_Y + 7, min(x + 260, COL_X + COL_W), EYE_Y + 7], fill=INK_600)

        hf, hl = fit(d, s["head"], COL_W, 1, [38, 35, 32, 29], lambda n: F.disp(n, 700))
        d.text((COL_X, HEAD_Y), hl[0], font=hf, fill=GREY_100)

        lf, ll = fit(d, s["body"], COL_W, 3, [19, 18, 17], lambda n: F.body(n, 400))
        for n, line in enumerate(ll):
            d.text((COL_X, LEDE_Y + n * 26), line, font=lf, fill=GREY_300)
        return lay

    def _stage(self, i, u):
        """Screen `i` at progress u in [0,1] — a slow push toward the upper middle."""
        src = self.src[self.slides[i]["img"]]
        sw, sh = src.size
        z = 1.0 + ZOOM * u
        cw, ch = sw / z, sh / z
        left = max(0, min(sw - cw, sw * 0.5 - cw / 2))
        top = max(0, min(sh - ch, sh * 0.36 - ch / 2))
        return src.resize((FR_W, FR_H), Image.BILINEAR, box=(left, top, left + cw, top + ch))

    def _at(self, t):
        for i, (a, b) in enumerate(self.spans):
            if t < b or i == len(self.spans) - 1:
                return i, (t - a) / self.slides[i]["sec"]
        return len(self.slides) - 1, 1.0

    def frame(self, t):
        """One finished 1920x1080 frame at time t."""
        i, u = self._at(t)
        into = t - self.spans[i][0]
        fading = i > 0 and into < XFADE
        k = into / XFADE if fading else 1.0

        out = self.bases[self.slides[i]["acc"]].copy()
        cur = self._stage(i, u)
        out.paste(Image.blend(self._stage(i - 1, 1.0), cur, k) if fading else cur, (FR_X, FR_Y))

        if fading:
            for lay, alpha in ((self.caps[i - 1], 1 - k), (self.caps[i], k)):
                c = lay.copy()
                c.putalpha(c.getchannel("A").point(lambda p, a=alpha: int(p * a)))
                out.paste(c, (0, 0), c)
        else:
            out.paste(self.caps[i], (0, 0), self.caps[i])

        d = ImageDraw.Draw(out)
        acc_c, _ = ACCENT[self.slides[i]["acc"]]

        gap = 4
        usable = COL_W - gap * (len(self.slides) - 1)
        x = COL_X
        for m, s in enumerate(self.slides):
            w = usable * s["sec"] / self.total
            d.rectangle([x, BAR_Y, x + w, BAR_Y + 4], fill=INK_700)
            f = 1.0 if m < i else (min(u, 1.0) if m == i else 0.0)
            if f > 0:
                d.rectangle([x, BAR_Y, x + w * f, BAR_Y + 4], fill=INK_500 if m < i else acc_c)
            x += w + gap

        cf = self.F.mono(12, 500)
        txt = "%s / %s" % (clock(t), clock(self.total))
        track(d, (COL_X + COL_W - track_w(d, txt, cf, 1.2), FOOT_Y - 1), txt, cf, GREY_400, 1.2)
        return out


def main():
    args = parse_args()
    use_mode(args.mode)
    film = Film(args)

    if args.probe:
        picks = [1.0, 9.3, 25.0, 48.0, 62.0, 101.0]
        sheet = Image.new("RGB", (W, H // 2 * 3), INK_900)
        for n, t in enumerate(picks):
            sheet.paste(film.frame(t).resize((W // 2, H // 2), Image.LANCZOS),
                        ((n % 2) * (W // 2), (n // 2) * (H // 2)))
        p = os.path.join(os.path.dirname(args.out) or ".", args.probe_name)
        sheet.save(p, quality=88)
        print("probe ->", p)
        return

    cmd = [find_ffmpeg(), "-y", "-f", "rawvideo", "-pix_fmt", "rgb24",
           "-s", "%dx%d" % (W, H), "-r", str(args.fps), "-i", "-", "-an",
           "-c:v", "libx264", "-preset", args.preset, "-crf", str(args.crf),
           "-pix_fmt", "yuv420p", "-movflags", "+faststart", args.out]
    proc = subprocess.Popen(cmd, stdin=subprocess.PIPE,
                            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    n_frames = int(film.total * args.fps)
    t0 = time.time()
    for n in range(n_frames):
        proc.stdin.write(film.frame(n / args.fps).tobytes())
        if n % 300 == 0:
            print("  %d/%d  %6.1fs  (%.0fs elapsed)"
                  % (n, n_frames, n / args.fps, time.time() - t0), flush=True)
    proc.stdin.close()
    if proc.wait() != 0:
        sys.exit("ffmpeg failed with exit code %d" % proc.returncode)

    print("done in %.0fs -> %s (%.1f MB)"
          % (time.time() - t0, args.out, os.path.getsize(args.out) / 1024 / 1024), flush=True)


if __name__ == "__main__":
    main()
