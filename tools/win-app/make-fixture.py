#!/usr/bin/env python3
"""Write a tiny synthetic recording for the suite's end-to-end smoke test (run-suite.py).

    python tools/win-app/make-fixture.py OUT_DIR

OUT_DIR must not exist or be empty. It gets what midgard-replay reads (format version 1,
tools/replay/src/recording.rs), as tools/mac-app/record.py packs it, except that the
batches are plain heimdall.jsonl: replay decompresses heimdall.jsonl.gz with
/usr/bin/gzip, which Windows lacks, so recordings for Windows are shipped uncompressed.

  meta.json            version 1, recordedFrom, duration, feeds, forgeFiles
  heimdall.jsonl       FEEDS' batches: 20 observations over the air, sea, land, events
                       and utilities layers, among them one land-camera
  media/CAM-TEST.jpg   the camera's still (an 8x8 grey JPEG made here), which its
                       image_url names as /media/CAM-TEST.jpg
  forge/manifest.json  one version of the Forge source file sites.csv
  forge/000000-sites.csv

Everything is made up here: no recorded or third-party data. The same input gives the
same bytes. Python standard library only.
"""
import json
import os
import sys

# The recording's start (Unix seconds; replay moves every time to "now") and length.
RECORDED_FROM = 1_792_000_000
DURATION = 120

# Feed (the mesh identity heimdall-source signs as) -> its observations' layer.
FEEDS = {
    "fixture-air": "air",
    "fixture-sea": "sea",
    "fixture-land": "land",
    "fixture-cameras": "land",
    "fixture-events": "events",
    "fixture-utilities": "utilities",
}
CAMERA_ENTITY = "CAM-TEST"
CAMERA_IMAGE = f"/media/{CAMERA_ENTITY}.jpg"
FORGE_FILE = "sites.csv"
# The Forge job run-suite.py configures reads these columns (updated_at is a time,
# which replay moves to the loop like the observations' times).
FORGE_CSV = (
    "key,name,status,level,updated_at\n"
    f"SITE-1,Fixture site one,open,1.5,{RECORDED_FROM - 60}\n"
    f"SITE-2,Fixture site two,closed,0.25,{RECORDED_FROM - 30}\n"
    f"SITE-3,Fixture site three,open,3,{RECORDED_FROM}\n"
)


def observation(entity, kind, offset, **attrs):
    return {"entity": entity, "kind": kind, "observedAt": RECORDED_FROM + offset, "attrs": attrs}


def batches():
    """(offset, feed, observations): every feed's first batch at offset 0, so the whole
    picture is there as soon as replay primes it, and a second at offset 60."""
    out = []
    for offset in (0, 60):
        step = offset // 60
        out.append((offset, "fixture-air", [
            observation(f"FIX-AIR-{n}", "air-track", offset, callsign=f"FIX{n:03d}",
                        lat=51.0 + n * 0.1 + step * 0.01, lon=-1.0 + n * 0.1, alt_ft=12000 + n * 500,
                        alt_m=(12000 + n * 500) * 0.3048, speed=320.0, track=90.0, on_ground=False,
                        source="fixture")
            for n in (1, 2, 3)]))
        out.append((offset, "fixture-sea", [
            observation(f"FIX-SEA-{n}", "sea-vessel", offset, lat=50.5 + n * 0.05,
                        lon=-1.5 + step * 0.01, alt_m=0.0, speed=11.5, track=180.0,
                        name=f"FIXTURE VESSEL {n}", source="fixture")
            for n in (1, 2)]))
        out.append((offset, "fixture-land", [
            observation(f"FIX-TRAIN-{n}", "land-train", offset, lat=52.0 + n * 0.02,
                        lon=-0.5 - step * 0.01, alt_m=0.0, headcode=f"9F{n:02d}",
                        station="Fixture Central", event="departure", punctuality="on time",
                        source="fixture")
            for n in (1, 2)]))
        out.append((offset, "fixture-events", [
            observation("FIX-FLOOD-1", "event-flood", offset, lat=51.75, lon=-1.25,
                        severity="Flood alert", severity_level=3, area="Fixture river",
                        time_raised=RECORDED_FROM - 3600, source="fixture")]))
        out.append((offset, "fixture-utilities", [
            observation("FIX-POWER-1", "utility-power-outage", offset, lat=52.2, lon=0.1,
                        network="power", name="Unplanned outage · Fixture town", planned=False,
                        started=RECORDED_FROM - 1800, source="fixture")]))
    # The camera reports once: a still that the app's web server serves from media/.
    out.append((0, "fixture-cameras", [
        observation(CAMERA_ENTITY, "land-camera", 0, lat=51.5, lon=-0.12, alt_m=0.0,
                    name="Fixture camera", view="North", image_url=CAMERA_IMAGE,
                    camera_type="traffic", source="fixture")]))
    out.sort(key=lambda b: b[0])
    return out


def grey_jpeg():
    """An 8x8 mid-grey baseline JPEG: one block whose DC difference is 0 and whose AC
    coefficients are all zero, coded with one-code Huffman tables."""
    def segment(marker, payload):
        return bytes([0xFF, marker]) + (len(payload) + 2).to_bytes(2, "big") + payload
    app0 = segment(0xE0, b"JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00")
    dqt = segment(0xDB, b"\x00" + bytes([1] * 64))
    sof = segment(0xC0, b"\x08\x00\x08\x00\x08\x01\x01\x11\x00")
    # Class/id, then the code counts for lengths 1..16 (one code of length 1), symbols.
    dht_dc = segment(0xC4, b"\x00" + b"\x01" + b"\x00" * 15 + b"\x00")
    dht_ac = segment(0xC4, b"\x10" + b"\x01" + b"\x00" * 15 + b"\x00")
    sos = segment(0xDA, b"\x01\x01\x00\x00\x3f\x00")
    # DC category 0 ("0"), then end of block ("0"), padded with 1 bits.
    return b"\xff\xd8" + app0 + dqt + sof + dht_dc + dht_ac + sos + b"\x3f" + b"\xff\xd9"


def write(path, data):
    mode = "wb" if isinstance(data, bytes) else "w"
    with open(path, mode, **({} if mode == "wb" else {"encoding": "utf-8", "newline": "\n"})) as f:
        f.write(data)


def main(argv):
    if len(argv) != 1:
        sys.exit(__doc__.split("\n\n")[1])
    out = argv[0]
    if os.path.exists(out) and os.listdir(out):
        sys.exit(f"{out} is not empty")
    for d in (out, os.path.join(out, "media"), os.path.join(out, "forge")):
        os.makedirs(d, exist_ok=True)
    lines = [json.dumps({"offset": o, "feed": f, "observations": obs}, separators=(",", ":"),
                        allow_nan=False, ensure_ascii=False)
             for o, f, obs in batches()]
    write(os.path.join(out, "heimdall.jsonl"), "\n".join(lines) + "\n")
    write(os.path.join(out, "media", f"{CAMERA_ENTITY}.jpg"), grey_jpeg())
    rel = f"forge/{0:06d}-{FORGE_FILE}"
    write(os.path.join(out, *rel.split("/")), FORGE_CSV)
    write(os.path.join(out, "forge", "manifest.json"),
          json.dumps([{"offset": 0, "file": FORGE_FILE, "path": rel}], indent=1) + "\n")
    write(os.path.join(out, "meta.json"),
          json.dumps({"version": 1, "recordedFrom": RECORDED_FROM, "duration": DURATION,
                      "feeds": sorted(FEEDS), "forgeFiles": [FORGE_FILE]}, indent=1) + "\n")
    count = sum(len(obs) for _, _, obs in batches())
    print(f"fixture: {out}: {count} observations in {len(lines)} batches from {len(FEEDS)} feeds, "
          f"1 camera still, 1 Forge file")


if __name__ == "__main__":
    main(sys.argv[1:])
