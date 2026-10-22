"""Free, no-sign-up live feeds for the local Heimdall proof of concept.

    .venv/bin/python feeds/feeds.py                  run forever (see start.sh / stop.sh)
    .venv/bin/python feeds/feeds.py --once [src..]   one cycle of each (or the named) source, then exit

Sources (feed identity in brackets):
  adsb-fi      opendata.adsb.fi aircraft, UK/IE + Balkans, Middle East, Russia/Belarus, Taiwan,
               Commonwealth cities (ADSBFI_WORLD)                    -> air-track
  gnss-interference      derived from adsb-fi NIC/NACp/GPS loss, 1 deg cells -> event-gnss-interference
  sondehub     api.v2.sondehub.org radiosondes near the UK          -> air-balloon
  celestrak    celestrak.org GP elements, propagated with sgp4      -> space-satellite
  usgs-quakes  earthquake.usgs.gov M2.5+ past day, global           -> event-earthquake
  ea-floods    environment.data.gov.uk current flood warnings       -> event-flood
  digitraffic-ais        meri.digitraffic.fi AIS, Baltic                  -> sea-vessel
  airports-metar         OurAirports + aviationweather METAR and TAF      -> land-airport
  aisstream              stream.aisstream.io AIS, UK + AIS_BOXES (key)    -> sea-vessel
  aviationweather-sigmet aviationweather.gov international SIGMETs, global -> event-sigmet
  metoffice-warnings     Met Office National Severe Weather Warnings      -> event-weather-warning
  crown-estate-wind      Crown Estate (+ Scotland) offshore wind sites    -> sea-windfarm
  nsta-platforms         NSTA UKCS offshore surface installations         -> sea-platform
  noaa-swpc              NOAA SWPC Kp, G/S/R scales, X-ray class          -> space-weather
  tfl-roads              TfL serious/severe London road disruptions       -> land-road-disruption
  gdacs                  GDACS global disaster alerts (keyless)           -> event-disaster
  meteoalarm             Meteoalarm warnings, Balkans/GR/CY/IL (keyless)  -> event-weather-warning
  sg-traffic-cams        data.gov.sg LTA traffic cameras (keyless)        -> land-camera
  nz-traffic-cams        NZTA trafficnz.info cameras (keyless)            -> land-camera
  eccc-alerts [ca-alerts]  ECCC GeoMet weather alerts, Canada (keyless)  -> event-weather-warning
  drivebc-cams [ca-cams]   DriveBC highway webcams, BC (keyless)          -> land-camera
  drivebc-events [regional-extra]  DriveBC Open511 road events (keyless)  -> land-road-disruption
  mrwa-roads [au-traffic]  Main Roads WA incidents and closures (keyless) -> land-road-disruption
  nzta-roads [regional-extra]  NZTA trafficnz.info road events (keyless)  -> land-road-disruption
  metservice-warnings [regional-extra]  MetService NZ CAP warnings        -> event-weather-warning

Areas carry `polygon` as "lat,lon;lat,lon;..." (outer ring, <= 200 vertices, 4 dp) plus a
representative lat/lon. Static layers (wind farms, platforms) are sent once a day with
observedAt = the UTC day start and need a Heimdall TTL of 0 (see ../state/ttl.json.proposed).

Each cycle writes out/<feed>.jsonl and submits it with ../hsource.sh, then logs one line to
feeds.log. Heimdall refuses (and audits) a repeated observation id (FEED-ENTITY-OBSERVEDAT), so
the last observedAt sent per entity is kept under state/ and never resent. Heimdall drops a track
after 300 s without an update, so earthquakes and floods are re-sent every 120 s as a heartbeat
(observedAt = send time; event_time / time_raised carry the real times).

Stdlib only, plus sgp4 (pinned in ../.venv) for the satellites.
"""

import csv
import gzip
import io
import json
import socket
import math
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timedelta, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
POC = os.path.dirname(HERE)
OUT = os.path.join(HERE, "out")
STATE = os.path.join(HERE, "state")
LOG = os.path.join(HERE, "feeds.log")
HSOURCE = os.path.join(POC, "hsource.sh")
MID = os.path.expanduser("~/Desktop/Midgard")  # mounted at /mid in the toolchain container
UA = {"User-Agent": "VeldrisMidgardDemo/0.1 (local reference test)", "Accept": "application/json"}
TIMEOUT = 20

# UK and Ireland, the same box as live-feeds/fetch_live.py uses for adsb.lol
BOX_LAT = (49.0, 61.5)
BOX_LON = (-11.0, 3.0)


class RateLimited(Exception):
    pass


def utc(ts=None):
    return datetime.fromtimestamp(ts if ts is not None else time.time(), timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def get(url, timeout=TIMEOUT):
    req = urllib.request.Request(url, headers=UA)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.load(r)
    except urllib.error.HTTPError as e:
        if e.code == 429:
            raise RateLimited(url) from e
        raise


def iso(ts):
    """ISO-8601 to Unix seconds; a time without an offset is taken as UTC."""
    if not ts:
        return None
    d = datetime.fromisoformat(str(ts).strip().replace("Z", "+00:00"))
    if d.tzinfo is None:
        d = d.replace(tzinfo=timezone.utc)
    return int(d.timestamp())


def clean(text, limit=None):
    if text is None:
        return None
    t = " ".join(str(text).split())
    return (t[:limit] if limit else t) or None


def load(name, default):
    try:
        with open(os.path.join(STATE, name)) as f:
            return json.load(f)
    except (OSError, ValueError):
        return default


def save(name, value):
    path = os.path.join(STATE, name)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".tmp"
    with open(tmp, "w") as f:
        json.dump(value, f)
    os.replace(tmp, path)


def obs(entity, kind, observed_at, attrs):
    return {"entity": entity, "kind": kind, "observedAt": int(observed_at),
            "attrs": {k: v for k, v in attrs.items() if v is not None}}


# ---------------------------------------------------------------- 1. adsb.fi

ADSBFI_CIRCLES = ((51.5, -7.0), (51.5, 0.0), (58.0, -7.0), (58.0, 0.0))  # 250 nm each, UK box filter
# Wider coverage (250 nm circles, no box filter). adsb.fi is a volunteer receiver network:
# coverage is only as good as the local feeders (dense in Europe, the Gulf, Taiwan, Australia's
# east coast; sparse or absent over Iran, Iraq, Syria, Yemen and much of Russia).
ADSBFI_WORLD = (
    ("balkans", 44.5, 18.0), ("balkans", 42.0, 22.0), ("balkans", 45.5, 25.5), ("balkans", 38.5, 23.5),
    ("turkey", 41.0, 29.0), ("turkey", 39.0, 34.5), ("levant", 34.8, 33.8), ("levant", 32.0, 35.5),
    ("egypt", 30.0, 31.5), ("red-sea", 22.5, 38.5), ("gulf", 26.0, 50.8), ("gulf", 24.8, 55.0),
    ("gulf", 29.3, 47.5), ("gulf", 24.7, 46.7), ("iraq", 33.3, 44.4), ("iran", 35.7, 51.4),
    ("black-sea", 43.5, 34.0), ("russia", 55.75, 37.6), ("russia", 59.9, 30.3), ("russia", 54.7, 20.5),
    ("taiwan", 25.0, 121.5), ("taiwan", 23.0, 119.5), ("singapore-malaysia", 2.2, 103.0),
    ("australia", -33.9, 151.2), ("australia", -37.8, 145.0), ("australia", -27.5, 153.0),
    ("new-zealand", -37.0, 174.8), ("new-zealand", -41.3, 174.8),
    ("canada", 43.7, -79.4), ("canada", 45.5, -73.6), ("canada", 49.2, -123.1),
    ("india", 28.6, 77.2), ("india", 19.1, 72.9), ("south-africa", -26.2, 28.0),
    ("kenya", -1.3, 36.8), ("nigeria", 6.5, 3.4), ("pakistan", 24.9, 67.0),
    # Russia beyond Moscow/St Petersburg/Kaliningrad: probed twice (2026-10-18) and kept only
    # where adsb.fi returned aircraft. Dropped for returning nothing (no feeders): Arkhangelsk,
    # Kazan, Samara, Rostov-on-Don, Yekaterinburg, Novosibirsk, Irkutsk, Khabarovsk,
    # Vladivostok, Kamchatka.
    ("ru-murmansk", 68.97, 33.08), ("ru-krasnodar-sochi", 44.2, 39.6), ("ru-crimea", 45.0, 34.1),
    ("belarus", 53.9, 27.57), ("baltic-east", 57.5, 28.0),
)


def num(v, f=float):
    try:
        return f(v)
    except (TypeError, ValueError):
        return None


ADSBFI_MIL = "https://opendata.adsb.fi/api/v2/mil"


def src_adsb_fi():
    """Aircraft over the UK and Ireland plus the wider regions in ADSBFI_WORLD; same entity
    key as adsb-lol so Heimdall fuses them. Deduplicated by hex (overlapping circles)."""
    seen, per = {}, {}
    queries = [("uk", lat, lon) for lat, lon in ADSBFI_CIRCLES] + list(ADSBFI_WORLD)
    queries.append(("mil", None, None))  # every aircraft adsb.fi flags as military, worldwide
    errors = 0
    for i, (region, lat, lon) in enumerate(queries):
        if i:
            time.sleep(1.1)  # the open API asks for at most one request a second
        try:
            d = get(ADSBFI_MIL if region == "mil" else f"https://opendata.adsb.fi/api/v2/lat/{lat}/lon/{lon}/dist/250")
        except RateLimited:
            if i == 0:
                raise
            log_line("adsb-fi", f"429 at circle {i} ({region}); stopping this cycle early")
            time.sleep(5)
            break
        except Exception as e:  # one circle failing must not lose the rest
            errors += 1
            if errors >= 5:
                raise
            log_line("adsb-fi", f"circle {region} {lat},{lon} failed ({type(e).__name__}: {e})")
            continue
        now = float(d.get("now") or time.time())
        if now > 1e11:  # milliseconds
            now /= 1000.0
        for a in d.get("aircraft") or d.get("ac") or []:
            if a.get("lat") is None or a.get("lon") is None or not a.get("hex"):
                continue
            if region == "uk" and not (BOX_LAT[0] <= a["lat"] <= BOX_LAT[1] and BOX_LON[0] <= a["lon"] <= BOX_LON[1]):
                continue
            if a["hex"] not in seen:
                per[region] = per.get(region, 0) + 1
            seen[a["hex"]] = (a, now)
    out = []
    for hexid, (a, now) in seen.items():
        alt = a.get("alt_baro")
        on_ground = alt == "ground"
        alt_ft = 0 if on_ground else (int(alt) if isinstance(alt, (int, float)) else None)
        age = a.get("seen_pos") if a.get("seen_pos") is not None else (a.get("seen") or 0)
        adsb = str(a.get("type", "")).startswith("adsb")
        attrs = {"callsign": clean(a.get("flight")), "lat": float(a["lat"]), "lon": float(a["lon"]),
                 "alt_ft": alt_ft, "alt_m": round(alt_ft * 0.3048, 1) if alt_ft is not None else None,
                 # MLAT positions are estimated by ground receivers, and their ground speed is
                 # unreliable (1,000+ kt seen at 500 ft): keep the speed only from the aircraft's
                 # own broadcast (adsb_*), and say where every position came from.
                 "speed": num(a.get("gs")) if str(a.get("type", "")).startswith("adsb") else None,
                 "pos_source": clean(a.get("type")),
                 "track": num(a.get("track")), "squawk": a.get("squawk") or None,
                 "on_ground": on_ground, "category": a.get("category") or None, "source": "adsb.fi",
                 # avionics detail (adsb.fi only; the adsb.lol CSV lacks these): ICAO type and
                 # registration, navigation integrity / accuracy categories, and whether the
                 # aircraft has lost GPS (adsb.fi keeps `gpsOkBefore` after a GPS loss). NIC and
                 # NACp only for ADS-B positions: adsb.fi gives MLAT aircraft nic 0 / nac_p 0,
                 # which says nothing about their GNSS and would read as degraded.
                 "aircraft_type": clean(a.get("t")), "registration": clean(a.get("r")),
                 "nic": num(a.get("nic"), int) if adsb else None,
                 "nac_p": num(a.get("nac_p"), int) if adsb else None,
                 "gps_lost": True if "gpsOkBefore" in a else None,
                 # adsb.fi's database flag bit 0: a military registration or hex block
                 "military": True if (num(a.get("dbFlags"), int) or 0) & 1 else None}
        if (attrs["speed"] or 0) > 700:
            # Watch for implausible ground speeds (one showed 1,327 kt in the console).
            with open(os.path.join(HERE, "outliers.log"), "a") as f:
                f.write(json.dumps({"t": int(time.time()), "feed": "adsb.fi", "hex": hexid, "raw": a}) + "\n")
        out.append(obs(hexid.upper().replace("~", "X"), "air-track", int(now - float(age)), attrs))
    gnss_record(seen)
    gulf_air_record(seen)
    return f"{len(seen)} [{' '.join(f'{k}={v}' for k, v in per.items())}]", out


# ------------------------------------------------- 1b. GNSS interference (derived from adsb.fi)
#
# The same method the public trackers use (gpsjam.org): aircraft broadcasting ADS-B report how
# good their own GNSS position is (NIC, the integrity containment radius, and NACp, the position
# accuracy). Bin airborne ADS-B aircraft into 1 x 1 degree cells and look at the share reporting
# NIC 0 or NACp 0 or a lost GPS fix. Jamming and spoofing both show up this way, and so do a few
# aircraft with poor or misconfigured avionics, hence the minimum counts below. MLAT positions are
# excluded (they are computed on the ground and say nothing about the aircraft's GNSS).

GNSS_WINDOW = 15 * 60  # rolling window of per-cycle cell counts, so single cycles do not flicker
GNSS = {"cycles": []}  # [(cycle time, {(lat, lon): {hex: degraded}})], filled by src_adsb_fi


def gnss_record(seen):
    """Per-cell aircraft and degraded flags for one adsb.fi cycle (all circles, worldwide)."""
    cells = {}
    for hexid, (a, _) in seen.items():
        if not str(a.get("type", "")).startswith("adsb") or a.get("alt_baro") == "ground":
            continue
        cell = (math.floor(float(a["lat"])), math.floor(float(a["lon"])))
        cells.setdefault(cell, {})[hexid] = (a.get("nic") == 0 or a.get("nac_p") == 0 or "gpsOkBefore" in a)
    now = time.time()
    GNSS["cycles"] = [c for c in GNSS["cycles"] if now - c[0] <= GNSS_WINDOW] + [(now, cells)]


def src_gnss():
    """Cells with likely GNSS interference over the last 15 min of adsb.fi cycles. A cell needs
    >= 3 distinct aircraft and >= 2 distinct degraded ones; share = degraded reports / reports,
    summed over the window's cycles (so a cell is smoothed, not decided by one cycle)."""
    now = time.time()
    cycles = [c for c in GNSS["cycles"] if now - c[0] <= GNSS_WINDOW]
    if not cycles:
        return "no adsb-fi cycle in the last 15 min", []
    agg = {}  # cell -> [reports, degraded reports, hexes, degraded hexes]
    for _, cells in cycles:
        for cell, acs in cells.items():
            g = agg.setdefault(cell, [0, 0, set(), set()])
            for hexid, bad in acs.items():
                g[0] += 1
                g[2].add(hexid)
                if bad:
                    g[1] += 1
                    g[3].add(hexid)
    t = int(now)
    out = []
    for (la, lo), (reports, bad_reports, hexes, bad_hexes) in agg.items():
        share = bad_reports / reports
        if len(hexes) < 3 or len(bad_hexes) < 2 or share < 0.15:
            continue
        ring = [(la, lo), (la, lo + 1), (la + 1, lo + 1), (la + 1, lo)]
        out.append(obs(f"GNSS-{la}-{lo}", "event-gnss-interference", t, {
            "polygon": ";".join(f"{y},{x}" for y, x in ring), "lat": la + 0.5, "lon": lo + 0.5,
            "aircraft": len(hexes), "degraded": len(bad_hexes), "share": round(share, 3),
            "level": "high" if share >= 0.5 else "medium" if share >= 0.25 else "low",
            "source": "derived-adsb-fi",
            "method": "share of aircraft in the cell reporting NIC 0 or NACp 0 or lost GPS (adsb.fi)"}))
    return f"{len(agg)} cells from {len(cycles)} cycles, {len(out)} flagged", out


# ---------------------------------------------------------------- 2. SondeHub

def src_sondehub():
    """Radiosondes within ~900 km of 54N 3W whose last frame is under 30 minutes old."""
    d = get("https://api.v2.sondehub.org/sondes?lat=54&lon=-3&distance=900000&last=1800")
    now = time.time()
    out = []
    for serial, s in (d or {}).items():
        t = iso(s.get("datetime"))
        if t is None or now - t > 1800 or s.get("lat") is None or s.get("lon") is None:
            continue
        attrs = {"lat": float(s["lat"]), "lon": float(s["lon"]),
                 "alt_m": round(float(s["alt"]), 1) if s.get("alt") is not None else None,
                 "vel_v": num(s.get("vel_v")), "vel_h": num(s.get("vel_h")), "heading": num(s.get("heading")),
                 "type": clean(s.get("type")),
                 "frequency": num(s.get("frequency")), "launch_site": clean(s.get("launch_site")),
                 "source": "sondehub"}
        out.append(obs(clean(s.get("serial")) or serial, "air-balloon", t, attrs))
    # only frames that changed are resent: send() skips an entity whose observedAt was already sent
    return len(d or {}), out


# ---------------------------------------------------------------- 3. CelesTrak + sgp4

# glo-ops (operational GLONASS) last: its satellites are mostly in gnss already, and the first
# group listed wins, so the gnss-satellite rule keeps matching them
CELESTRAK_GROUPS = ("stations", "visual", "weather", "gnss", "glo-ops")
CELESTRAK_REFRESH = 2 * 3600  # CelesTrak asks for no more than one download per group per 2 h
WGS84_A = 6378.137
WGS84_F = 1 / 298.257223563


def celestrak_elements():
    """GP elements per group, from the disk cache, refreshed at most every 2 hours."""
    meta = load("celestrak/meta.json", {})
    groups = {}
    for g in CELESTRAK_GROUPS:
        cached = load(f"celestrak/{g}.json", None)
        if cached is None or time.time() - meta.get(g, 0) >= CELESTRAK_REFRESH:
            try:
                data = get(f"https://celestrak.org/NORAD/elements/gp.php?GROUP={g}&FORMAT=json", timeout=30)
                if not isinstance(data, list) or not data:
                    raise ValueError(f"unexpected response for group {g}")
                save(f"celestrak/{g}.json", data)
                cached = data
            except Exception as e:  # keep using a stale cache rather than nothing
                log_line("celestrak", f"group {g} download failed ({type(e).__name__}: {e}); "
                         f"{'using stale cache' if cached else 'no cache'}")
            # a failed attempt also counts, so a refusing server is not hammered every minute
            meta[g] = time.time()
            save("celestrak/meta.json", meta)
        groups[g] = cached or []
    return groups


def gmst_rad(jd_ut1):
    """Greenwich mean sidereal time, IAU-82 (Vallado, Fundamentals of Astrodynamics, eq. 3-47)."""
    t = (jd_ut1 - 2451545.0) / 36525.0
    sec = 67310.54841 + (876600.0 * 3600 + 8640184.812866) * t + 0.093104 * t * t - 6.2e-6 * t ** 3
    return math.radians((sec % 86400.0) / 240.0)


def teme_to_geodetic(r, jd):
    """TEME position (km) -> geodetic lat, lon (deg), height (km) on WGS-84.

    TEME is rotated to the pseudo-Earth-fixed frame by GMST about z (polar motion and the
    UT1-UTC difference are ignored: errors well under 1 km), then converted to geodetic
    coordinates with Bowring-style fixed-point iteration on latitude."""
    th = gmst_rad(jd)
    x = r[0] * math.cos(th) + r[1] * math.sin(th)
    y = -r[0] * math.sin(th) + r[1] * math.cos(th)
    z = r[2]
    lon = math.atan2(y, x)
    p = math.hypot(x, y)
    e2 = WGS84_F * (2 - WGS84_F)
    lat = math.atan2(z, p * (1 - e2))
    for _ in range(6):
        n = WGS84_A / math.sqrt(1 - e2 * math.sin(lat) ** 2)
        h = p / math.cos(lat) - n
        lat = math.atan2(z, p * (1 - e2 * n / (n + h)))
    n = WGS84_A / math.sqrt(1 - e2 * math.sin(lat) ** 2)
    h = p / math.cos(lat) - n
    return math.degrees(lat), (math.degrees(lon) + 540) % 360 - 180, h


_SATS = {}  # (norad, epoch) -> Satrec, so elements are parsed once per refresh


def satrec(fields):
    from sgp4.api import Satrec
    from sgp4 import omm
    key = (fields.get("NORAD_CAT_ID"), fields.get("EPOCH"))
    if key not in _SATS:
        f = dict(fields)
        if "." not in str(f["EPOCH"]):
            f["EPOCH"] = f"{f['EPOCH']}.000000"
        sat = Satrec()
        omm.initialize(sat, f)
        _SATS[key] = sat
    return _SATS[key]


def src_celestrak():
    """Every satellite in the groups, propagated to now (global)."""
    groups = celestrak_elements()
    now = int(time.time())
    jd_full = now / 86400.0 + 2440587.5
    jd, fr = math.floor(jd_full), jd_full - math.floor(jd_full)
    out, seen, total = [], set(), 0
    glonass = {str(f.get("NORAD_CAT_ID")) for f in groups.get("glo-ops", [])}
    for g in CELESTRAK_GROUPS:
        for fields in groups[g]:
            total += 1
            try:
                norad = int(fields["NORAD_CAT_ID"])
                if norad in seen:
                    continue  # in more than one group: the first group listed wins
                err, r, v = satrec(fields).sgp4(jd, fr)
                if err != 0:
                    continue  # decayed or otherwise unpropagatable
                lat, lon, h = teme_to_geodetic(r, jd_full)
            except Exception:
                continue
            seen.add(norad)
            attrs = {"name": clean(fields.get("OBJECT_NAME")), "norad_id": norad, "group": g,
                     "lat": round(lat, 4), "lon": round(lon, 4), "alt_m": round(h * 1000.0, 0),
                     "alt_km": round(h, 2), "speed_kms": round(math.sqrt(sum(c * c for c in v)), 3),
                     # in CelesTrak's operational GLONASS group (glo-ops)
                     "glonass_operational": True if str(norad) in glonass else None,
                     "source": "celestrak"}
            out.append(obs(f"NORAD-{norad}", "space-satellite", now, attrs))
    return total, out


# ---------------------------------------------------------------- 4. USGS earthquakes

def src_usgs():
    """M2.5+ earthquakes in the past day, refetched every 5 min, heartbeat every 120 s."""
    cache = load("usgs.json", {"fetched": 0, "events": {}})
    fetched = "cached"  # a heartbeat cycle: no refetch due
    if time.time() - cache["fetched"] >= 300:
        d = get("https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/2.5_day.geojson")
        events = {}
        for f in d.get("features", []):
            p, g = f.get("properties") or {}, (f.get("geometry") or {}).get("coordinates") or []
            if len(g) < 3 or p.get("time") is None or not f.get("id"):
                continue
            depth = float(g[2] or 0.0)
            events[f["id"]] = {"lat": float(g[1]), "lon": float(g[0]), "alt_m": round(-depth * 1000.0, 1),
                               "depth_km": depth, "mag": num(p.get("mag")), "place": clean(p.get("place"), 200),
                               "event_time": int(p["time"] / 1000), "url": p.get("url"),
                               "alert": p.get("alert") or None, "tsunami": bool(p.get("tsunami")),
                               "source": "usgs"}
        cache = {"fetched": time.time(), "events": events}
        save("usgs.json", cache)
        fetched = len(events)
    now = int(time.time())
    return fetched, [obs(k, "event-earthquake", now, a) for k, a in cache["events"].items()]


# ---------------------------------------------------------------- 5. Environment Agency floods

EA = "https://environment.data.gov.uk/flood-monitoring"


def flood_area_centroid(notation, areas):
    if notation not in areas:
        d = get(f"{EA}/id/floodAreas/{urllib.parse.quote(notation)}")
        it = d.get("items") or {}
        if isinstance(it, list):
            it = it[0] if it else {}
        areas[notation] = [it.get("lat"), it.get("long")]
        save("flood_areas.json", areas)
    return areas[notation]


def src_floods():
    """Current flood alerts and warnings (England), refetched every 10 min, heartbeat every 120 s."""
    cache = load("floods.json", {"fetched": 0, "warnings": {}})
    fetched = "cached"  # a heartbeat cycle: no refetch due
    if time.time() - cache["fetched"] >= 600:
        d = get(f"{EA}/id/floods")
        areas = load("flood_areas.json", {})
        warnings = {}
        for it in d.get("items", []):
            fa = it.get("floodArea") or {}
            key = it.get("floodAreaID") or fa.get("notation")
            if not key:
                continue
            lat, lon = flood_area_centroid(key, areas)
            if lat is None or lon is None:
                continue
            warnings[key] = {"lat": float(lat), "lon": float(lon), "severity": clean(it.get("severity")),
                             "severity_level": num(it.get("severityLevel"), int),
                             "area": clean(it.get("description"), 200), "county": clean(fa.get("county"), 200),
                             "river": clean(fa.get("riverOrSea"), 200), "time_raised": iso(it.get("timeRaised")),
                             "message_changed": iso(it.get("timeMessageChanged")),
                             "source": "environment-agency"}
        cache = {"fetched": time.time(), "warnings": warnings}
        save("floods.json", cache)
        fetched = len(warnings)
    now = int(time.time())
    return fetched, [obs(k, "event-flood", now, a) for k, a in cache["warnings"].items()]


# ---------------------------------------------------------------- runner

# name -> (feed identity, function, seconds between cycles)
# ---------------------------------------------------------------- 6. Digitraffic AIS (Finland)

AIS_TYPES = [((30, 30), "fishing"), ((31, 32), "towing"), ((33, 34), "dredging/diving"), ((35, 35), "military"),
             ((36, 36), "sailing"), ((37, 37), "pleasure craft"), ((40, 49), "high-speed craft"), ((50, 50), "pilot"),
             ((51, 51), "search and rescue"), ((52, 52), "tug"), ((53, 59), "port/law/medical"),
             ((60, 69), "passenger"), ((70, 79), "cargo"), ((80, 89), "tanker")]
AIS_STATUS = {0: "under way", 1: "at anchor", 2: "not under command", 3: "restricted manoeuvrability",
              4: "constrained by draught", 5: "moored", 6: "aground", 7: "fishing", 8: "sailing", 14: "AIS-SART"}


def digitraffic(path):
    """Fintraffic's open marine API: no key, but it asks for a Digitraffic-User header and gzip."""
    req = urllib.request.Request("https://meri.digitraffic.fi" + path, headers={
        "Digitraffic-User": "veldris-midgard-heimdall-poc", "Accept-Encoding": "gzip", "Accept": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
            body = r.read()
            return json.loads(gzip.decompress(body) if r.headers.get("Content-Encoding") == "gzip" else body)
    except urllib.error.HTTPError as e:
        if e.code == 429:
            raise RateLimited(path) from e
        raise


def src_digitraffic():
    """Ships in the Baltic (Finnish AIS network) reported in the last 5 minutes; vessel names
    and types from the vessel list, refreshed at most every 30 minutes."""
    now = time.time()
    meta = load("digitraffic_vessels.json", {"at": 0, "by_mmsi": {}})
    if now - meta["at"] > 1800:
        meta = {"at": now, "by_mmsi": {str(v["mmsi"]): v for v in digitraffic("/api/ais/v1/vessels")}}
        save("digitraffic_vessels.json", meta)
    d = digitraffic(f"/api/ais/v1/locations?from={int((now - 300) * 1000)}")
    out = []
    for f in d.get("features", []):
        p = f["properties"]
        lon, lat = f["geometry"]["coordinates"]
        if not (-90 <= lat <= 90 and -180 <= lon <= 180) or (lat == 91 or lon == 181):
            continue
        v = meta["by_mmsi"].get(str(p["mmsi"]), {})
        st = v.get("shipType")
        sog, cog, hdg = p.get("sog"), p.get("cog"), p.get("heading")
        attrs = {"lat": float(lat), "lon": float(lon), "alt_m": 0.0, "mmsi": int(p["mmsi"]),
                 "speed": float(sog) if sog is not None and sog < 102.3 else None,
                 "track": float(cog) if cog is not None and cog < 360 else None,
                 "heading": float(hdg) if hdg is not None and hdg < 360 else None,
                 "nav_status": AIS_STATUS.get(p.get("navStat")),
                 "name": clean(v.get("name")), "callsign": clean(v.get("callSign")),
                 "ship_type": next((label for (lo, hi), label in AIS_TYPES if st is not None and lo <= st <= hi), None),
                 "ship_type_code": int(st) if st is not None else None,
                 "destination": clean(v.get("destination")), "imo": int(v["imo"]) if v.get("imo") else None,
                 "source": "digitraffic"}
        out.append(obs(f"MMSI-{p['mmsi']}", "sea-vessel", p["timestampExternal"] / 1000, attrs))
    return len(d.get("features", [])), out


# ---------------------------------------------------------------- 7. Airports + METAR

AIRPORTS_RAW = os.path.join(MID, "airports-data", "raw")


# Beyond GB/IE (all large + medium, as before): every large airport in these countries, plus
# medium airports with scheduled service in countries with few large ones; capped at AIRPORTS_CAP.
AIRPORT_COUNTRIES = (
    "HR SI BA RS ME MK AL XK BG RO GR "                                   # Balkans
    "TR CY IL PS JO LB SY IQ IR SA AE QA BH KW OM YE EG "                 # Middle East
    "RU BY TW "
    "AU NZ CA IN SG MY ZA KE NG PK BD LK MT GH TZ UG BN FJ MU").split()  # Commonwealth
AIRPORT_MEDIUM_ALWAYS = ("RU", "BY")  # also every scheduled medium airport, however many large ones
AIRPORTS_CAP = 600
AIRPORTS_VERSION = 3  # bump to rebuild state/airports.json after changing the selection


def airport_list():
    """UK and Irish large and medium airports, plus large (and, for countries with few large
    ones, scheduled medium) airports in AIRPORT_COUNTRIES (OurAirports, public domain) with
    their runways, read once from the local copy in airports-data/raw."""
    cached = load("airports.json", None)
    if isinstance(cached, dict) and cached.get("version") == AIRPORTS_VERSION:
        return cached["ports"]
    rows = []
    with open(os.path.join(AIRPORTS_RAW, "airports.csv")) as f:
        rows = [r for r in csv.DictReader(f) if r["type"] in ("large_airport", "medium_airport") and r["ident"]]
    large = {}
    for r in rows:
        if r["type"] == "large_airport":
            large[r["iso_country"]] = large.get(r["iso_country"], 0) + 1
    picked, extra = [], []
    for r in rows:
        c = r["iso_country"]
        if c in ("GB", "IE"):
            picked.append(r)
        elif c in AIRPORT_COUNTRIES and re.fullmatch(r"[A-Z]{4}", r["ident"]):
            if r["type"] == "large_airport":
                extra.append((0, r))
            elif (large.get(c, 0) <= 4 or c in AIRPORT_MEDIUM_ALWAYS) and r["scheduled_service"] == "yes":
                extra.append((1, r))
    extra.sort(key=lambda x: x[0])  # large first, then the small-country medium ones
    picked += [r for _, r in extra[:max(0, AIRPORTS_CAP - len(picked))]]
    ports = {}
    for r in picked:
        ports[r["ident"]] = {"icao": r["ident"], "name": r["name"], "iata": r["iata_code"] or None,
                             "type": r["type"].replace("_airport", ""), "municipality": r["municipality"] or None,
                             "country": r["iso_country"], "lat": float(r["latitude_deg"]),
                             "lon": float(r["longitude_deg"]),
                             "elevation_ft": int(r["elevation_ft"]) if r["elevation_ft"] else None,
                             "runways": []}
    with open(os.path.join(AIRPORTS_RAW, "runways.csv")) as f:
        for r in csv.DictReader(f):
            p = ports.get(r["airport_ident"])
            if not p or r["closed"] == "1" or not all(r[k] for k in ("le_latitude_deg", "le_longitude_deg",
                                                                        "he_latitude_deg", "he_longitude_deg")):
                continue
            p["runways"].append({"name": f'{r["le_ident"]}/{r["he_ident"]}', "length_ft": int(r["length_ft"] or 0),
                                 "width_ft": int(r["width_ft"] or 0),
                                 "ends": [round(float(r["le_latitude_deg"]), 5), round(float(r["le_longitude_deg"]), 5),
                                          round(float(r["he_latitude_deg"]), 5), round(float(r["he_longitude_deg"]), 5)]})
    out = list(ports.values())
    save("airports.json", {"version": AIRPORTS_VERSION, "ports": out})
    return out


def aw_batch(kind, ports):
    """METAR or TAF rows for every airport, in requests of up to 100 ids."""
    ids = [p["icao"] for p in ports]
    rows = []
    for i in range(0, len(ids), 100):
        if i:
            time.sleep(1)
        rows += get(f"https://aviationweather.gov/api/data/{kind}?ids={','.join(ids[i:i + 100])}&format=json", timeout=60) or []
    return rows


# Flight category thresholds (FAA): LIFR ceiling < 500 ft or visibility < 1 sm; IFR < 1,000 ft
# or < 3 sm; MVFR <= 3,000 ft or <= 5 sm; otherwise VFR. The ceiling is the lowest BKN, OVC
# (or OVX) layer, or the vertical visibility when the sky is obscured.
CATEGORY_RANK = {"VFR": 0, "MVFR": 1, "IFR": 2, "LIFR": 3}


def flight_category(vis_sm, ceiling_ft):
    v = vis_sm if vis_sm is not None else 99.0
    c = ceiling_ft if ceiling_ft is not None else 99999
    if c < 500 or v < 1:
        return "LIFR"
    if c < 1000 or v < 3:
        return "IFR"
    if c <= 3000 or v <= 5:
        return "MVFR"
    return "VFR"


def taf_vis(v):
    """aviationweather visibility ('6+', 4.35, '1/2', 'P6SM') -> statute miles, or None."""
    if v is None:
        return None
    if isinstance(v, (int, float)):
        return float(v)
    t = str(v).upper().replace("SM", "").replace("P", "").replace("+", "").strip()
    try:
        if " " in t:
            whole, frac = t.split(" ", 1)
            n, d = frac.split("/")
            return float(whole) + float(n) / float(d)
        if "/" in t:
            n, d = t.split("/")
            return float(n) / float(d)
        return float(t)
    except (ValueError, ZeroDivisionError):
        return None


def taf_ceiling(group):
    """Lowest BKN/OVC/OVX base in ft, the vertical visibility if given, None for no ceiling,
    or the string 'unset' when the group says nothing about cloud (so it is inherited)."""
    clouds = group.get("clouds") or []
    if group.get("vertVis") is not None:
        return float(group["vertVis"])
    if not clouds:
        return "unset"
    bases = [float(c["base"]) for c in clouds if c.get("cover") in ("BKN", "OVC", "OVX") and c.get("base") is not None]
    return min(bases) if bases else None


def taf_worst_12h(taf, now):
    """The worst flight category a TAF forecasts in the next 12 h.

    Method: walk the decoded groups in order, keeping the prevailing conditions (visibility,
    ceiling). The base forecast and FM groups replace them; BECMG groups update the elements
    they give and, as the prevailing state from then on, are evaluated until the next FM;
    TEMPO and PROB groups are evaluated with any element they omit inherited from the
    prevailing state, and never change it. Every group whose period overlaps [now, now + 12 h]
    is categorised with the thresholds above, and the worst one wins. Conservative by design:
    a TEMPO or PROB30 deterioration counts the same as the prevailing forecast."""
    end = now + 12 * 3600
    groups = taf.get("fcsts") or []
    prevailing = {"vis": None, "ceil": None}
    worst = None
    for i, g in enumerate(groups):
        change = (g.get("fcstChange") or "").upper()
        vis, ceil = taf_vis(g.get("visib")), taf_ceiling(g)
        state = {"vis": vis if vis is not None else prevailing["vis"],
                 "ceil": prevailing["ceil"] if ceil == "unset" else ceil}
        t0, t1 = g.get("timeFrom") or 0, g.get("timeTo") or 0
        if change in ("", "FM", "BECMG"):
            if change in ("", "FM"):
                state = {"vis": vis, "ceil": None if ceil == "unset" else ceil}
            prevailing = state
            if change == "BECMG":  # persists until the next FM (or the end of the TAF)
                t1 = next((h.get("timeFrom") for h in groups[i + 1:] if (h.get("fcstChange") or "").upper() == "FM"),
                          taf.get("validTimeTo") or t1)
        if t1 <= now or t0 >= end:
            continue
        cat = flight_category(state["vis"], state["ceil"])
        if worst is None or CATEGORY_RANK[cat] > CATEGORY_RANK[worst]:
            worst = cat
    return worst


def src_airports():
    """Each airport with its latest METAR and TAF (NOAA aviationweather.gov, public domain, no
    key): METARs refetched every 10 min, TAFs every 30 min; every airport resent every 120 s
    as a heartbeat."""
    ports = airport_list()
    now = time.time()
    wx = load("metar.json", {"at": 0, "by_icao": {}})
    fetched = "cached"
    if now - wx["at"] > 600:
        rows = aw_batch("metar", ports)
        wx = {"at": now, "by_icao": {r["icaoId"]: r for r in rows}}
        save("metar.json", wx)
        fetched = len(rows)
    tafs = load("taf.json", {"at": 0, "by_icao": {}})
    if now - tafs["at"] > 1800:
        try:
            rows = aw_batch("taf", ports)
            tafs = {"at": now, "by_icao": {r["icaoId"]: r for r in rows if r.get("icaoId")}}
        except Exception as e:  # keep the METARs flowing; retry the TAFs next cycle
            log_line("airports-metar", f"TAF fetch failed ({type(e).__name__}: {e}); using cache")
            tafs["at"] = now - 1800 + 300  # retry in 5 min, not every cycle
        save("taf.json", tafs)
        fetched = f"{fetched} taf={len(tafs['by_icao'])}"
    out = []
    for p in ports:
        m = wx["by_icao"].get(p["icao"], {})
        tf = tafs["by_icao"].get(p["icao"], {})
        clouds = " ".join(f'{c.get("cover")}{int(c["base"]) // 100:03d}' if c.get("base") is not None else str(c.get("cover"))
                          for c in m.get("clouds") or [])
        attrs = {"lat": p["lat"], "lon": p["lon"], "name": p["name"], "icao": p["icao"], "iata": p["iata"],
                 "airport_type": p["type"], "municipality": p["municipality"], "country": p["country"],
                 "elevation_ft": p["elevation_ft"],
                 "alt_m": round(p["elevation_ft"] * 0.3048, 1) if p["elevation_ft"] is not None else None,
                 "runway_count": len(p["runways"]),
                 "longest_runway_ft": max((r["length_ft"] for r in p["runways"]), default=None),
                 "runway_names": ", ".join(r["name"] for r in p["runways"]) or None,
                 # runways for the 3D view: lat,lon,lat,lon,width_m;...
                 "runways_geo": ";".join(",".join(str(x) for x in r["ends"] + [round(r.get("width_ft", 0) * 0.3048)])
                                         for r in p["runways"]) or None,
                 "flight_category": m.get("fltCat") or None,
                 "temp_c": num(m.get("temp")), "dewpoint_c": num(m.get("dewp")),
                 "wind_dir": num(m.get("wdir")) if isinstance(m.get("wdir"), (int, float)) else None,
                 "wind_variable": m.get("wdir") == "VRB" or None,
                 "wind_kt": num(m.get("wspd")), "gust_kt": num(m.get("wgst")),
                 "visibility": clean(m.get("visib")), "qnh_hpa": num(m.get("altim")),
                 "clouds": clouds or None, "metar": clean(m.get("rawOb")),
                 "metar_time": int(m["obsTime"]) if m.get("obsTime") else None,
                 "taf": clean(tf.get("rawTAF")), "taf_issued": iso(tf.get("issueTime")),
                 "taf_worst_category_12h": taf_worst_12h(tf, now) if tf else None,
                 "source": "ourairports+aviationweather"}
        out.append(obs(p["icao"], "land-airport", now, attrs))
    return fetched, out


# ---------------------------------------------------------------- 8. AISStream (UK waters)

AISSTREAM_KEY = os.path.join(HERE, "secrets", "aisstream.key")  # local only, never committed
AIS_BOX = [[48.0, -12.0], [62.0, 4.0]]  # UK and Ireland, lat/lon corners
AIS_BOXES = [  # [[lat, lon], [lat, lon]] corners; AISStream coverage is volunteer receivers too
    AIS_BOX,
    [[30.0, -6.0], [46.0, 36.5]],     # Mediterranean (Adriatic, Aegean, Levant, Cyprus)
    [[40.5, 27.0], [47.5, 42.0]],     # Black Sea
    [[12.0, 32.0], [30.0, 44.0]],     # Red Sea and Suez
    [[10.5, 43.0], [15.0, 52.0]],     # Bab el-Mandeb, Gulf of Aden
    [[22.0, 47.5], [30.5, 60.0]],     # Persian Gulf, Strait of Hormuz, Gulf of Oman
    [[20.5, 117.0], [27.0, 123.5]],   # Taiwan Strait and around Taiwan
    [[0.5, 98.0], [6.5, 105.0]],      # Malacca and Singapore Straits
    [[-44.0, 145.0], [-10.0, 155.0]],  # eastern Australia
    [[-48.0, 165.0], [-33.0, 179.9]],  # New Zealand
    [[42.0, -71.0], [52.0, -50.0]],   # Canada east coast, St Lawrence
    [[47.0, -132.0], [55.0, -122.0]],  # Canada west coast
    # Russia (some overlap the Black Sea box: listed separately so their volumes can be reported)
    [[68.0, 25.0], [72.5, 45.0]],     # Barents Sea, Kola (Murmansk, Severomorsk)
    [[63.5, 32.0], [67.0, 45.0]],     # White Sea (Arkhangelsk, Severodvinsk)
    [[54.2, 18.5], [56.0, 21.6]],     # Kaliningrad approaches (Baltiysk)
    [[45.0, 34.5], [47.5, 39.5]],     # Sea of Azov, Kerch Strait
    [[41.0, 36.0], [45.5, 42.0]],     # eastern Black Sea (Novorossiysk, Tuapse, Sochi)
    [[36.5, 46.5], [47.5, 55.0]],     # Caspian Sea
    [[41.0, 129.5], [44.0, 135.0]],   # Peter the Great Gulf, Sea of Japan (Vladivostok, Nakhodka)
]
AIS_RU_BOXES = dict(zip(("barents-kola", "white-sea", "kaliningrad", "azov", "east-black-sea", "caspian",
                         "peter-the-great-gulf"), AIS_BOXES[-7:]))
AIS = {"positions": {}, "static": {}, "thread": None, "status": "not started", "msgs": 0, "box_msgs": {}}


def ais_ru_box(lat, lon):
    return [n for n, ((s, w), (n_, e)) in AIS_RU_BOXES.items()
            if lat is not None and lon is not None and s <= lat <= n_ and w <= lon <= e]


def ais_stream():
    """Keep one AISStream WebSocket open (reconnecting with back-off) and remember each
    ship's latest position and its static data (name, type, destination)."""
    import asyncio
    import websockets  # pinned in the venv: websockets==13.1

    key = open(AISSTREAM_KEY).read().strip()

    async def run():
        wait = 5
        while True:
            try:
                async with websockets.connect("wss://stream.aisstream.io/v0/stream", open_timeout=20, ping_interval=30) as ws:
                    await ws.send(json.dumps({"APIKey": key, "BoundingBoxes": AIS_BOXES, "FilterMessageTypes": [
                        "PositionReport", "StandardClassBPositionReport", "ShipStaticData", "StaticDataReport"]}))
                    AIS["status"] = "connected"
                    wait = 5
                    async for raw in ws:
                        AIS["msgs"] += 1
                        m = json.loads(raw)
                        meta, kind = m.get("MetaData", {}), m.get("MessageType")
                        mmsi = meta.get("MMSI")
                        body = (m.get("Message") or {}).get(kind) or {}
                        if not mmsi:
                            continue
                        if kind in ("PositionReport", "StandardClassBPositionReport"):
                            ts = str(meta.get("time_utc", ""))[:19]
                            t = iso(ts.replace(" ", "T")) if ts else int(time.time())
                            AIS["positions"][mmsi] = {"t": t, "lat": body.get("Latitude"), "lon": body.get("Longitude"),
                                                      "sog": body.get("Sog"), "cog": body.get("Cog"),
                                                      "hdg": body.get("TrueHeading"), "nav": body.get("NavigationalStatus"),
                                                      "name": meta.get("ShipName")}
                            for b in ais_ru_box(body.get("Latitude"), body.get("Longitude")):
                                AIS["box_msgs"][b] = AIS["box_msgs"].get(b, 0) + 1
                        elif kind == "ShipStaticData":
                            AIS["static"][mmsi] = {"name": body.get("Name"), "type": body.get("Type"),
                                                   "dest": body.get("Destination"), "call": body.get("CallSign"),
                                                   "imo": body.get("ImoNumber")}
                        elif kind == "StaticDataReport":
                            s = AIS["static"].setdefault(mmsi, {})
                            a = body.get("ReportA") or {}
                            b = body.get("ReportB") or {}
                            if a.get("Valid") and a.get("Name"):
                                s["name"] = a["Name"]
                            if b.get("Valid"):
                                s["type"] = b.get("ShipType", s.get("type"))
                                s["call"] = b.get("CallSign", s.get("call"))
            except Exception as e:  # keep the stream alive across drops
                AIS["status"] = f"reconnecting after {type(e).__name__}"
                await asyncio.sleep(wait)
                wait = min(wait * 2, 300)

    asyncio.run(run())


def src_aisstream():
    """Ships in UK and Irish waters and the AIS_BOXES regions from AISStream (free API key), positions from the
    last 5 minutes; the stream runs in the background between cycles."""
    import threading
    if AIS["thread"] is None:
        AIS["thread"] = threading.Thread(target=ais_stream, name="aisstream", daemon=True)
        AIS["thread"].start()
        time.sleep(30)  # first cycle: let the stream fill
    now = time.time()
    out = []
    for mmsi, p in list(AIS["positions"].items()):
        if now - p["t"] > 300 or p["lat"] is None or p["lon"] is None or abs(p["lat"]) > 90 or abs(p["lon"]) > 180:
            if now - p["t"] > 3600:
                AIS["positions"].pop(mmsi, None)
                AIS["static"].pop(mmsi, None)  # keep memory bounded with the wider boxes
            continue
        s = AIS["static"].get(mmsi, {})
        st = s.get("type")
        sog, cog, hdg = p.get("sog"), p.get("cog"), p.get("hdg")
        attrs = {"lat": float(p["lat"]), "lon": float(p["lon"]), "alt_m": 0.0, "mmsi": int(mmsi),
                 "speed": float(sog) if sog is not None and sog < 102.3 else None,
                 "track": float(cog) if cog is not None and cog < 360 else None,
                 "heading": float(hdg) if hdg is not None and hdg < 360 else None,
                 "nav_status": AIS_STATUS.get(p.get("nav")),
                 "name": clean(s.get("name") or p.get("name")), "callsign": clean(s.get("call")),
                 "ship_type": next((label for (lo, hi), label in AIS_TYPES if st is not None and lo <= st <= hi), None),
                 "ship_type_code": int(st) if st is not None else None,
                 "destination": clean(s.get("dest")), "imo": int(s["imo"]) if s.get("imo") else None,
                 "source": "aisstream"}
        out.append(obs(f"MMSI-{mmsi}", "sea-vessel", p["t"], attrs))
    msgs, AIS["msgs"] = AIS["msgs"], 0
    box_msgs, AIS["box_msgs"] = AIS["box_msgs"], {}
    ships = {}
    for o in out:
        for b in ais_ru_box(o["attrs"]["lat"], o["attrs"]["lon"]):
            ships[b] = ships.get(b, 0) + 1
    ru = " ".join(f"{b}={box_msgs.get(b, 0)}/{ships.get(b, 0)}" for b in AIS_RU_BOXES)
    return f"{len(AIS['positions'])} ({AIS['status']}, {msgs} msgs since last cycle; ru msgs/ships {ru})", out


# ---------------------------------------------------------------- shared helpers for areas

MAX_VERTICES = 200  # the console contract: an area's outer ring has at most ~200 vertices


def get_text(url, timeout=TIMEOUT, accept="*/*"):
    """Like get(), for RSS / HTML bodies."""
    req = urllib.request.Request(url, headers={**UA, "Accept": accept})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return r.read().decode(r.headers.get_content_charset() or "utf-8", "replace")
    except urllib.error.HTTPError as e:
        if e.code == 429:
            raise RateLimited(url) from e
        raise


def ring_area(ring):
    """Signed planar area of a ring of (lat, lon) in degrees^2 (shoelace on lon, lat)."""
    return sum(ring[i][1] * ring[(i + 1) % len(ring)][0] - ring[(i + 1) % len(ring)][1] * ring[i][0]
               for i in range(len(ring))) / 2.0


def centroid(ring):
    """Area-weighted centroid of a ring of (lat, lon); the vertex mean for a degenerate ring.
    Planar in degrees, which is fine for areas of a few hundred km."""
    a = ring_area(ring)
    if abs(a) < 1e-12:
        return (sum(p[0] for p in ring) / len(ring), sum(p[1] for p in ring) / len(ring))
    cx = cy = 0.0
    for i in range(len(ring)):
        (y0, x0), (y1, x1) = ring[i], ring[(i + 1) % len(ring)]
        cross = x0 * y1 - x1 * y0
        cx += (x0 + x1) * cross
        cy += (y0 + y1) * cross
    return (cy / (6 * a), cx / (6 * a))


def _dp(points, eps):
    """Douglas-Peucker on (lat, lon) points (open polyline), planar in degrees."""
    if len(points) < 3:
        return points
    (ay, ax), (by, bx) = points[0], points[-1]
    dx, dy = bx - ax, by - ay
    norm = math.hypot(dx, dy) or 1e-12
    worst, idx = -1.0, 0
    for i in range(1, len(points) - 1):
        py, px = points[i]
        d = abs(dy * (px - ax) - dx * (py - ay)) / norm if (dx or dy) else math.hypot(px - ax, py - ay)
        if d > worst:
            worst, idx = d, i
    if worst <= eps:
        return [points[0], points[-1]]
    return _dp(points[:idx + 1], eps)[:-1] + _dp(points[idx:], eps)


def simplify(ring, limit=MAX_VERTICES):
    """Round to 4 dp, drop repeated vertices, then Douglas-Peucker with a growing tolerance
    until the ring fits the vertex limit (the closing vertex is kept)."""
    pts = []
    for lat, lon in ring:
        p = (round(lat, 4), round(lon, 4))
        if not pts or pts[-1] != p:
            pts.append(p)
    eps = 0.0005
    while len(pts) > limit:
        pts = _dp(pts, eps)
        eps *= 2
    return pts


def poly_text(ring):
    """(lat, lon) ring -> the contract's "lat,lon;lat,lon;..." text."""
    return ";".join(f"{lat},{lon}" for lat, lon in simplify(ring)) or None


def geojson_outer(geom):
    """GeoJSON Polygon / MultiPolygon -> the largest outer ring as (lat, lon) points."""
    if not geom:
        return None
    if geom.get("type") == "Polygon":
        polys = [geom.get("coordinates") or []]
    elif geom.get("type") == "MultiPolygon":
        polys = geom.get("coordinates") or []
    else:
        return None
    rings = [[(float(c[1]), float(c[0])) for c in p[0]] for p in polys if p and len(p[0]) >= 3]
    return max(rings, key=lambda r: abs(ring_area(r))) if rings else None


def slot(period):
    """The current time floored to a resend period: observedAt for a resend every `period`
    seconds (send() skips a second send in the same slot)."""
    return int(time.time() // period * period)


def day_start():
    return int(time.time() // 86400 * 86400)


def slug(text):
    return re.sub(r"[^A-Z0-9]+", "-", str(text).upper()).strip("-")


# ---------------------------------------------------------------- 9. SIGMETs (aviationweather)

def src_sigmet():
    """International SIGMETs worldwide (NOAA aviationweather.gov Data API, public domain, no
    key; the old 40-72N 30W-25E filter was dropped). Refetched every 10 min; each still-valid SIGMET is resent
    every 10 min (observedAt = the 10-minute slot); expired ones are dropped."""
    cache = load("sigmet.json", {"fetched": 0, "sigmets": {}})
    fetched = "cached"
    if time.time() - cache["fetched"] >= 600:
        rows = get("https://aviationweather.gov/api/data/isigmet?format=json") or []
        sigmets = {}
        for s in rows:
            # Most SIGMETs are one area (a list of points); some ("AREAS") are a list of
            # areas, each sent as its own entity.
            coords = s.get("coords") or []
            areas = coords if coords and isinstance(coords[0], list) else [coords]
            for n, area in enumerate(areas):
                ring = [(float(c["lat"]), float(c["lon"])) for c in area
                        if isinstance(c, dict) and c.get("lat") is not None and c.get("lon") is not None]
                if len(ring) < 3 or not s.get("validTimeTo"):
                    continue
                fir = clean(s.get("firId")) or clean(s.get("icaoId")) or "UNKNOWN"
                key = (f"SIGMET-{slug(fir)}-{slug(s.get('seriesId') or 'X')}-{int(s['validTimeFrom'] or 0)}"
                       + (f"-{n + 1}" if len(areas) > 1 else ""))
                if max(p[1] for p in ring) - min(p[1] for p in ring) > 180:  # crosses the antimeridian
                    lat, lon = centroid([(a, b + 360 if b < 0 else b) for a, b in ring])
                    lon = (lon + 180) % 360 - 180
                else:
                    lat, lon = centroid(ring)
                sigmets[key] = {"lat": round(lat, 4), "lon": round(lon, 4), "polygon": poly_text(ring),
                                "hazard": clean(s.get("hazard")), "qualifier": clean(s.get("qualifier")),
                                "base_ft": num(s.get("base"), int), "top_ft": num(s.get("top"), int),
                                "fir": fir, "fir_name": clean(s.get("firName")),
                                "valid_from": num(s.get("validTimeFrom"), int), "valid_to": num(s.get("validTimeTo"), int),
                                "raw": clean(s.get("rawSigmet")), "source": "aviationweather"}
        cache = {"fetched": time.time(), "sigmets": sigmets}
        save("sigmet.json", cache)
        fetched = len(sigmets)
    now = time.time()
    t = slot(600)
    return fetched, [obs(k, "event-sigmet", t, a) for k, a in cache["sigmets"].items() if a["valid_to"] > now]


# ---------------------------------------------------------------- 10. Met Office warnings

# The Met Office's 16 National Severe Weather Warning regions, with a rough centroid each:
# the fallback position when a warning's polygon is not available.
MO_REGIONS = {
    "orkney & shetland": (59.7, -2.0), "highlands & eilean siar": (57.5, -5.0), "grampian": (57.2, -2.7),
    "strathclyde": (55.8, -4.9), "central, tayside & fife": (56.5, -3.6),
    "sw scotland, lothian borders": (55.3, -3.3), "northern ireland": (54.6, -6.7), "wales": (52.3, -3.7),
    "north west england": (54.0, -2.7), "north east england": (55.0, -1.9), "yorkshire & humber": (53.9, -1.2),
    "west midlands": (52.5, -2.2), "east midlands": (52.9, -0.9), "east of england": (52.3, 0.6),
    "south west england": (50.9, -3.6), "london & south east england": (51.3, -0.5),
}
MO_RSS = "https://www.metoffice.gov.uk/public/data/PWSCache/WarningsRSS/Region/UK"
MO_PAGE = "https://weather.metoffice.gov.uk/warnings-and-advice/uk-warnings"
MONTHS = {m: i + 1 for i, m in enumerate("jan feb mar apr may jun jul aug sep oct nov dec".split())}


def mo_time(hhmm, day, mon):
    """'0600', '18', 'Oct' (UK local time, no year) -> Unix seconds, taking the year that puts
    the time nearest now."""
    from zoneinfo import ZoneInfo
    now = datetime.now(timezone.utc)
    best = None
    for year in (now.year - 1, now.year, now.year + 1):
        try:
            d = datetime(year, MONTHS[mon.lower()[:3]], int(day), int(hhmm[:2]) % 24, int(hhmm[2:]),
                         tzinfo=ZoneInfo("Europe/London"))
        except (KeyError, ValueError):
            return None
        if best is None or abs((d - now).total_seconds()) < abs((best - now).total_seconds()):
            best = d
    return int(best.timestamp())


def mo_region_point(region):
    r = (region or "").lower().replace(" and ", " & ").strip()
    return MO_REGIONS.get(r) or next((v for k, v in MO_REGIONS.items() if k in r), None)


def parse_mo_rss(text):
    """The warnings RSS -> {key: attrs}. One item per warning per region, e.g. title
    'Yellow warning of wind affecting London & South East England', description '...: Kent,
    Surrey valid from 0600 Sat 18 Oct to 2100 Sat 18 Oct', link '...uk-warnings#?id=<id>...'."""
    import xml.etree.ElementTree as ET
    out = {}
    for it in ET.fromstring(text).iter("item"):
        title = clean(it.findtext("title")) or ""
        desc = clean(it.findtext("description")) or ""
        link = clean(it.findtext("link"))
        m = re.match(r"(?i)(yellow|amber|red)\s+warning\s+of\s+(.+?)\s+affecting\s+(.+)$", title)
        level, weather, region = (m.group(1).lower(), m.group(2).lower(), m.group(3)) if m else (None, None, None)
        if weather:
            weather = ", ".join(sorted({w.strip().rstrip("s") if w.strip() == "thunderstorms" else w.strip()
                                        for w in re.split(r",|\band\b", weather) if w.strip()}))
        v = re.search(r"valid from (\d{4}) \w+ (\d{1,2}) (\w+) to (\d{4}) \w+ (\d{1,2}) (\w+)", desc, re.I)
        wid = (re.search(r"[?&#]id=([^&]+)", link or "") or [None, None])[1] or clean(it.findtext("guid"))
        key = f"MOWARN-{slug(wid or title + desc)[:60]}-{slug(region or 'UK')[:40]}"
        p = mo_region_point(region)
        out[key] = {"lat": p[0] if p else None, "lon": p[1] if p else None, "level": level, "weather": weather,
                    "region": region, "title": title or None, "areas": clean(desc.split(":", 1)[1].split(" valid ")[0])
                    if ":" in desc else None,
                    "valid_from": mo_time(*v.group(1, 2, 3)) if v else None,
                    "valid_to": mo_time(*v.group(4, 5, 6)) if v else None,
                    "link": link, "warning_id": wid, "source": "metoffice"}
    return out


def parse_mo_polygons(html):
    """The public warnings page embeds its map polygons (no key) as
    `polygonsGeoJson: {...FeatureCollection...}` with properties id, warningLevel, weather."""
    m = re.search(r"polygonsGeoJson:\s*(\{.*?\})\s*,\s*\n\s*days:", html, re.S)
    if not m:
        return {}
    polys = {}
    for f in json.loads(m.group(1)).get("features") or []:
        p = f.get("properties") or {}
        ring = geojson_outer(f.get("geometry"))
        if ring and p.get("id") is not None:
            polys.setdefault(str(p["id"]), (ring, p))
    return polys


def src_metoffice():
    """Met Office National Severe Weather Warnings (keyless public RSS, Crown copyright),
    polygons from the public warnings page when it has them, else the region centroid.
    Refetched every 15 min; resent every 10 min (observedAt = the 10-minute slot)."""
    cache = load("metoffice.json", {"fetched": 0, "warnings": {}})
    fetched = "cached"
    if time.time() - cache["fetched"] >= 900:
        warnings = parse_mo_rss(get_text(MO_RSS, accept="application/rss+xml, application/xml"))
        try:
            polys = parse_mo_polygons(get_text(MO_PAGE, accept="text/html"))
        except Exception as e:  # polygons are a nice-to-have: keep the region centroids
            log_line("metoffice-warnings", f"polygon page failed ({type(e).__name__}: {e}); using region centroids")
            polys = {}
        matched = set()
        for w in warnings.values():
            if w["warning_id"] in polys:
                ring, _ = polys[w["warning_id"]]
                matched.add(w["warning_id"])
                w["polygon"] = poly_text(ring)
                if w["lat"] is None:
                    w["lat"], w["lon"] = (round(c, 4) for c in centroid(ring))
        for wid, (ring, p) in polys.items():  # a mapped warning the RSS did not list
            if wid not in matched:
                lat, lon = centroid(ring)
                warnings[f"MOWARN-{slug(wid)[:60]}"] = {
                    "lat": round(lat, 4), "lon": round(lon, 4), "polygon": poly_text(ring),
                    "level": (clean(p.get("warningLevel")) or "").lower() or None,
                    "weather": clean(", ".join(p["weather"]) if isinstance(p.get("weather"), list) else p.get("weather")),
                    "warning_id": wid, "link": MO_PAGE, "source": "metoffice"}
        warnings = {k: w for k, w in warnings.items() if w.get("lat") is not None}
        cache = {"fetched": time.time(), "warnings": warnings}
        save("metoffice.json", cache)
        fetched = len(warnings)
    now = time.time()
    t = slot(600)
    return fetched, [obs(k, "event-weather-warning", t, a) for k, a in cache["warnings"].items()
                     if not a.get("valid_to") or a["valid_to"] > now]


# ---------------------------------------------------------------- 11. Offshore wind farms

TCE_WIND = ("https://services2.arcgis.com/PZklK9Q45mfMFuZs/arcgis/rest/services/"
            "WindSite_EngWalNI_TheCrownEstate/FeatureServer/0/query")
CES_WIND = ("https://services3.arcgis.com/nGV4jiurzcahJ9LV/arcgis/rest/services/"
            "Offshore_Wind_Crown_Estate_Scotland/FeatureServer/0/query")
WIND_STATUS = {"active/in operation": "operational", "pre planning": "pre-planning",
               "pre-planning application": "pre-planning"}


def arcgis_geojson(url, fields):
    q = urllib.parse.urlencode({"where": "1=1", "outFields": fields, "outSR": 4326, "f": "geojson",
                                "returnGeometry": "true", "geometryPrecision": 5})
    d = get(f"{url}?{q}", timeout=60)
    if not isinstance(d, dict) or "features" not in d:
        raise ValueError(f"unexpected ArcGIS response: {str(d)[:200]}")
    return d["features"]


def daily_cache(name, fetch):
    """A static dataset cached on disk, refetched once a day (a stale cache beats nothing)."""
    cache = load(name, None)
    if cache is None or day_start() > cache.get("day", 0):
        try:
            cache = {"day": day_start(), "items": fetch()}
            save(name, cache)
            return len(cache["items"]), cache
        except Exception as e:
            if cache is None:
                raise
            log_line(name.split(".")[0], f"refetch failed ({type(e).__name__}: {e}); using cache of {utc(cache['day'])}")
    return "cached", cache


def fetch_windfarms():
    items = {}

    def add(prefix, name, ring, attrs):
        key = f"{prefix}-{slug(name)}"
        while key in items:
            key += "-2"
        lat, lon = centroid(ring)
        items[key] = {"lat": round(lat, 4), "lon": round(lon, 4), "name": name, "polygon": poly_text(ring), **attrs}

    # The Crown Estate (England, Wales and NI): Open Data Licence (GIS) v1.1
    for f in arcgis_geojson(TCE_WIND, "Name_Prop,Name_Ten,Wind_Round,Inf_Status,Lease_Stat,km2"):
        p, ring = f.get("properties") or {}, geojson_outer(f.get("geometry"))
        if ring and p.get("Name_Prop"):
            st = (p.get("Inf_Status") or "").strip().lower()
            add("TCE", clean(p["Name_Prop"]), ring, {
                "status": WIND_STATUS.get(st, st or None), "developer": clean(p.get("Name_Ten")),
                "round": clean(p.get("Wind_Round")), "lease": clean(p.get("Lease_Stat")),
                "area_km2": round(p["km2"], 2) if p.get("km2") else None, "source": "crown-estate"})
    # Crown Estate Scotland: Open Government Licence v3; wind farm sites only (not OFTO/substations)
    for f in arcgis_geojson(CES_WIND, "Property_Description,Tenant_Name,Property_Classification,Project_Phase,Capacity_MW"):
        p, ring = f.get("properties") or {}, geojson_outer(f.get("geometry"))
        if ring and p.get("Property_Description") and p.get("Property_Classification") == "Wind Farm":
            st = (p.get("Project_Phase") or "").strip().lower()
            add("CES", clean(p["Property_Description"]), ring, {
                "status": WIND_STATUS.get(st, st or None), "developer": clean(p.get("Tenant_Name")),
                "capacity_mw": float(p["Capacity_MW"]) if p.get("Capacity_MW") else None,
                "source": "crown-estate-scotland"})
    return items


def src_windfarms():
    """UK offshore wind farm sites (The Crown Estate and Crown Estate Scotland open data, no
    key): fetched and sent once a day, observedAt = the UTC day start (Heimdall TTL 0)."""
    fetched, cache = daily_cache("windfarms.json", fetch_windfarms)
    t = day_start()
    return fetched, [obs(k, "sea-windfarm", t, a) for k, a in cache["items"].items()]


# ---------------------------------------------------------------- 12. Offshore installations (NSTA)

NSTA_SURFACE = ("https://services-eu1.arcgis.com/OZMfUznmLTnWccBc/arcgis/rest/services/"
                "UKCS%20offshore%20infrastructure%20surface%20points%20WGS84/FeatureServer/1/query")


def fetch_platforms():
    """Surface installations only (this layer has ~300; the subsea layers are separate and
    not used). Removed installations and onshore terminals are skipped."""
    items = {}
    for f in arcgis_geojson(NSTA_SURFACE, "FEATURE_ID,NAME,INF_TYPE,REP_GROUP,DESCRIPTIO,STATUS,PIPE_SYS"):
        p, g = f.get("properties") or {}, (f.get("geometry") or {}).get("coordinates") or []
        typ, st = (p.get("INF_TYPE") or "").strip().lower(), (p.get("STATUS") or "").strip().lower()
        if len(g) < 2 or not p.get("FEATURE_ID") or st == "removed" or typ == "terminal":
            continue
        items[f"NSTA-{p['FEATURE_ID']}"] = {
            "lat": round(float(g[1]), 5), "lon": round(float(g[0]), 5), "alt_m": 0.0,
            "name": clean(p.get("NAME")), "operator": clean(p.get("REP_GROUP")), "status": st or None,
            "type": typ or None, "description": clean(p.get("DESCRIPTIO"), 200),
            "pipeline_system": clean(p.get("PIPE_SYS")), "source": "nsta"}
    return items


def src_platforms():
    """UKCS offshore surface installations (NSTA open data, NSTA Open User Licence, no key):
    fetched and sent once a day, observedAt = the UTC day start (Heimdall TTL 0)."""
    fetched, cache = daily_cache("platforms.json", fetch_platforms)
    t = day_start()
    return fetched, [obs(k, "sea-platform", t, a) for k, a in cache["items"].items()]


# ---------------------------------------------------------------- 13. Space weather (NOAA SWPC)

SWPC = "https://services.swpc.noaa.gov"


def xray_class(flux):
    """GOES 0.1-0.8 nm flux (W/m^2) -> flare class letter and magnitude, e.g. 1.3e-6 -> 'C1.3'."""
    for letter, base in (("X", 1e-4), ("M", 1e-5), ("C", 1e-6), ("B", 1e-7), ("A", 1e-8)):
        if flux >= base:
            return f"{letter}{flux / base:.1f}"
    return "A0.0"


def src_swpc():
    """One global entity 'SWPC' (NOAA SWPC JSON products, public domain, no key): latest
    3-hourly planetary Kp, current NOAA G/S/R scales and the current GOES X-ray class."""
    kp_rows = get(f"{SWPC}/products/noaa-planetary-k-index.json")
    last = kp_rows[-1]
    if isinstance(last, list):  # the older table form: header row, then [time_tag, Kp, ...]
        kp, kp_time = num(last[1]), last[0]
    else:
        kp, kp_time = num(last.get("Kp")), last.get("time_tag")
    scales = (get(f"{SWPC}/products/noaa-scales.json") or {}).get("0") or {}
    xray = None
    try:
        flares = get(f"{SWPC}/json/goes/primary/xray-flares-latest.json") or []
        xray = clean(flares[-1].get("current_class")) if flares else None
        if not xray:
            rows = [r for r in get(f"{SWPC}/json/goes/primary/xrays-6-hour.json") if r.get("energy") == "0.1-0.8nm"]
            xray = xray_class(rows[-1]["flux"]) if rows else None
    except Exception as e:  # the X-ray class is optional
        log_line("noaa-swpc", f"X-ray class unavailable ({type(e).__name__}: {e})")
    scale = lambda k: num((scales.get(k) or {}).get("Scale"), int)
    attrs = {"lat": 0.0, "lon": 0.0, "kp": kp, "kp_time": iso(kp_time), "g_scale": scale("G"),
             "s_scale": scale("S"), "r_scale": scale("R"), "xray_class": xray, "source": "noaa-swpc"}
    return 1, [obs("SWPC", "space-weather", time.time(), attrs)]


# ---------------------------------------------------------------- 14. TfL road disruptions

def tfl_line(d):
    """The longest street segment's line as 'lat,lon;...' (when there is no polygon)."""
    best = []
    for s in d.get("streets") or []:
        for seg in s.get("segments") or []:
            try:
                pts = [(float(c[1]), float(c[0])) for c in json.loads(seg.get("lineString") or "[]")]
            except (ValueError, TypeError, IndexError):
                continue
            if len(pts) > len(best):
                best = pts
    return poly_text(best) if len(best) >= 2 else None


def src_tfl():
    """Serious and Severe current road disruptions in London (TfL Unified API, keyless at low
    rate; Powered by TfL Open Data). Refetched every 5 min, resent every 4 min
    (observedAt = the 4-minute slot)."""
    cache = load("tfl.json", {"fetched": 0, "disruptions": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 300:
        rows = get("https://api.tfl.gov.uk/Road/all/Disruption", timeout=60) or []
        dis = {}
        for d in rows:
            if d.get("severity") not in ("Serious", "Severe") or not str(d.get("status", "")).startswith("Active"):
                continue
            start, end = iso(d.get("startDateTime")), iso(d.get("endDateTime"))
            if (start and start > now) or (end and end < now):
                continue
            try:
                lon, lat = json.loads(d.get("point") or "null") or (d.get("geography") or {}).get("coordinates")
            except (TypeError, ValueError):
                continue
            geom = d.get("geometry")
            ring = geojson_outer(json.loads(geom) if isinstance(geom, str) else geom)
            dis[f"TFL-{d['id']}"] = {
                "lat": float(lat), "lon": float(lon), "severity": d["severity"], "category": clean(d.get("category")),
                "sub_category": clean(d.get("subCategory")), "location": clean(d.get("location"), 200),
                "comments": clean(d.get("comments"), 200), "current_update": clean(d.get("currentUpdate"), 200),
                "start": start, "end": end, "has_closures": bool(d.get("hasClosures")),
                "polygon": poly_text(ring) if ring else None, "line": None if ring else tfl_line(d), "source": "tfl"}
        cache = {"fetched": now, "disruptions": dis}
        save("tfl.json", cache)
        fetched = len(dis)
    t = slot(240)
    return fetched, [obs(k, "land-road-disruption", t, a) for k, a in cache["disruptions"].items()
                     if not a.get("end") or a["end"] > now]


# ---------------------------------------------------------------- 16. openAIP airspace (free key)

OPENAIP_KEY = os.path.join(HERE, "secrets", "openaip.key")  # local only, never committed
# openAIP enums (Core API schema): airspace type, ICAO class, height unit, height datum.
OAIP_TYPES = {0: "other", 1: "restricted", 2: "danger", 3: "prohibited", 4: "ctr", 5: "tmz", 6: "rmz", 7: "tma",
              8: "tra", 9: "tsa", 10: "fir", 11: "uir", 12: "adiz", 13: "atz", 14: "matz", 15: "airway", 16: "mtr",
              17: "alert", 18: "warning", 19: "protected", 20: "htz", 21: "gliding", 22: "trp", 23: "tiz", 24: "tia",
              25: "mta", 26: "cta", 27: "acc-sector", 28: "sporting", 29: "overflight-restriction", 30: "mrt",
              31: "tfr", 32: "vfr-sector", 33: "fis-sector", 34: "lta", 35: "uta", 36: "mctr"}
OAIP_CLASS = {0: "A", 1: "B", 2: "C", 3: "D", 4: "E", 5: "F", 6: "G"}
OAIP_DATUM = {0: "GND", 1: "MSL", 2: "STD"}


def oaip_feet(limit):
    """An openAIP height limit in feet (unit 0 metres, 1 feet, 6 flight level)."""
    if not limit:
        return None
    v, unit = limit.get("value"), limit.get("unit")
    if v is None:
        return None
    return int(round(v * 3.28084)) if unit == 0 else int(v * 100) if unit == 6 else int(v)


OAIP_COUNTRIES = ("GB IE " + " ".join(c for c in AIRPORT_COUNTRIES)).split()


def fetch_airspaces():
    """Every country's airspaces, paged 1000 at a time with 1 s between requests. A country
    that fails is logged and skipped; a 429 waits 30 s and retries once."""
    key = open(OPENAIP_KEY).read().strip()
    items, failed = [], []
    for country in OAIP_COUNTRIES:
        page = 1
        while True:
            req = urllib.request.Request(
                f"https://api.core.openaip.net/api/airspaces?country={country}&limit=1000&page={page}",
                headers={**UA, "x-openaip-api-key": key})
            d = None
            for attempt in (0, 1):
                try:
                    with urllib.request.urlopen(req, timeout=60) as r:
                        d = json.load(r)
                    break
                except urllib.error.HTTPError as e:
                    if e.code == 429 and not attempt:
                        time.sleep(30)
                        continue
                    failed.append(f"{country}:HTTP{e.code}")
                    break
                except Exception as e:
                    failed.append(f"{country}:{type(e).__name__}")
                    break
            time.sleep(1)
            if d is None:
                break
            for a in d.get("items", []):
                kind = OAIP_TYPES.get(a.get("type"), "other")
                geom = a.get("geometry") or {}
                if kind in ("fir", "uir") or geom.get("type") != "Polygon":
                    continue  # flight information regions cover everything; nothing to see
                ring = geom["coordinates"][0]
                step = max(1, len(ring) // 200)
                pts = ring[::step]
                items.append({"id": a["_id"], "name": clean(a.get("name")), "type": kind,
                              "class": OAIP_CLASS.get(a.get("icaoClass")), "country": country,
                              "lower_ft": oaip_feet(a.get("lowerLimit")), "upper_ft": oaip_feet(a.get("upperLimit")),
                              "lower_ref": OAIP_DATUM.get((a.get("lowerLimit") or {}).get("referenceDatum")),
                              "upper_ref": OAIP_DATUM.get((a.get("upperLimit") or {}).get("referenceDatum")),
                              "lat": round(sum(p[1] for p in pts) / len(pts), 5), "lon": round(sum(p[0] for p in pts) / len(pts), 5),
                              "polygon": ";".join(f"{p[1]:.4f},{p[0]:.4f}" for p in pts)})
            if page >= (d.get("totalPages") or 1):
                break
            page += 1
    if failed:
        log_line("openaip", f"{len(failed)} requests failed: {', '.join(failed)}")
        if len(failed) == len(OAIP_COUNTRIES):
            raise RuntimeError("every openAIP country failed")
    return items


OAIP = {"thread": None}


def openaip_refresh(name):
    try:
        save(name, {"day": day_start(), "items": fetch_airspaces()})
    except Exception as e:
        log_line("openaip", f"background refetch failed ({type(e).__name__}: {e}); keeping the old cache")
    finally:
        OAIP["thread"] = None


def src_openaip():
    """Airspace for GB, IE and OAIP_COUNTRIES from openAIP (free API key; attribution to
    openaip.net required), fetched and sent once a UTC day; static, so given TTL 0. The cache
    file name carries the country set, so changing it triggers a refetch. The refetch (51
    countries, ~4 min) runs in a background thread so it never stalls the other sources;
    observedAt is the cache's day, so a refreshed cache is sent as soon as it lands."""
    import threading
    name = f"openaip_airspaces_{len(OAIP_COUNTRIES)}.json"
    cache = load(name, None)
    fetched = "cached"
    if cache is None or day_start() > cache.get("day", 0):
        if OAIP["thread"] is None:
            OAIP["thread"] = threading.Thread(target=openaip_refresh, args=(name,), name="openaip", daemon=True)
            OAIP["thread"].start()
        fetched = "refreshing in background"
        if "--once" in sys.argv:  # a one-shot run waits for it
            OAIP["thread"].join()
            cache, fetched = load(name, cache), "refetched"
    if cache is None:
        return fetched, []
    out = [obs(f"OAIP-{a['id']}", "airspace-volume", cache["day"], {
        "lat": a["lat"], "lon": a["lon"], "name": a["name"], "airspace_type": a["type"], "icao_class": a["class"],
        "lower_ft": a["lower_ft"], "upper_ft": a["upper_ft"], "lower_ref": a["lower_ref"], "upper_ref": a["upper_ref"],
        "country": a["country"], "polygon": a["polygon"], "source": "openaip"}) for a in cache["items"]]
    return f"{fetched} {len(cache['items'])}", out


# ---------------------------------------------------------------- 17. NASA FIRMS fires (free key)

FIRMS_KEY = os.path.join(HERE, "secrets", "firms.key")  # local only, never committed


FIRMS_BOXES = (  # west,south,east,north; overlapping boxes are deduplicated by entity key
    ("uk", "-12,48,4,62"), ("balkans", "13,39,30,48"), ("turkey-levant", "25,29,45,43"),
    ("red-sea-egypt", "24,12,44,32"), ("gulf-iraq-iran", "38,16,64,40"), ("west-russia", "19,50,50,62"),
    ("taiwan", "118,21,123,26.5"), ("singapore-malaysia", "99,0,120,8"), ("east-australia", "140,-44,154,-10"),
    ("new-zealand", "165,-48,179,-34"), ("canada-east", "-84,41,-52,50"), ("canada-west", "-130,48,-114,56"),
    ("india-pakistan", "60,8,90,37"), ("south-africa", "16,-35,33,-22"), ("kenya", "33,-5,42,5"),
    ("nigeria", "2,4,15,14"),
    # the rest of Russia, in large boxes (5 boxes x 2 products = 10 more requests per 30 min)
    ("russia-north", "25,62,66,72"), ("russia-south-caspian", "36,41,56,50"), ("urals-volga", "50,50,66,62"),
    ("siberia", "66,49,110,78"), ("far-east", "110,42,180,73"),
)


def src_firms():
    """Satellite fire hotspots (VIIRS on Suomi NPP and NOAA-20, near real time) over the
    UK and Ireland and the FIRMS_BOXES regions, from the last day; NASA FIRMS, free MAP_KEY.
    Every 30 min: 2 products x 21 boxes = 42 requests, far inside 5000 per 10 min."""
    key = open(FIRMS_KEY).read().strip()
    found, fetched, per, failed = {}, 0, {}, []
    for region, box in FIRMS_BOXES:
        for product in ("VIIRS_SNPP_NRT", "VIIRS_NOAA20_NRT"):
            req = urllib.request.Request(f"https://firms.modaps.eosdis.nasa.gov/api/area/csv/{key}/{product}/{box}/1", headers=UA)
            try:
                with urllib.request.urlopen(req, timeout=90) as r:
                    text = r.read().decode()
            except urllib.error.HTTPError as e:
                if e.code == 429:
                    raise RateLimited(req.full_url) from e
                failed.append(f"{region}/{product}:HTTP{e.code}")
                continue
            except Exception as e:  # one box failing must not lose the rest
                failed.append(f"{region}/{product}:{type(e).__name__}")
                continue
            if not text.startswith("latitude"):
                failed.append(f"{region}/{product}:{text[:60]!r}")
                continue
            rows = list(csv.DictReader(io.StringIO(text)))
            fetched += len(rows)
            per[region] = per.get(region, 0) + len(rows)
            for f in rows:
                t = f["acq_time"].zfill(4)
                at = iso(f"{f['acq_date']}T{t[:2]}:{t[2:]}:00Z")
                lat, lon = float(f["latitude"]), float(f["longitude"])
                conf = {"l": "low", "n": "nominal", "h": "high"}.get(f.get("confidence", ""), f.get("confidence"))
                k = f"FIRE-{lat:.3f}-{lon:.3f}-{at}"
                found[k] = obs(k, "event-fire", at, {
                    "lat": lat, "lon": lon, "alt_m": 0.0, "frp": num(f.get("frp")), "confidence": conf,
                    "satellite": {"N": "Suomi NPP", "1": "NOAA-20", "N20": "NOAA-20"}.get(f.get("satellite"), f.get("satellite")),
                    "instrument": f.get("instrument"), "acq_time": at, "brightness_k": num(f.get("bright_ti4")),
                    "daynight": f.get("daynight"), "region": region, "source": "nasa-firms"})
    if failed:
        log_line("nasa-firms", f"{len(failed)} box requests failed: {', '.join(failed)[:300]}")
    if failed and not found:
        raise RuntimeError("every FIRMS request failed")
    gulf_fire_record([(o["attrs"]["lat"], o["attrs"]["lon"], o["observedAt"], o["attrs"]["frp"], o["attrs"]["confidence"])
                      for o in found.values()])
    return f"{fetched} [{' '.join(f'{k}={v}' for k, v in per.items())}]", list(found.values())


# ---------------------------------------------------------------- 18. National Highways (free key)

NH_KEY = os.path.join(HERE, "secrets", "national-highways.key")  # local only, never committed


def nh_lines(loc, out):
    """Every GML line string (posList "lat lon lat lon ...") anywhere under a DATEX II location."""
    if isinstance(loc, dict):
        for k, v in loc.items():
            if k == "posList" and isinstance(v, str):
                n = [float(x) for x in v.split()]
                out.append(list(zip(n[0::2], n[1::2])))
            else:
                nh_lines(v, out)
    elif isinstance(loc, list):
        for v in loc:
            nh_lines(v, out)
    return out


def nh_find(obj, key):
    """The first value for `key` anywhere under a DATEX II record."""
    if isinstance(obj, dict):
        if key in obj:
            return obj[key]
        for v in obj.values():
            r = nh_find(v, key)
            if r is not None:
                return r
    elif isinstance(obj, list):
        for v in obj:
            r = nh_find(v, key)
            if r is not None:
                return r
    return None


def src_national_highways():
    """Road and lane closures on England's motorways and major A roads, planned works and
    unplanned events (National Highways Road and Lane Closures v2.0, DATEX II; free key,
    10 calls a minute). Only those in force now. Refetched every 5 min, resent every 4 min
    (observedAt = the 4-minute slot); the same kind as the TfL feed."""
    cache = load("national_highways.json", {"fetched": 0, "items": []})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] > 300:
        req = urllib.request.Request("https://api.data.nationalhighways.co.uk/roads/v2.0/closures", headers={
            **UA, "Ocp-Apim-Subscription-Key": open(NH_KEY).read().strip(), "X-Response-MediaType": "application/json"})
        with urllib.request.urlopen(req, timeout=60) as r:
            d = json.load(r)["D2Payload"]
        items = []
        for s in d.get("situation", []):
            for rec in s.get("situationRecord", []):
                rtype, body = next(iter(rec.items()))
                v = body.get("validity", {})
                spec = v.get("validityTimeSpecification", {})
                start, end = iso(spec.get("overallStartTime")), iso(spec.get("overallEndTime"))
                status = v.get("validityStatus")
                live = status == "active" or (status == "definedByValidityTimeSpec" and (start or 0) <= now <= (end or now + 1))
                if not live:
                    continue
                lines = nh_lines(body.get("locationReference"), [])
                if not lines:
                    continue
                line = max(lines, key=len)
                step = max(1, len(line) // 100)
                pts = line[::step] + ([line[-1]] if (len(line) - 1) % step else [])
                mgmt = (body.get("roadOrCarriagewayOrLaneManagementType") or {}).get("value", "")
                kind = rtype.removeprefix("sit")
                unplanned = (body.get("source") or {}).get("sourceIdentification") != "roadworks"
                closed = "clos" in mgmt.lower() or "closure" in kind.lower()
                comment = " ".join(c.get("comment", "") for c in body.get("generalPublicComment") or [])
                items.append({"id": f'{s["idG"]}-{body.get("idG", "")[-12:]}', "lat": pts[len(pts) // 2][0], "lon": pts[len(pts) // 2][1],
                              "line": ";".join(f"{a:.5f},{b:.5f}" for a, b in pts),
                              "severity": "Severe" if closed and unplanned else "Serious" if closed or unplanned else "Moderate",
                              "category": ("unplanned " if unplanned else "planned ") + ((body.get("cause") or {}).get("causeType") or kind),
                              "location": clean(nh_find(body, "locationDescription")), "road": nh_find(body, "roadName"),
                              "comments": clean(comment, 200), "start": start, "end": end, "management": mgmt or kind})
        cache = {"fetched": now, "items": items}
        save("national_highways.json", cache)
        fetched = len(items)
    t = slot(240)
    out = [obs(f"NH-{i['id']}", "land-road-disruption", t, {
        "lat": i["lat"], "lon": i["lon"], "line": i["line"], "severity": i["severity"], "category": i["category"],
        "location": i["location"] or i["road"], "road": i["road"], "comments": i["comments"], "start": i["start"],
        "end": i["end"], "management": i["management"], "source": "national-highways"}) for i in cache["items"]]
    return fetched, out


# ---------------------------------------------------------------- 19. Network Rail train movements

NR_CRED = os.path.join(HERE, "secrets", "network-rail.cred")  # email, password; local only, never committed
RAIL = {"trains": {}, "thread": None, "status": "not started", "stanox": None}


class LoginRefused(ConnectionError):
    """The STOMP broker answered CONNECT with an ERROR frame."""


def rail_stream():
    """Keep one STOMP connection to Network Rail's open data feed (TRAIN_MVT_ALL_TOC,
    reconnecting with back-off) and remember each train's latest movement report."""
    user, pw = [x.strip() for x in open(NR_CRED, encoding="utf-8").read().split("\n")[:2]]
    wait = 5
    while True:
        s = None
        try:
            s = socket.create_connection(("publicdatafeeds.networkrail.co.uk", 61618), timeout=60)

            def send(cmd, headers):
                s.sendall((cmd + "\n" + "".join(f"{k}:{v}\n" for k, v in headers.items()) + "\n\x00").encode())
            send("CONNECT", {"accept-version": "1.2", "host": "/", "login": user, "passcode": pw, "heart-beat": "0,0"})
            buf = b""
            while b"\x00" not in buf:
                chunk = s.recv(65536)
                if not chunk:
                    raise ConnectionError("closed during login")
                buf += chunk
            first, buf = buf.split(b"\x00", 1)
            if not first.lstrip(b"\n").startswith(b"CONNECTED"):
                lines = first.decode(errors="replace").strip().split("\n")
                raise LoginRefused(next((l for l in lines if l.startswith("message:")), lines[0])[:120])
            send("SUBSCRIBE", {"destination": "/topic/TRAIN_MVT_ALL_TOC", "id": "1", "ack": "auto"})
            RAIL["status"] = "connected"
            wait = 5
            while True:
                chunk = s.recv(262144)
                if not chunk:
                    raise ConnectionError("closed")
                buf += chunk
                while b"\x00" in buf:
                    frame, buf = buf.split(b"\x00", 1)
                    frame = frame.decode(errors="replace").lstrip("\n")
                    if frame.startswith("ERROR"):
                        raise ConnectionError(frame[:160])
                    if not frame.startswith("MESSAGE"):
                        continue
                    for m in json.loads(frame.split("\n\n", 1)[1] or "[]"):
                        h, b = m.get("header", {}), m.get("body", {})
                        if h.get("msg_type") != "0003":
                            continue
                        tid = b.get("train_id")
                        if tid:
                            RAIL["trains"][tid] = b
        except Exception as e:  # keep the stream alive across drops
            if s:  # close before waiting, so a dead socket is not held open (CLOSE_WAIT) for the back-off
                s.close()
                s = None
            if isinstance(e, LoginRefused):
                # the broker refused the CONNECT (it answers a wrong or unknown login with the same
                # AMQ339009 "Exception getting session"): back off exponentially to 30 min, not 5 min
                wait = min(max(wait * 2, 60), 1800)
            else:
                wait = min(wait * 2, 300)
            RAIL["status"] = f"reconnecting in {wait} s after {type(e).__name__}: {' '.join(str(e).split())}"[:140]
            time.sleep(wait)
        finally:
            if s:
                s.close()


def src_network_rail():
    """Trains at their last reported station (Network Rail TRUST movements, free account;
    positions from CORPUS and NaPTAN station locations, so a train sits at the last
    station it reported from). Reports from the last 20 minutes; terminated trains drop."""
    import threading
    if RAIL["stanox"] is None:
        RAIL["stanox"] = json.load(open(os.path.join(STATE, "rail", "stanox.json")))
    if RAIL["thread"] is None:
        RAIL["thread"] = threading.Thread(target=rail_stream, name="network-rail", daemon=True)
        RAIL["thread"].start()
        time.sleep(30)  # first cycle: let the stream fill
    now = time.time()
    out = []
    for tid, b in list(RAIL["trains"].items()):
        at = int(b.get("actual_timestamp") or 0) // 1000
        if now - at > 1200 or b.get("train_terminated") == "true":
            if now - at > 7200 or b.get("train_terminated") == "true":
                RAIL["trains"].pop(tid, None)
            continue
        loc = RAIL["stanox"].get(b.get("loc_stanox") or "")
        if not loc:
            continue
        var = int(b.get("timetable_variation") or 0)
        status = (b.get("variation_status") or "").upper()
        late = var if status == "LATE" else -var if status == "EARLY" else 0
        nxt = RAIL["stanox"].get(b.get("next_report_stanox") or "")
        out.append(obs(f"TRAIN-{tid}", "land-train", at, {
            "lat": loc["lat"], "lon": loc["lon"], "alt_m": 0.0, "train_id": tid, "headcode": tid[2:6],
            "station": loc["name"], "crs": loc["crs"], "event": (b.get("event_type") or "").lower() or None,
            "punctuality": status.lower() or None, "late_min": late, "platform": clean(b.get("platform")),
            "toc_id": b.get("toc_id"), "next_station": nxt["name"] if nxt else None, "source": "network-rail"}))
    return f"{len(RAIL['trains'])} ({RAIL['status']})", out


# ---------------------------------------------------------------- 20. Cameras

CAMERAS_FILE = os.path.join(HERE, "cameras.json")  # your own cameras; see cameras.example.json


def src_tfl_jamcams():
    """TfL JamCams: ~900 London traffic cameras, each a still image refreshed every few
    minutes and a ~10 s clip (TfL Unified API, keyless; Powered by TfL Open Data).
    Refetched every 10 min, resent every 5 (observedAt = the 5-minute slot)."""
    cache = load("jamcams.json", {"fetched": 0, "items": []})
    fetched = "cached"
    if time.time() - cache["fetched"] > 600:
        items = []
        for c in get("https://api.tfl.gov.uk/Place/Type/JamCam") or []:
            p = {x["key"]: x["value"] for x in c.get("additionalProperties", [])}
            if c.get("lat") is None or c.get("lon") is None or p.get("available") != "true":
                continue
            items.append({"id": c["id"], "name": clean(c.get("commonName")), "lat": float(c["lat"]), "lon": float(c["lon"]),
                          "view": clean(p.get("view")), "image": p.get("imageUrl"), "video": p.get("videoUrl")})
        cache = {"fetched": time.time(), "items": items}
        save("jamcams.json", cache)
        fetched = len(items)
    t = slot(300)
    return fetched, [obs(c["id"], "land-camera", t, {
        "lat": c["lat"], "lon": c["lon"], "alt_m": 0.0, "name": c["name"], "view": c["view"],
        "image_url": c["image"], "video_url": c["video"], "camera_type": "traffic", "operator": "Transport for London",
        "source": "tfl"}) for c in cache["items"]]


def src_custom_cameras():
    """Your own cameras from feeds/cameras.json (a list; see cameras.example.json): any
    camera with a position and a browser-viewable image, MJPEG, MP4 or HLS URL. Resent
    every 5 min; edit the file and they update on the next cycle."""
    try:
        with open(CAMERAS_FILE) as f:
            cams = json.load(f)
    except FileNotFoundError:
        return 0, []
    t = slot(300)
    out = []
    for c in cams:
        if c.get("lat") is None or c.get("lon") is None or not c.get("id"):
            continue
        out.append(obs(f"CAM-{slug(c['id'])}", "land-camera", t, {
            "lat": float(c["lat"]), "lon": float(c["lon"]), "alt_m": float(c.get("alt_m", 0.0)),
            "name": clean(c.get("name")) or c["id"], "view": clean(c.get("view")),
            "image_url": c.get("image_url"), "video_url": c.get("video_url"), "stream_url": c.get("stream_url"),
            "stream_type": c.get("stream_type"), "camera_type": c.get("camera_type", "custom"),
            "operator": clean(c.get("operator")), "source": "custom"}))
    return len(cams), out



# ---------------------------------------------------------------- 21. GDACS disaster alerts (keyless)

GDACS = "https://www.gdacs.org/gdacsapi/api"
GDACS_TYPES = {"EQ": "earthquake", "TC": "tropical-cyclone", "FL": "flood", "VO": "volcano", "DR": "drought",
               "WF": "wildfire", "TS": "tsunami"}


def gdacs_polygon(f, polys):
    """The largest polygon from GDACS's per-event geometry (orange/red events only), cached
    per event episode; None when there is none."""
    p = f["properties"]
    k = f"{p['eventtype']}-{p['eventid']}-{p.get('episodeid')}"
    if k not in polys:
        polys[k] = None
        url = (p.get("url") or {}).get("geometry")
        if url:
            try:
                rings = [geojson_outer(g.get("geometry")) for g in (get(url, timeout=60) or {}).get("features") or []]
                rings = [r for r in rings if r]
                if rings:
                    polys[k] = poly_text(max(rings, key=lambda r: abs(ring_area(r))))
            except Exception as e:
                log_line("gdacs", f"geometry {k} failed ({type(e).__name__}: {e})")
            time.sleep(0.5)
    return polys[k]


def src_gdacs():
    """GDACS (EC JRC / UN OCHA) current disaster alerts worldwide: earthquakes, tropical
    cyclones, floods, volcanoes, droughts, wildfires. Keyless SEARCH API: every current
    orange/red event (one request), and current green events from the last 14 days (paged,
    newest first, until a page has none current). Polygons for orange/red events from the
    per-event geometry. Refetched every 15 min, resent every 10 (observedAt = the slot)."""
    cache = load("gdacs.json", {"fetched": 0, "events": {}, "polys": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 900:
        evl = "EQ;TC;FL;VO;DR;WF"
        feats = list((get(f"{GDACS}/events/geteventlist/SEARCH?eventlist={evl}&alertlevel=Orange;Red", timeout=60)
                      or {}).get("features") or [])
        since, until = utc(now - 14 * 86400)[:10], utc(now + 86400)[:10]
        for page in range(1, 7):
            time.sleep(1)
            fs = (get(f"{GDACS}/events/geteventlist/SEARCH?eventlist={evl}&alertlevel=Green&fromdate={since}"
                      f"&todate={until}&pagenumber={page}", timeout=60) or {}).get("features") or []
            feats += fs
            if len(fs) < 100 or not any(f["properties"].get("iscurrent") == "true" for f in fs):
                break
        polys = {}
        old = cache.get("polys") or {}
        events = {}
        for f in feats:
            p, g = f.get("properties") or {}, (f.get("geometry") or {}).get("coordinates") or []
            if p.get("iscurrent") != "true" or len(g) < 2 or not p.get("eventid"):
                continue
            level = (p.get("alertlevel") or "").lower()
            ek = f"{p['eventtype']}-{p['eventid']}-{p.get('episodeid')}"
            if ek in old:
                polys[ek] = old[ek]
            poly = gdacs_polygon(f, polys) if level in ("orange", "red") else None
            sev = p.get("severitydata") or {}
            events[f"GDACS-{p['eventtype']}-{p['eventid']}"] = {
                "lat": float(g[1]), "lon": float(g[0]), "event_type": GDACS_TYPES.get(p["eventtype"], p["eventtype"].lower()),
                "name": clean(p.get("name") or p.get("eventname"), 200), "alert_level": level or None,
                "alert_score": num(p.get("alertscore")), "severity": clean(sev.get("severitytext"), 200),
                "severity_value": num(sev.get("severity")), "severity_unit": clean(sev.get("severityunit")),
                "country": clean(p.get("country"), 200), "iso3": clean(p.get("iso3")),
                "from": iso(p.get("fromdate")), "to": iso(p.get("todate")),
                "url": (p.get("url") or {}).get("report"), "polygon": poly, "source": "gdacs"}
        cache = {"fetched": now, "events": events, "polys": polys}
        save("gdacs.json", cache)
        fetched = len(events)
    t = slot(600)
    return fetched, [obs(k, "event-disaster", t, a) for k, a in cache["events"].items()]


# ---------------------------------------------------------------- 22. Meteoalarm warnings (keyless)

# EUMETNET Meteoalarm Atom feeds (CC BY 4.0 equivalent, attribution to meteoalarm.org). No
# feed exists for Albania, Kosovo or Turkey (404).
METEOALARM_COUNTRIES = {"croatia": "HR", "slovenia": "SI", "bosnia-herzegovina": "BA", "serbia": "RS",
                        "montenegro": "ME", "republic-of-north-macedonia": "MK", "bulgaria": "BG",
                        "romania": "RO", "greece": "GR", "cyprus": "CY", "israel": "IL"}
GISCO_NUTS = "https://gisco-services.ec.europa.eu/distribution/v2/nuts/geojson/NUTS_LB_2021_4326_LEVL_{}.geojson"
MA_LEVEL = {"yellow": "yellow", "orange": "amber", "amber": "amber", "red": "red"}


def fetch_nuts_points():
    """Eurostat GISCO NUTS 1-3 label points (keyless): {code: [lat, lon, name, country]}
    for the Meteoalarm countries (Greece is EL in NUTS)."""
    want = set(METEOALARM_COUNTRIES.values()) | {"EL"}
    out = {}
    for level in (1, 2, 3):
        for f in get(GISCO_NUTS.format(level), timeout=90).get("features") or []:
            p, g = f.get("properties") or {}, (f.get("geometry") or {}).get("coordinates") or []
            if p.get("CNTR_CODE") in want and len(g) >= 2:
                out[p["NUTS_ID"]] = [round(g[1], 4), round(g[0], 4), p.get("NAME_LATN") or p.get("NUTS_NAME"),
                                     "GR" if p["CNTR_CODE"] == "EL" else p["CNTR_CODE"]]
    return out


MA_COUNTRY_POINT = {"HR": (45.1, 15.2), "SI": (46.1, 14.8), "BA": (44.2, 17.8), "RS": (44.0, 20.9),
                    "ME": (42.8, 19.3), "MK": (41.6, 21.7), "BG": (42.7, 25.3), "RO": (45.9, 24.9),
                    "GR": (39.1, 22.0), "CY": (35.0, 33.2), "IL": (31.4, 34.9)}


def _ascii(text):
    import unicodedata
    return slug(unicodedata.normalize("NFKD", text or "").encode("ascii", "ignore").decode())


def ma_point(codes, area, country, nuts):
    """A warning area's point and how it was found: its NUTS3 code's label point, else a
    NUTS region of the same country whose name matches the area description, else the
    country's centre (Bosnia and Herzegovina and some Greek areas have no NUTS match)."""
    for c in codes:
        if c in nuts:
            return nuts[c][:2], "nuts"
    a = _ascii(area)
    if a:
        for exact in (True, False):
            for code, (lat, lon, name, cc) in nuts.items():
                n = _ascii(name)
                if cc == country and (n == a if exact else len(n) > 3 and (a in n or n in a)):
                    return [lat, lon], "nuts-name"
    p = MA_COUNTRY_POINT.get(country)
    return (list(p), "country") if p else (None, None)


def src_meteoalarm():
    """Meteoalarm awareness warnings (yellow / orange -> 'amber' / red; green skipped) for
    the Balkans, Greece, Cyprus and Israel. Polygon from the CAP when the feed gives one,
    else the NUTS label point of the area (Eurostat GISCO, cached daily). Refetched every
    15 min, resent every 10 (observedAt = the slot); the same kind as the Met Office feed."""
    import xml.etree.ElementTree as ET
    cache = load("meteoalarm.json", {"fetched": 0, "warnings": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 900:
        _, nuts_cache = daily_cache("nuts_points.json", fetch_nuts_points)
        nuts = nuts_cache["items"]
        ns = {"a": "http://www.w3.org/2005/Atom", "cap": "urn:oasis:names:tc:emergency:cap:1.2"}
        warnings, unplaced, failed = {}, 0, []
        for i, (feed, cc) in enumerate(METEOALARM_COUNTRIES.items()):
            if i:
                time.sleep(0.5)
            try:
                root = ET.fromstring(get_text(f"https://feeds.meteoalarm.org/feeds/meteoalarm-legacy-atom-{feed}"))
            except Exception as e:  # one country failing must not lose the rest
                failed.append(f"{feed}:{type(e).__name__}{getattr(e, 'code', '')}")
                continue
            for e in root.findall("a:entry", ns):
                title = clean(e.findtext("a:title", namespaces=ns)) or ""
                m = re.match(r"(?i)(green|yellow|orange|amber|red)\s+(.+?)\s+warning\b", title)
                if not m or m.group(1).lower() == "green":
                    continue
                expires = iso(e.findtext("cap:expires", namespaces=ns))
                if expires and expires < now:
                    continue
                cap = next((l.get("href") for l in e.findall("a:link", ns) if l.get("type") == "application/cap+xml"), None)
                eid = e.findtext("a:id", namespaces=ns) or ""
                area_idx = (re.search(r"index_area=(\d+)", eid) or [None, "0"])[1]
                uuid = (cap or eid).rstrip("/").split("/")[-1].split("?")[0]
                key = f"MA-{cc}-{uuid[:36]}-{area_idx}"
                area = clean(e.findtext("cap:areaDesc", namespaces=ns))
                codes = [clean(g.findtext("value")) for g in e.findall("cap:geocode", ns)]
                w = warnings.get(key)
                if w is None:
                    poly_raw = clean(e.findtext("cap:polygon", namespaces=ns))
                    ring = []
                    for pair in (poly_raw or "").split():
                        try:
                            la, lo = pair.split(",")
                            ring.append((float(la), float(lo)))
                        except ValueError:
                            pass
                    weather = m.group(2).lower().replace("coastalevent", "coastal event").replace("-", " ")
                    w = {"lat": None, "lon": None, "polygon": poly_text(ring) if len(ring) >= 3 else None,
                         "level": MA_LEVEL.get(m.group(1).lower()), "weather": weather, "region": area,
                         "country": cc, "title": title, "severity": clean(e.findtext("cap:severity", namespaces=ns)),
                         "valid_from": iso(e.findtext("cap:onset", namespaces=ns) or e.findtext("cap:effective", namespaces=ns)),
                         "valid_to": expires, "link": f"https://meteoalarm.org?region={cc}", "cap_url": cap,
                         "warning_id": clean(e.findtext("cap:identifier", namespaces=ns)), "codes": [],
                         "source": "meteoalarm"}
                    if len(ring) >= 3:
                        w["lat"], w["lon"] = (round(c, 4) for c in centroid(ring))
                    warnings[key] = w
                w["codes"] += [c for c in codes if c]
        for w in warnings.values():
            if w["lat"] is None:
                p, how = ma_point(w["codes"], w["region"], w["country"], nuts)
                if p:
                    w["lat"], w["lon"], w["position"] = p[0], p[1], how
            else:
                w["position"] = "polygon"
        if len(failed) == len(METEOALARM_COUNTRIES):
            raise RuntimeError(f"every Meteoalarm feed failed: {', '.join(failed)}")
        placed = {k: {a: v for a, v in w.items() if a != "codes"} for k, w in warnings.items() if w["lat"] is not None}
        unplaced = len(warnings) - len(placed)
        if failed or unplaced:
            log_line("meteoalarm", f"feeds failed: {', '.join(failed) or 'none'}; {unplaced} warnings with no position skipped")
        cache = {"fetched": now, "warnings": placed}
        save("meteoalarm.json", cache)
        fetched = len(placed)
    t = slot(600)
    return fetched, [obs(k, "event-weather-warning", t, a) for k, a in cache["warnings"].items()
                     if not a.get("valid_to") or a["valid_to"] > now]


# ---------------------------------------------------------------- 23. Singapore traffic cameras (keyless)

def src_sg_cams():
    """Singapore LTA traffic cameras via data.gov.sg (keyless, Singapore Open Data Licence):
    a still image per camera, its URL changing with each new image (every few minutes).
    Fetched and resent every 5 min (observedAt = the 5-minute slot)."""
    d = get("https://api.data.gov.sg/v1/transport/traffic-images")
    cams = ((d.get("items") or [{}])[0]).get("cameras") or []
    t = slot(300)
    out = []
    for c in cams:
        loc = c.get("location") or {}
        if loc.get("latitude") is None or not c.get("camera_id") or not c.get("image"):
            continue
        out.append(obs(f"SGCAM-{c['camera_id']}", "land-camera", t, {
            "lat": float(loc["latitude"]), "lon": float(loc["longitude"]), "alt_m": 0.0,
            "name": f"LTA camera {c['camera_id']}", "view": None, "image_url": c["image"],
            "image_time": iso(c.get("timestamp")), "camera_type": "traffic", "operator": "LTA Singapore",
            "source": "data.gov.sg"}))
    return len(cams), out


# ---------------------------------------------------------------- 24. New Zealand traffic cameras (keyless)

NZTA = "https://trafficnz.info"


def src_nz_cams():
    """NZ Transport Agency Waka Kotahi state highway cameras (trafficnz.info REST v4,
    keyless public JSON; NZTA traffic data is published for reuse with attribution).
    Offline or under-maintenance cameras skipped. Refetched every 10 min, resent every 5
    (observedAt = the 5-minute slot); the image URL is fixed and always the latest frame."""
    cache = load("nz_cams.json", {"fetched": 0, "items": []})
    fetched = "cached"
    if time.time() - cache["fetched"] > 600:
        req = urllib.request.Request(f"{NZTA}/service/traffic/rest/4/cameras/all", headers=UA)
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                d = json.load(r)
        except urllib.error.HTTPError as e:
            if e.code == 429:
                raise RateLimited(req.full_url) from e
            raise
        cams = (d.get("response") or {}).get("camera") or []
        if isinstance(cams, dict):
            cams = [cams]
        items = []
        for c in cams:
            if c.get("offline") or c.get("underMaintenance") or c.get("latitude") is None or not c.get("imageUrl"):
                continue
            items.append({"id": c["id"], "lat": float(c["latitude"]), "lon": float(c["longitude"]),
                          "name": clean(c.get("name")), "view": clean(" - ".join(x for x in (c.get("description"), c.get("direction")) if x)),
                          "image": NZTA + c["imageUrl"], "region": clean((c.get("region") or {}).get("name")),
                          "highway": clean(c.get("highway"))})
        cache = {"fetched": time.time(), "items": items}
        save("nz_cams.json", cache)
        fetched = f"{len(cams)} online={len(items)}"
    t = slot(300)
    return fetched, [obs(f"NZCAM-{c['id']}", "land-camera", t, {
        "lat": c["lat"], "lon": c["lon"], "alt_m": 0.0, "name": c["name"], "view": c["view"],
        "image_url": c["image"], "region": c["region"], "road": c["highway"], "camera_type": "traffic",
        "operator": "NZ Transport Agency Waka Kotahi", "source": "nzta"}) for c in cache["items"]]


# ---------------------------------------------------------------- 25. Regional keyless feeds (2026-10-18)
# Each is official, needs no key or sign-up and is published for reuse with attribution:
#   eccc-alerts      Environment and Climate Change Canada, GeoMet OGC API weather-alerts
#                    (ECCC Data Servers End-use Licence; alert text reproduced unaltered)
#   drivebc-cams     BC Ministry of Transportation DriveBC webcams (Open Government Licence - BC)
#   drivebc-events   DriveBC Open511 road events (Open Government Licence - BC)
#   mrwa-roads       Main Roads Western Australia WebEOC road incidents and closures (CC BY 4.0)
#   nzta-roads       NZ Transport Agency Waka Kotahi trafficnz.info road events (reuse with attribution)
#   metservice-warnings  MetService NZ public CAP watches/warnings (CC BY 4.0)

ECCC_LEVEL = {"red": "red", "orange": "amber", "yellow": "yellow"}


def latlon_of(geom):
    """A representative (lat, lon) of a GeoJSON geometry: polygon centroid, line midpoint, point."""
    t = (geom or {}).get("type")
    c = (geom or {}).get("coordinates")
    if not c:
        return None
    if t == "Point":
        return float(c[1]), float(c[0])
    if t in ("Polygon", "MultiPolygon"):
        ring = geojson_outer(geom)
        return centroid(ring) if ring else None
    if t == "LineString":
        p = c[len(c) // 2]
        return float(p[1]), float(p[0])
    if t == "MultiLineString":
        longest = max(c, key=len)
        p = longest[len(longest) // 2]
        return float(p[1]), float(p[0])
    return None


def line_of(geom):
    """GeoJSON LineString / MultiLineString (its longest part) -> 'lat,lon;...'."""
    t = (geom or {}).get("type")
    c = (geom or {}).get("coordinates") or []
    if t == "MultiLineString" and c:
        c = max(c, key=len)
    elif t != "LineString":
        return None
    pts = [(float(p[1]), float(p[0])) for p in c]
    return poly_text(pts) if len(pts) >= 2 else None


def src_eccc_alerts():
    """Environment and Climate Change Canada public weather alerts (GeoMet OGC API, keyless;
    ECCC Data Servers End-use Licence, attribution 'Environment and Climate Change Canada').
    One entity per alert area with its polygon; ECCC risk colour yellow / orange ('amber') /
    red, anything else (statements, grey) as yellow; ended alerts skipped. Refetched every
    10 min, resent every 10 (observedAt = the slot)."""
    cache = load("eccc_alerts.json", {"fetched": 0, "warnings": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 600:
        d = get("https://api.weather.gc.ca/collections/weather-alerts/items?f=json&lang=en&limit=2000", timeout=60)
        feats = d.get("features") or []
        warnings = {}
        for f in feats:
            p = f.get("properties") or {}
            if str(p.get("status_en", "")).lower() == "ended":
                continue
            end = iso(p.get("event_end_datetime"))
            if end and end < now:
                continue
            ring = geojson_outer(f.get("geometry"))
            if not ring:
                continue
            lat, lon = (round(v, 4) for v in centroid(ring))
            fid = p.get("feature_id") or str(f.get("id", "")).split("_")[-1]
            code = p.get("alert_code") or slug(p.get("alert_name_en") or "alert")
            key = f"ECCC-{code}-{fid}"
            name = clean(p.get("alert_name_en"))
            warnings[key] = {
                "lat": lat, "lon": lon, "polygon": poly_text(ring),
                "level": ECCC_LEVEL.get(str(p.get("risk_colour_en", "")).lower(), "yellow"),
                "weather": clean(p.get("alert_short_name_en")) or name, "alert_type": clean(p.get("alert_type")),
                "region": clean(p.get("feature_name_en")), "province": clean(p.get("province")),
                "title": name.capitalize() if name else None, "status": clean(p.get("status_en")),
                "valid_from": iso(p.get("validity_datetime")), "valid_to": end,
                "issued": iso(p.get("publication_datetime")), "text": clean(p.get("alert_text_en"), 1000),
                "confidence": clean(p.get("confidence_en")), "impact": clean(p.get("impact_en")),
                "link": "https://weather.gc.ca/warnings/index_e.html", "country": "CA",
                "attribution": "Environment and Climate Change Canada", "source": "eccc"}
        cache = {"fetched": now, "warnings": warnings}
        save("eccc_alerts.json", cache)
        fetched = f"{len(feats)} active={len(warnings)}"
    t = slot(600)
    return fetched, [obs(k, "event-weather-warning", t, a) for k, a in cache["warnings"].items()
                     if not a.get("valid_to") or a["valid_to"] > now]


DRIVEBC = "https://www.drivebc.ca"


def src_drivebc_cams():
    """British Columbia highway webcams (DriveBC, keyless JSON; Open Government Licence -
    British Columbia). Cameras switched off or marked stale skipped. Refetched every 10 min,
    resent every 5 (observedAt = the 5-minute slot); the image URL is the latest frame."""
    cache = load("drivebc_cams.json", {"fetched": 0, "items": []})
    fetched = "cached"
    if time.time() - cache["fetched"] >= 600:
        cams = get(f"{DRIVEBC}/api/webcams/", timeout=60) or []
        items = []
        for c in cams:
            coords = (c.get("location") or {}).get("coordinates")
            if not c.get("is_on") or not c.get("should_appear") or c.get("marked_stale") or not coords:
                continue
            img = (c.get("links") or {}).get("imageDisplay") or f"/images/{c['id']}.jpg"
            items.append({"id": c["id"], "lat": float(coords[1]), "lon": float(coords[0]),
                          "name": clean(c.get("name_override") or c.get("name")),
                          "view": clean(c.get("caption_override") or c.get("caption"), 200),
                          "image": DRIVEBC + img.split("?")[0], "region": clean(c.get("region_name")),
                          "road": clean(f"Highway {c['highway']}" if c.get("highway") else None),
                          "orientation": clean(c.get("orientation")), "elevation_m": num(c.get("elevation")),
                          "delayed": bool(c.get("marked_delayed"))})
        cache = {"fetched": time.time(), "items": items}
        save("drivebc_cams.json", cache)
        fetched = f"{len(cams)} online={len(items)}"
    t = slot(300)
    return fetched, [obs(f"BCCAM-{c['id']}", "land-camera", t, {
        "lat": c["lat"], "lon": c["lon"], "alt_m": 0.0, "name": c["name"], "view": c["view"],
        "image_url": c["image"], "region": c["region"], "road": c["road"], "orientation": c["orientation"],
        "elevation_m": c["elevation_m"], "delayed": c["delayed"], "camera_type": "traffic",
        "operator": "BC Ministry of Transportation and Transit (DriveBC)", "source": "drivebc"}) for c in cache["items"]]


def src_drivebc_events():
    """British Columbia road events (DriveBC Open511 API, keyless; Open Government Licence -
    British Columbia). Active events; a closed road is 'Severe' (has_closures), a MAJOR event
    'Serious', the rest 'Moderate'. Line geometry kept (<= 200 vertices). Refetched every
    5 min, resent every 4 (observedAt = the 4-minute slot)."""
    cache = load("drivebc_events.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 300:
        evs = []
        for offset in range(0, 2000, 500):  # the API answers HTTP 500 to a limit above 500
            page = get(f"https://api.open511.gov.bc.ca/events?format=json&status=ACTIVE&limit=500&offset={offset}",
                       timeout=60).get("events") or []
            evs += page
            if len(page) < 500:
                break
        items = {}
        for e in evs:
            geom = e.get("geography") or {}
            p = latlon_of(geom)
            if not p or not e.get("id"):
                continue
            roads = e.get("roads") or []
            closed = any(str(r.get("state", "")).upper() == "CLOSED" for r in roads)
            sev = "Severe" if closed else "Serious" if e.get("severity") == "MAJOR" else "Moderate"
            start = end = None
            iv = ((e.get("schedule") or {}).get("intervals") or [""])[0].split("/")
            try:
                start = iso(iv[0]) if iv[0] else None
                end = iso(iv[1]) if len(iv) > 1 and iv[1] else None
            except ValueError:
                pass
            if end and end < now:
                continue
            r0 = roads[0] if roads else {}
            items["BC511-" + slug(e["id"].split("/")[-1])] = {
                "lat": round(p[0], 5), "lon": round(p[1], 5), "severity": sev,
                "category": clean(str(e.get("event_type", "")).replace("_", " ").title()),
                "sub_category": clean(", ".join(s.replace("_", " ").lower() for s in e.get("event_subtypes") or [])),
                "road": clean(r0.get("name")), "direction": clean(r0.get("direction")),
                "road_state": clean(str(r0.get("state", "")).replace("_", " ").lower()),
                "location": clean(", ".join(a.get("name", "") for a in e.get("areas") or []) or None),
                "comments": clean(e.get("description"), 300), "start": start, "end": end,
                "has_closures": closed, "line": line_of(geom), "updated": iso(e.get("updated")),
                "link": "https://www.drivebc.ca/", "region": "british-columbia", "source": "drivebc-open511"}
        cache = {"fetched": now, "items": items}
        save("drivebc_events.json", cache)
        fetched = f"{len(evs)} placed={len(items)}"
    t = slot(240)
    return fetched, [obs(k, "land-road-disruption", t, a) for k, a in cache["items"].items()
                     if not a.get("end") or a["end"] > now]


MRWA = "https://services2.arcgis.com/cHGEnmsJ165IBJRM/arcgis/rest/services"
MRWA_LAYERS = (("incident", "WebEoc_RoadIncidents/FeatureServer/1"), ("closure", "WebEoc_RoadClosures/FeatureServer/4"))


def wa_time(text):
    """Main Roads WA 'dd/mm/yyyy hh:mm:ss' local time (AWST, UTC+8, no daylight saving)."""
    try:
        d = datetime.strptime(str(text).strip(), "%d/%m/%Y %H:%M:%S")
    except (TypeError, ValueError):
        return None
    return int(d.replace(tzinfo=timezone.utc).timestamp()) - 8 * 3600


def src_mrwa_roads():
    """Western Australia road incidents and closures (Main Roads WA WebEOC open data on
    ArcGIS, keyless; CC BY 4.0, attribution Main Roads Western Australia). A closed road
    is 'Severe' (has_closures), other closures and incidents with lane closures 'Serious',
    the rest 'Moderate'. Refetched every 5 min, resent every 4 (the 4-minute slot)."""
    cache = load("mrwa_roads.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 300:
        items, counts = {}, []
        for kind, layer in MRWA_LAYERS:
            feats = arcgis_geojson(f"{MRWA}/{layer}/query", "*")
            counts.append(f"{kind}s={len(feats)}")
            for f in feats:
                a = f.get("properties") or {}
                geom = f.get("geometry")
                p = latlon_of(geom)
                if not p or str(a.get("publishExt", "Yes")).lower() != "yes":
                    continue
                state = clean(a.get("ClosureTyp")) or clean(a.get("TrafficCon")) or ""
                st = state.lower()
                closed = st.startswith("road closed") or st in ("closed", "all lanes closed")
                sev = ("Severe" if closed else
                       "Serious" if kind == "closure" or "closed" in st else "Moderate")
                items[f"WA-{kind[:3].upper()}-{a.get('Id') or a.get('GlobalID')}"] = {
                    "lat": round(p[0], 5), "lon": round(p[1], 5), "severity": sev,
                    "category": clean(a.get("IncidentTy")) or kind, "road": clean(a.get("Road") or a.get("LocalRoadName")),
                    "location": clean(a.get("Location"), 200), "suburb": clean(a.get("Suburb")),
                    "district": clean(a.get("Region")), "road_state": clean(state),
                    "comments": clean(a.get("TrafficImp"), 300), "start": wa_time(a.get("EntryDate")),
                    "updated": wa_time(a.get("UpdateDate")), "has_closures": closed, "line": line_of(geom),
                    "link": clean(a.get("SeeMoreUrl")) or "https://travelmap.mainroads.wa.gov.au/",
                    "region": "western-australia", "source": "mainroads-wa"}
        cache = {"fetched": now, "items": items}
        save("mrwa_roads.json", cache)
        fetched = " ".join(counts)
    t = slot(240)
    return fetched, [obs(k, "land-road-disruption", t, a) for k, a in cache["items"].items()]


NZ_IMPACT = {"road closed": "Severe", "delays": "Serious"}


def wkt_points(text):
    """WKT POINT / LINESTRING / MULTILINESTRING -> (representative (lat, lon), longest line)."""
    parts = re.findall(r"\(([^()]+)\)", text or "")
    lines = []
    for part in parts:
        pts = []
        for pair in part.split(","):
            xy = pair.split()
            if len(xy) >= 2:
                try:
                    pts.append((float(xy[1]), float(xy[0])))
                except ValueError:
                    pass
        if pts:
            lines.append(pts)
    if not lines:
        return None, None
    best = max(lines, key=len)
    return best[len(best) // 2], best if len(best) >= 2 else None


def src_nzta_roads():
    """New Zealand state highway road events (NZ Transport Agency Waka Kotahi trafficnz.info
    REST v4, keyless public JSON). Active events only; 'Road Closed' is 'Severe'
    (has_closures), 'Delays' 'Serious', the rest 'Moderate'. Refetched every 5 min, resent
    every 4 (observedAt = the 4-minute slot)."""
    cache = load("nzta_roads.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 300:
        d = get(f"{NZTA}/service/traffic/rest/4/events/all/10", timeout=60)
        evs = (d.get("response") or {}).get("roadevent") or []
        if isinstance(evs, dict):
            evs = [evs]
        items = {}
        for e in evs:
            if str(e.get("status", "")).lower() != "active" or not e.get("id"):
                continue
            p, line = wkt_points(e.get("geometry"))
            if not p:
                j = e.get("journey") or {}
                if j.get("startLatitude") is None:
                    continue
                p = (float(j["startLatitude"]), float(j["startLongitude"]))
            end = iso(e.get("endDate"))
            if end and end < now:
                continue
            impact = clean(e.get("impact")) or ""
            closed = impact.lower() == "road closed"
            items[f"NZRD-{e['id']}"] = {
                "lat": round(p[0], 5), "lon": round(p[1], 5), "severity": NZ_IMPACT.get(impact.lower(), "Moderate"),
                "category": clean(e.get("eventType")), "sub_category": clean(e.get("eventDescription")),
                "impact": impact or None, "road": clean((e.get("journey") or {}).get("name")),
                "location": clean(e.get("locationArea"), 200), "district": clean(e.get("region") if isinstance(e.get("region"), str) else (e.get("region") or {}).get("name")),
                "comments": clean(e.get("eventComments"), 300), "alternative_route": clean(e.get("alternativeRoute"), 200),
                "expected_resolution": clean(e.get("expectedResolution")), "planned": str(e.get("planned")).lower() == "true",
                "start": iso(e.get("startDate")), "end": end, "updated": iso(e.get("eventModified")),
                "has_closures": closed, "line": poly_text(line) if line else None,
                "link": "https://www.journeys.nzta.govt.nz/", "region": "new-zealand", "source": "nzta"}
        cache = {"fetched": now, "items": items}
        save("nzta_roads.json", cache)
        fetched = f"{len(evs)} active={len(items)}"
    t = slot(240)
    return fetched, [obs(k, "land-road-disruption", t, a) for k, a in cache["items"].items()
                     if not a.get("end") or a["end"] > now]


CAP_NS = {"cap": "urn:oasis:names:tc:emergency:cap:1.2"}
CAP_SEVERITY_LEVEL = {"extreme": "red", "severe": "amber", "moderate": "yellow", "minor": "yellow"}


def src_metservice():
    """MetService New Zealand public watches and warnings (CAP 1.2 via alerts.metservice.com,
    keyless; CC BY 4.0, attribution MetService). One entity per CAP area with its polygon;
    the level from MetService's ColourCode parameter (Yellow / Orange -> 'amber' / Red),
    else from the CAP severity. Refetched every 10 min, resent every 10 (the slot)."""
    import xml.etree.ElementTree as ET
    cache = load("metservice.json", {"fetched": 0, "warnings": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 600:
        rss = ET.fromstring(get_text("https://alerts.metservice.com/cap/rss", accept="application/rss+xml"))
        links = [clean(i.findtext("link")) for i in rss.iter("item")]
        warnings, failed = {}, 0
        for i, url in enumerate(l for l in links if l):
            if i:
                time.sleep(0.3)
            try:
                cap = ET.fromstring(get_text(url, accept="application/xml"))
            except Exception:
                failed += 1
                continue
            ident = clean(cap.findtext("cap:identifier", namespaces=CAP_NS)) or url
            if (cap.findtext("cap:msgType", namespaces=CAP_NS) or "").lower() == "cancel":
                continue
            for info in cap.findall("cap:info", CAP_NS):
                lang = (info.findtext("cap:language", namespaces=CAP_NS) or "en").lower()
                if not lang.startswith("en"):
                    continue
                expires = iso(info.findtext("cap:expires", namespaces=CAP_NS))
                if expires and expires < now:
                    continue
                params = {clean(p.findtext("cap:valueName", namespaces=CAP_NS) or "").lower():
                          clean(p.findtext("cap:value", namespaces=CAP_NS)) for p in info.findall("cap:parameter", CAP_NS)}
                colour = (params.get("colourcode") or "").lower()
                level = MA_LEVEL.get(colour) or CAP_SEVERITY_LEVEL.get(
                    (info.findtext("cap:severity", namespaces=CAP_NS) or "").lower(), "yellow")
                for n, area in enumerate(info.findall("cap:area", CAP_NS)):
                    ring = []
                    for poly in area.findall("cap:polygon", CAP_NS):
                        pts = []
                        for pair in (poly.text or "").split():
                            try:
                                la, lo = pair.split(",")
                                pts.append((float(la), float(lo)))
                            except ValueError:
                                pass
                        if len(pts) > len(ring):
                            ring = pts
                    if len(ring) < 3:
                        continue
                    lat, lon = (round(v, 4) for v in centroid(ring))
                    warnings[f"MS-{slug(ident)[-40:]}-{n}"] = {
                        "lat": lat, "lon": lon, "polygon": poly_text(ring), "level": level,
                        "weather": clean(info.findtext("cap:event", namespaces=CAP_NS)),
                        "region": clean(area.findtext("cap:areaDesc", namespaces=CAP_NS), 200),
                        "title": clean(info.findtext("cap:headline", namespaces=CAP_NS)),
                        "text": clean(info.findtext("cap:description", namespaces=CAP_NS), 1000),
                        "instruction": clean(info.findtext("cap:instruction", namespaces=CAP_NS), 500),
                        "severity": clean(info.findtext("cap:severity", namespaces=CAP_NS)),
                        "valid_from": iso(info.findtext("cap:onset", namespaces=CAP_NS) or info.findtext("cap:effective", namespaces=CAP_NS)),
                        "valid_to": expires, "link": clean(info.findtext("cap:web", namespaces=CAP_NS)) or "https://www.metservice.com/warnings/home",
                        "cap_url": url, "warning_id": ident, "country": "NZ",
                        "attribution": "MetService (CC BY 4.0)", "source": "metservice"}
        if links and failed == len(links):
            raise RuntimeError(f"every MetService CAP document failed ({failed})")
        cache = {"fetched": now, "warnings": warnings}
        save("metservice.json", cache)
        fetched = f"{len(links)} areas={len(warnings)}"
    t = slot(600)
    return fetched, [obs(k, "event-weather-warning", t, a) for k, a in cache["warnings"].items()
                     if not a.get("valid_to") or a["valid_to"] > now]


# ---------------------------------------------------------------- 30. Utilities: power (GB)
#
# Live power cuts from three distribution network operators (keyless, open licences):
# UK Power Networks (CC BY 4.0), Northern Powergrid (Northern Powergrid Open Data Licence:
# "Supported by Northern Powergrid Open Data") and NGED (connecteddata.nationalgrid.co.uk).
# GB grid status from Elexon Insights ("Contains BMRS data (c) Elexon Limited copyright and
# database right") and NESO's Carbon Intensity API (CC BY 4.0). The fixed grid (400/275/132
# kV lines, major substations, power stations) from OpenStreetMap (ODbL, "(c) OpenStreetMap
# contributors"), snapshotted weekly in the background. SP Energy Networks and Electricity
# North West need a free key and are not included; SSEN and NIE publish no licence.

from zoneinfo import ZoneInfo

LONDON = ZoneInfo("Europe/London")
UKPN_CUTS = "https://ukpowernetworks.opendatasoft.com/api/explore/v2.1/catalog/datasets/ukpn-live-faults/exports/json"
NPG_CUTS = "https://northernpowergrid.opendatasoft.com/api/explore/v2.1/catalog/datasets/live-power-cuts-data/exports/json"
NGED_CUTS = ("https://connecteddata.nationalgrid.co.uk/dataset/d6672e1e-c684-4cea-bb78-c7e5248b62a2/resource/"
             "a1365982-4e05-463c-8304-8323a2ba0ccd/download/live_detailed_power_cuts.csv")
ELEXON = "https://data.elexon.co.uk/bmrs/api/v1"
CARBON = "https://api.carbonintensity.org.uk"
OVERPASS = ("https://overpass-api.de/api/interpreter", "https://overpass.private.coffee/api/interpreter")


def get_text_gz(url, timeout=TIMEOUT):
    req = urllib.request.Request(url, headers={**UA, "Accept": "*/*", "Accept-Encoding": "gzip"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            if r.headers.get("Content-Encoding") == "gzip":
                raw = gzip.decompress(raw)
            return raw.decode("utf-8-sig", errors="replace")
    except urllib.error.HTTPError as e:
        if e.code == 429:
            raise RateLimited(url) from e
        raise


def local_time(ts):
    """A UK local time without an offset (as UKPN and NGED give them) to Unix seconds;
    placeholders (UKPN's 0001-01-01 on grouped incidents) are None."""
    if not ts or str(ts) in ("None", ""):
        return None
    try:
        d = datetime.fromisoformat(str(ts).strip().replace("Z", "+00:00"))
    except ValueError:
        return None
    if d.tzinfo is None:
        d = d.replace(tzinfo=LONDON)
    return int(d.timestamp()) if d.year >= 2000 else None


def as_int(v):
    try:
        return int(float(v))
    except (TypeError, ValueError):
        return None


def cut(dno, ref, lat, lon, planned, customers, **extra):
    customers = customers or None  # 0 means "not given" in these feeds
    return {"lat": round(float(lat), 5), "lon": round(float(lon), 5), "network": "power", "operator": dno,
            "ref": ref, "planned": bool(planned), "customers": customers,
            # The count when the operator gives one, else who reported it.
            "name": f"{'Planned work' if planned else 'Power cut'} · " + (f"{customers} customers" if customers is not None else dno),
            **extra}


def fetch_power_cuts():
    items, failed = {}, []
    try:
        for r in get(UKPN_CUTS, timeout=60):
            g = r.get("geopoint") or {}
            if r.get("powercuttype") == "Restored" or r.get("restoreddatetime") or g.get("lat") is None:
                continue
            planned = r.get("powercuttype") == "Planned"
            customers = as_int(r.get("noplannedcustomers") if planned else r.get("nocustomeraffected"))
            items[f"PWR-UKPN-{slug(r.get('incidentreference'))}"] = cut(
                "UK Power Networks", r.get("incidentreference"), g["lat"], g["lon"], planned, customers,
                started=local_time(r.get("planneddate") if planned else r.get("creationdatetime")),
                etr=local_time(r.get("estimatedrestorationdate")), area=clean(r.get("operatingzone"), 80),
                postcodes=clean(r.get("postcodesaffected"), 120),
                cause=clean(r.get("plannedincidentreason") if planned else r.get("incidentcategorycustomerfriendlydescription"), 160),
                source="ukpn")
    except Exception as e:
        failed.append(f"ukpn {type(e).__name__}")
    try:
        for r in get(NPG_CUTS, timeout=60):
            if r.get("lat") is None or r.get("lng") is None:
                continue
            planned = "planned" in str(r.get("natureofoutage") or "").lower()
            customers = as_int(r.get("totalconfirmedpowercut")) or as_int(r.get("totalpredictedpowercut"))
            pcs = r.get("postcode") or []
            items[f"PWR-NPG-{slug(r.get('reference'))}"] = cut(
                "Northern Powergrid", r.get("reference"), r["lat"], r["lng"], planned, customers,
                started=iso(r.get("loggedtime")), etr=iso(r.get("estimatedtimetillresolution")),
                area=clean(r.get("area"), 80), voltage=clean(r.get("type")),
                postcodes=clean(", ".join(pcs[:6]) if isinstance(pcs, list) else pcs, 120),
                cause=clean(r.get("powercutcategory"), 160), calls=as_int(r.get("numberofcalls")), source="northern-powergrid")
    except Exception as e:
        failed.append(f"npg {type(e).__name__}")
    try:
        for r in csv.DictReader(io.StringIO(get_text_gz(NGED_CUTS, timeout=60))):
            st = (r.get("status") or "").lower()
            if "restor" in st or "complete" in st or not r.get("location_latitude"):
                continue
            planned = (r.get("planned") or "").lower() == "true"
            customers = as_int(r.get("confirmed_off")) or as_int(r.get("predicted_off")) or as_int(r.get("planned_outage_off"))
            items[f"PWR-NGED-{slug(r.get('fault_id'))}"] = cut(
                "NGED", r.get("fault_id"), r["location_latitude"], r["location_longitude"], planned, customers,
                started=local_time(r.get("planned_outage_start_date") if planned else r.get("date_of_reported_fault")),
                etr=local_time(r.get("etr") or r.get("planned_outage_end_date")), area=clean(r.get("licence_area"), 80),
                voltage=clean(r.get("voltage")), status=clean(r.get("status")), postcodes=clean(r.get("postcode"), 120),
                cause=clean(r.get("category"), 160), source="nged")
    except Exception as e:
        failed.append(f"nged {type(e).__name__}")
    if failed:
        log_line("power-cuts", f"failed: {', '.join(failed)}")
        if len(failed) == 3:
            raise RuntimeError("every power-cut source failed")
    return items


def src_power_cuts():
    """Live power cuts and planned work (GB DNOs), refetched every 5 min and resent each
    5-min slot (Heimdall TTL 900 s, so a restored cut leaves within 15 min)."""
    cache = load("power_cuts.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    if time.time() - cache["fetched"] >= 300:
        cache = {"fetched": time.time(), "items": fetch_power_cuts()}
        save("power_cuts.json", cache)
        fetched = len(cache["items"])
    t = slot(300)
    return fetched, [obs(k, "utility-power-outage", t, a) for k, a in cache["items"].items()]


# The GB interconnectors: Elexon fuel code, name, capacity (MW), GB end, far end and a
# point between (routes approximate).
INTERCONNECTORS = {
    "INTFR": ("IFA", "France", 2000, (51.106, 0.980), (50.890, 1.780), (51.02, 1.40)),
    "INTIFA2": ("IFA2", "France", 1000, (50.820, -1.270), (49.120, -0.100), (50.20, -0.80)),
    "INTELEC": ("ElecLink", "France", 1000, (51.098, 1.130), (50.930, 1.810), (51.02, 1.47)),
    "INTNED": ("BritNed", "Netherlands", 1000, (51.443, 0.700), (51.960, 4.020), (51.75, 2.40)),
    "INTNEM": ("Nemo Link", "Belgium", 1000, (51.300, 1.350), (51.290, 3.200), (51.33, 2.30)),
    "INTNSL": ("North Sea Link", "Norway", 1400, (55.140, -1.510), (59.520, 6.640), (57.40, 2.50)),
    "INTVKL": ("Viking Link", "Denmark", 1400, (52.930, -0.200), (55.530, 9.120), (54.40, 4.50)),
    "INTEW": ("East West", "Ireland", 500, (53.230, -3.070), (53.530, -6.500), (53.45, -4.80)),
    "INTIRL": ("Moyle", "Northern Ireland", 500, (55.070, -4.980), (54.840, -5.770), (55.00, -5.40)),
    "INTGRNL": ("Greenlink", "Ireland", 500, (51.680, -4.990), (52.270, -6.980), (51.95, -6.00)),
}
# INTOUTHH names Elexon uses -> fuel code
IC_NAMES = {"Eleclink (INTELEC)": "INTELEC", "Ireland(East-West)": "INTEW", "France(IFA)": "INTFR",
            "Ireland (Greenlink)": "INTGRNL", "IFA2 (INTIFA2)": "INTIFA2", "Northern Ireland(Moyle)": "INTIRL",
            "Netherlands(BritNed)": "INTNED", "Belgium (Nemolink)": "INTNEM", "North Sea Link (INTNSL)": "INTNSL",
            "Denmark (Viking link)": "INTVKL"}
LOW_CARBON = {"WIND", "NUCLEAR", "BIOMASS", "NPSHYD", "PS"}


def fetch_grid_slow():
    """Fuel mix (now), interconnector flows (latest half hour, signed: + import), national
    demand and carbon intensity."""
    mix = {r["fuelType"]: r.get("currentUsage") for r in get(f"{ELEXON}/generation/outturn/current?format=json")}
    now = datetime.now(timezone.utc)
    frm = (now.replace(microsecond=0) - timedelta(hours=3)).strftime("%Y-%m-%dT%H:%MZ")
    flows = {}
    for r in get(f"{ELEXON}/generation/outturn/interconnectors?from={frm}&format=json").get("data", []):
        code = IC_NAMES.get(r.get("interconnectorName"))
        if code and r.get("startTime", "") >= flows.get(code, ("",))[0]:
            flows[code] = (r["startTime"], r.get("generation"))
    demand = None
    try:
        rows = get(f"{ELEXON}/demand/outturn/summary?format=json")
        rows = rows if isinstance(rows, list) else rows.get("data", [])
        if rows:
            demand = max(rows, key=lambda r: r.get("startTime", "")).get("demand")
    except Exception:
        pass
    carbon = {}
    try:
        c = (get(f"{CARBON}/intensity").get("data") or [{}])[0].get("intensity") or {}
        carbon = {"actual": c.get("actual"), "forecast": c.get("forecast"), "index": c.get("index")}
    except Exception:
        pass
    return {"mix": mix, "flows": {k: v for k, v in flows.items()}, "demand": demand, "carbon": carbon}


def src_gb_grid():
    """GB grid status: system frequency every minute (Elexon, 15 s readings), and every 5
    min the fuel mix, interconnector flows, demand and carbon intensity. One grid entity
    plus one per interconnector (drawn as a line with its flow)."""
    cache = load("gb_grid.json", {"slow_at": 0, "slow": None, "freq": None})
    fetched = []
    now = datetime.now(timezone.utc)
    frm = (now - timedelta(minutes=5)).strftime("%Y-%m-%dT%H:%M:%SZ")
    d = get(f"{ELEXON}/system/frequency?from={frm}&to={now.strftime('%Y-%m-%dT%H:%M:%SZ')}&format=json").get("data", [])
    if d:
        last = max(d, key=lambda r: r["measurementTime"])
        vals = [r["frequency"] for r in d]
        cache["freq"] = {"hz": last["frequency"], "at": iso(last["measurementTime"]), "min": min(vals), "max": max(vals)}
        fetched.append("frequency")
    if time.time() - cache["slow_at"] >= 300 or cache["slow"] is None:
        try:
            cache["slow"], cache["slow_at"] = fetch_grid_slow(), time.time()
            fetched.append("mix")
        except Exception as e:
            log_line("gb-grid", f"mix/flows failed ({type(e).__name__}: {e}); keeping the last")
    save("gb_grid.json", cache)
    out, t = [], slot(60)
    f, s = cache.get("freq") or {}, cache.get("slow") or {}
    mix = {k: v for k, v in (s.get("mix") or {}).items() if not k.startswith("INT") and v}
    total = sum(mix.values()) or None
    net_import = sum((v[1] or 0) for v in (s.get("flows") or {}).values()) if s.get("flows") else None
    out.append(obs("GB-GRID", "utility-power-grid", t, {
        "lat": 54.3, "lon": -2.3, "name": "GB electricity grid", "network": "power",
        "frequency_hz": f.get("hz"), "frequency_at": f.get("at"), "frequency_min_5m": f.get("min"), "frequency_max_5m": f.get("max"),
        "generation_mw": total, "demand_mw": s.get("demand"), "net_import_mw": net_import,
        "mix": ",".join(f"{k}:{v}" for k, v in sorted(mix.items(), key=lambda kv: -kv[1])) or None,
        "wind_pct": round(100 * mix.get("WIND", 0) / total, 1) if total else None,
        "low_carbon_pct": round(100 * sum(v for k, v in mix.items() if k in LOW_CARBON) / total, 1) if total else None,
        "carbon_g_kwh": (s.get("carbon") or {}).get("actual") or (s.get("carbon") or {}).get("forecast"),
        "carbon_index": (s.get("carbon") or {}).get("index"), "source": "elexon"}))
    for code, (name, country, cap, gb, far, mid) in INTERCONNECTORS.items():
        at, mw = (s.get("flows") or {}).get(code, (None, None))
        direction = None if mw is None else "import" if mw > 20 else "export" if mw < -20 else "idle"
        out.append(obs(f"IC-{code}", "utility-power-interconnector", slot(300), {
            "lat": gb[0], "lon": gb[1], "name": f"{name} interconnector ({country})", "network": "power",
            "code": code, "country": country, "capacity_mw": cap, "flow_mw": mw, "direction": direction,
            "flow_pct": round(100 * abs(mw) / cap) if mw is not None else None, "flow_at": iso(at),
            "line": ";".join(f"{p[0]},{p[1]}" for p in (gb, mid, far)), "route": "approximate", "source": "elexon"}))
    return ",".join(fetched) or "nothing", out


# ---- the fixed grid, from OpenStreetMap (weekly snapshot, refreshed in the background)

OSM_POWER_QUERIES = {
    "lines": '[out:json][timeout:240];area["ISO3166-1"="GB"][admin_level=2]->.gb;'
             'way["power"="line"]["voltage"~"(^|;)(400000|275000|132000)(;|$)"](area.gb);out tags geom qt;',
    "substations": '[out:json][timeout:240];area["ISO3166-1"="GB"][admin_level=2]->.gb;'
                   'nwr["power"="substation"]["voltage"~"(^|;)(400000|275000|132000)(;|$)"](area.gb);out tags center qt;',
    "plants": '[out:json][timeout:240];area["ISO3166-1"="GB"][admin_level=2]->.gb;'
              'nwr["power"="plant"](area.gb);out tags center qt;',
}


def overpass(query):
    """An Overpass query, retried (the public servers are often busy: 429, 502, 504) and
    then tried on a mirror. Runs in a background thread, so the waits stall nothing."""
    data = urllib.parse.urlencode({"data": query}).encode()
    last = None
    for attempt, server in enumerate([OVERPASS[0]] * 3 + [OVERPASS[1]] * 2):
        try:
            req = urllib.request.Request(server, data=data, headers={**UA, "Accept-Encoding": "gzip"})
            with urllib.request.urlopen(req, timeout=300) as r:
                raw = r.read()
                if r.headers.get("Content-Encoding") == "gzip":
                    raw = gzip.decompress(raw)
            return json.loads(raw).get("elements", [])
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError, ValueError) as e:
            last = e
            time.sleep(min(30 * (attempt + 1), 90))
    raise RuntimeError(f"Overpass failed: {last}")


def simplify_line(pts, tol):
    """Douglas-Peucker on [lat, lon] points (tolerance in degrees)."""
    if len(pts) < 3:
        return pts
    keep = [False] * len(pts)
    keep[0] = keep[-1] = True
    stack = [(0, len(pts) - 1)]
    while stack:
        a, b = stack.pop()
        (y1, x1), (y2, x2) = pts[a], pts[b]
        dx, dy = x2 - x1, y2 - y1
        n = math.hypot(dx, dy) or 1e-12
        best, idx = 0.0, None
        for i in range(a + 1, b):
            y, x = pts[i]
            d = abs(dy * x - dx * y + x2 * y1 - y2 * x1) / n
            if d > best:
                best, idx = d, i
        if idx is not None and best > tol:
            keep[idx] = True
            stack += [(a, idx), (idx, b)]
    return [p for p, k in zip(pts, keep) if k]


def max_kv(v):
    kv = [as_int(x) for x in str(v or "").split(";")]
    kv = [k for k in kv if k]
    return max(kv) // 1000 if kv else None


def mw(v):
    """'1,200 MW' / '50 kW' / '1.2 GW' -> MW."""
    m = re.match(r"\s*([\d.,]+)\s*([kKmMgG])?[wW]", str(v or ""))
    if not m:
        return None
    x = float(m.group(1).replace(",", ""))
    return round(x * {"k": 0.001, "m": 1, "g": 1000}.get((m.group(2) or "m").lower(), 1), 1)


def fetch_power_infra():
    items = {}
    tiles = {}
    for w in overpass(OSM_POWER_QUERIES["lines"]):
        kv = max_kv((w.get("tags") or {}).get("voltage"))
        g = w.get("geometry") or []
        if kv not in (400, 275, 132) or len(g) < 2:
            continue
        pts = simplify_line([(round(p["lat"], 4), round(p["lon"], 4)) for p in g], 0.0015)
        c = pts[len(pts) // 2]
        tiles.setdefault((kv, int(math.floor(c[0])), int(math.floor(c[1]))), []).append(pts)
    for (kv, la, lo), parts in tiles.items():
        flat = [p for part in parts for p in part]
        items[f"OSM-PWRLINE-{kv}-{la}-{lo}".replace("--", "-M")] = {
            "kind": "infra-power-line", "lat": round(sum(p[0] for p in flat) / len(flat), 4),
            "lon": round(sum(p[1] for p in flat) / len(flat), 4), "network": "power", "voltage_kv": kv,
            "name": f"{kv} kV lines", "lines_n": len(parts),
            "line": "|".join(";".join(f"{a},{b}" for a, b in part) for part in parts), "source": "osm"}
    for e in overpass(OSM_POWER_QUERIES["substations"]):
        t, c = e.get("tags") or {}, e.get("center") or e
        if c.get("lat") is None:
            continue
        items[f"OSM-SUB-{e['type'][0]}{e['id']}"] = {
            "kind": "infra-power-substation", "lat": round(c["lat"], 5), "lon": round(c["lon"], 5), "network": "power",
            "name": clean(t.get("name"), 80) or f"{max_kv(t.get('voltage'))} kV substation", "operator": clean(t.get("operator"), 80),
            "voltage_kv": max_kv(t.get("voltage")), "ref": clean(t.get("ref"), 40), "source": "osm"}
    for e in overpass(OSM_POWER_QUERIES["plants"]):
        t, c = e.get("tags") or {}, e.get("center") or e
        out_mw = mw(t.get("plant:output:electricity"))
        if c.get("lat") is None or not (t.get("name") or (out_mw or 0) >= 1):
            continue
        items[f"OSM-PLANT-{e['type'][0]}{e['id']}"] = {
            "kind": "infra-power-plant", "lat": round(c["lat"], 5), "lon": round(c["lon"], 5), "network": "power",
            "name": clean(t.get("name"), 80) or "Power station", "operator": clean(t.get("operator"), 80),
            "fuel": clean(t.get("plant:source"), 40), "method": clean(t.get("plant:method"), 40), "output_mw": out_mw,
            "source": "osm"}
    return items


INFRA = {"thread": None}
INFRA_EVERY_S = 7 * 86400


def infra_refresh(name, fetch):
    try:
        save(name, {"day": day_start(), "items": fetch()})
    except Exception as e:
        log_line(name.split(".")[0], f"background refetch failed ({type(e).__name__}: {e}); keeping the old cache")
    finally:
        INFRA[name] = None


def weekly_infra(name, fetch):
    """A large static dataset refetched weekly in a background thread (never stalling the
    other sources); observedAt is the snapshot's day, so a new snapshot is sent once."""
    import threading
    cache = load(name, None)
    fetched = "cached"
    if cache is None or time.time() - cache.get("day", 0) >= INFRA_EVERY_S:
        if INFRA.get(name) is None:
            INFRA[name] = threading.Thread(target=infra_refresh, args=(name, fetch), name=name, daemon=True)
            INFRA[name].start()
        fetched = "refreshing in background"
        if "--once" in sys.argv:
            INFRA[name].join()
            cache, fetched = load(name, cache), "refetched"
    if cache is None:
        return fetched, []
    return f"{fetched} {len(cache['items'])}", [
        obs(k, a["kind"], cache["day"], {x: y for x, y in a.items() if x != "kind"}) for k, a in cache["items"].items()]


def src_power_infra():
    return weekly_infra("infra_power.json", fetch_power_infra)


# ---------------------------------------------------------------- 31. Utilities: water (GB)
#
# Live storm overflow (Event Duration Monitoring) status from the nine English water
# companies' Stream / National Storm Overflow Hub layers (Water UK schema, CC BY 4.0:
# "Contains storm overflow data from <company>, licensed under CC BY 4.0, via Stream") and
# Scottish Water's via the Rivers Trust mirror (CC BY). Only overflows discharging now or
# within the last 48 h are sent (about 20,000 sites otherwise). Welsh Water's layer states
# no licence and is not used. Reservoir levels from 32 Environment Agency stations (OGL
# v3.0). Treatment works from OpenStreetMap (ODbL), weekly.

EDM_LAYERS = {
    "Thames Water": "https://services2.arcgis.com/g6o32ZDQ33GpCIu3/arcgis/rest/services/Thames_Water_Storm_Overflow_Activity_%28Production%29_view/FeatureServer/0/query",
    "Southern Water": "https://services-eu1.arcgis.com/6qJmARkS2dt2IjVA/arcgis/rest/services/SouthernWater_StormOverflowActivity_PROD_view/FeatureServer/0/query",
    "Anglian Water": "https://services3.arcgis.com/VCOY1atHWVcDlvlJ/arcgis/rest/services/stream_service_outfall_locations_view/FeatureServer/0/query",
    "Severn Trent Water": "https://services1.arcgis.com/NO7lTIlnxRMMG9Gw/arcgis/rest/services/Severn_Trent_Water_Storm_Overflow_Activity/FeatureServer/0/query",
    "United Utilities": "https://services5.arcgis.com/5eoLvR0f8HKb7HWP/arcgis/rest/services/United_Utilities_Storm_Overflow_Activity/FeatureServer/0/query",
    "Yorkshire Water": "https://services-eu1.arcgis.com/1WqkK5cDKUbF0CkH/arcgis/rest/services/Yorkshire_Water_Storm_Overflow_Activity/FeatureServer/0/query",
    "Wessex Water": "https://services.arcgis.com/3SZ6e0uCvPROr4mS/arcgis/rest/services/Wessex_Water_Storm_Overflow_Activity/FeatureServer/0/query",
    "South West Water": "https://services-eu1.arcgis.com/OMdMOtfhATJPcHe3/arcgis/rest/services/NEH_outlets_PROD/FeatureServer/0/query",
    "Northumbrian Water": "https://services-eu1.arcgis.com/MSNNjkZ51iVh8yBj/arcgis/rest/services/Northumbrian_Water_Storm_Overflow_Activity_2_view/FeatureServer/0/query",
}
SCOT_EDM = "https://services3.arcgis.com/Bb8lfThdhugyc4G3/arcgis/rest/services/Scottish_Water_Storm_Overflow_Activity/FeatureServer/0/query"
RECENT_OVERFLOW_S = 48 * 3600


def ci(props, name):
    """A property by name, any case (South West Water uses camelCase)."""
    low = name.lower()
    for k, v in props.items():
        if k.lower() == low:
            return v
    return None


def arcgis_pages(url, where, page=1000):
    out, offset = [], 0
    while True:
        q = urllib.parse.urlencode({"where": where, "outFields": "*", "outSR": 4326, "f": "geojson",
                                    "resultOffset": offset, "resultRecordCount": page})
        d = get(f"{url}?{q}", timeout=60)
        if not isinstance(d, dict) or "features" not in d:
            raise ValueError(f"unexpected ArcGIS response: {str(d)[:160]}")
        out += d["features"]
        if len(d["features"]) < page or offset > 20000:
            return out
        offset += page


def fetch_overflows():
    items, failed = {}, []
    now = time.time()
    since = datetime.fromtimestamp(now - RECENT_OVERFLOW_S, timezone.utc).strftime("%Y-%m-%d %H:%M:%S")
    where = f"Status = 1 OR LatestEventEnd >= TIMESTAMP '{since}'"
    for company, url in EDM_LAYERS.items():
        try:
            for f in arcgis_pages(url, where):
                p, g = f.get("properties") or {}, (f.get("geometry") or {}).get("coordinates") or []
                sid, status = ci(p, "Id"), ci(p, "Status")
                if not sid or len(g) < 2 or status not in (0, 1):
                    continue
                ms = lambda k: int(ci(p, k) / 1000) if isinstance(ci(p, k), (int, float)) else None
                end = ms("LatestEventEnd")
                if status == 0 and not (end and now - end <= RECENT_OVERFLOW_S):
                    continue
                items[f"WTR-{slug(sid)}"] = {
                    "lat": round(float(g[1]), 5), "lon": round(float(g[0]), 5), "network": "water", "operator": company,
                    "ref": sid, "status": "discharging" if status == 1 else "recent",
                    "name": f"Storm overflow {'discharging' if status == 1 else 'stopped'} · {clean(ci(p, 'ReceivingWaterCourse'), 60) or sid}",
                    "since": ms("StatusStart"), "last_start": ms("LatestEventStart"), "last_end": end,
                    "watercourse": clean(ci(p, "ReceivingWaterCourse"), 80), "updated": ms("LastUpdated"), "source": "stream-edm"}
        except Exception as e:
            failed.append(f"{company} {type(e).__name__}")
    try:
        for f in arcgis_pages(SCOT_EDM, "STATUS_NSOH = 1 OR STATUS_NSOH = 0"):
            p, g = f.get("properties") or {}, (f.get("geometry") or {}).get("coordinates") or []
            st = ci(p, "STATUS_NSOH")
            desc = str(ci(p, "OVERFLOW_STATUS_DESCRIPTION") or ci(p, "STATUS") or "")
            recent = st == 1 or "recent" in desc.lower()
            sid = ci(p, "ASSET_ID") or ci(p, "Id")
            if not recent or not sid or len(g) < 2:
                continue
            items[f"WTR-SW-{slug(sid)}"] = {
                "lat": round(float(g[1]), 5), "lon": round(float(g[0]), 5), "network": "water", "operator": "Scottish Water",
                "ref": str(sid), "status": "discharging" if st == 1 else "recent",
                "name": f"Storm overflow {'discharging' if st == 1 else 'stopped'} · {clean(ci(p, 'RECEIVING_WATER'), 60) or clean(ci(p, 'ASSET_NAME'), 60)}",
                "site": clean(ci(p, "ASSET_NAME"), 80), "watercourse": clean(ci(p, "RECEIVING_WATER"), 80),
                "source": "scottish-water-edm"}
    except Exception as e:
        failed.append(f"Scottish Water {type(e).__name__}")
    if failed:
        log_line("storm-overflows", f"failed: {', '.join(failed)}")
        if len(failed) == len(EDM_LAYERS) + 1:
            raise RuntimeError("every storm overflow source failed")
    return items


def src_overflows():
    """Storm overflows discharging now or in the last 48 h, refetched every 10 min and
    resent each 10-min slot (Heimdall TTL 1800 s)."""
    cache = load("overflows.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    if time.time() - cache["fetched"] >= 600:
        cache = {"fetched": time.time(), "items": fetch_overflows()}
        save("overflows.json", cache)
        fetched = len(cache["items"])
    t = slot(600)
    return fetched, [obs(k, "utility-water-overflow", t, a) for k, a in cache["items"].items()]


def src_reservoirs():
    """Reservoir levels (m) at the Environment Agency's reservoir stations, every 15 min."""
    cache = load("reservoirs.json", {"fetched": 0, "stations": None, "items": {}})
    fetched = "cached"
    if cache["stations"] is None or time.time() - cache.get("stations_at", 0) >= 86400:
        st = get(f"{EA}/id/stations?search=Reservoir&_limit=500").get("items", [])
        cache["stations"] = [{"id": s.get("notation") or s.get("stationReference"), "label": clean(s.get("label"), 100),
                              "lat": s.get("lat"), "lon": s.get("long"), "river": clean(s.get("riverName"), 80)}
                             for s in st if s.get("lat") is not None]
        cache["stations_at"] = time.time()
    if time.time() - cache["fetched"] >= 900:
        from concurrent.futures import ThreadPoolExecutor

        def latest(s):
            try:
                return s, get(f"{EA}/id/stations/{s['id']}/readings?latest").get("items") or []
            except Exception:
                return s, []

        items = {}
        with ThreadPoolExecutor(max_workers=8) as pool:  # the EA API takes ~3 s a request
            readings = list(pool.map(latest, cache["stations"]))
        for s, r in readings:
            r = [x for x in r if isinstance(x.get("value"), (int, float))]
            if not r:
                continue
            m = re.search(r"spillway level\s*=\s*([\d.]+)", s["label"] or "", re.I)
            items[f"RES-{slug(s['id'])}"] = {
                "lat": round(float(s["lat"]), 5), "lon": round(float(s["lon"]), 5), "network": "water",
                "name": re.sub(r"\s*\(.*\)$", "", s["label"] or "Reservoir"), "level_m": r[0]["value"],
                "spillway_m": float(m.group(1)) if m else None, "reading_at": iso(r[0].get("dateTime")),
                "operator": "Environment Agency", "source": "environment-agency"}
        cache["items"], cache["fetched"] = items, time.time()
        fetched = len(items)
    save("reservoirs.json", cache)
    return fetched, [obs(k, "utility-water-reservoir", slot(900), a) for k, a in cache["items"].items()]


OSM_WATER_QUERY = ('[out:json][timeout:240];area["ISO3166-1"="GB"][admin_level=2]->.gb;'
                   '(nwr["man_made"="water_works"](area.gb);nwr["man_made"="wastewater_plant"]["name"](area.gb););'
                   'out tags center qt;')


def fetch_water_infra():
    items = {}
    for e in overpass(OSM_WATER_QUERY):
        t, c = e.get("tags") or {}, e.get("center") or e
        if c.get("lat") is None:
            continue
        works = t.get("man_made") == "water_works"
        items[f"OSM-WTR-{e['type'][0]}{e['id']}"] = {
            "kind": "infra-water-works" if works else "infra-water-treatment", "lat": round(c["lat"], 5),
            "lon": round(c["lon"], 5), "network": "water",
            "name": clean(t.get("name"), 80) or ("Water treatment works" if works else "Wastewater treatment works"),
            "operator": clean(t.get("operator"), 80), "source": "osm"}
    return items


def src_water_infra():
    return weekly_infra("infra_water.json", fetch_water_infra)


# ---------------------------------------------------------------- 32. Utilities: gas (GB)
#
# GB National Transmission System flows from National Gas's operational data API (keyless;
# published every 12 min, 2-minute values in mcm/d; no open licence is stated, so this is
# used under the National Gas website terms, "Source: National Gas Transmission Data
# Portal"). Terminal, LNG, storage and interconnector positions are approximate (about 1 km).

NG_API = "https://api.nationalgas.com/operationaldata/v1"
# terminal (supply-from-terminals siteName) -> (id, name, lat, lon, kind, entry-point prefix)
GAS_SITES = {
    "ST FERGUS TERMINAL": ("STFERGUS", "St Fergus gas terminal", 57.578, -1.833, "terminal", "ST FERGUS"),
    "BACTON UKCS Terminal": ("BACTON", "Bacton gas terminal", 52.858, 1.465, "terminal", "BACTON"),
    "EASINGTON TERMINAL": ("EASINGTON", "Easington gas terminal", 53.656, 0.115, "terminal", "EASINGTON"),
    "TEESSIDE TERMINAL": ("TEESSIDE", "Teesside gas terminal", 54.620, -1.180, "terminal", "TEESSIDE"),
    "THEDDLETHORPE TERMINAL": ("THEDDLETHORPE", "Theddlethorpe gas terminal", 53.370, 0.230, "terminal", "THEDDLETHORPE"),
    "BARROW TERMINAL": ("BARROW", "Barrow gas terminal", 54.087, -3.157, "terminal", "BARROW"),
    "ISLE OF GRAIN TERMINAL": ("GRAIN", "Isle of Grain LNG terminal", 51.445, 0.705, "lng", "GRAIN"),
    "MILFORD HAVEN TERMINAL": ("MILFORD", "Milford Haven LNG terminals", 51.701, -5.062, "lng", "MILFORD HAVEN"),
}
# storage entry points -> (id, name, lat, lon)
GAS_STORAGE = {
    "ALDBROUGH": ("ALDBROUGH", "Aldbrough gas storage", 53.830, -0.110),
    "HORNSEA": ("HORNSEA", "Hornsea (Atwick) gas storage", 53.930, -0.180),
    "EASINGTON ROUGH ST": ("ROUGH", "Rough gas storage (offshore)", 53.830, 0.450),
    "HOLFORD": ("HOLFORD", "Holford gas storage", 53.240, -2.420),
    "STUBLACH": ("STUBLACH", "Stublach gas storage", 53.215, -2.480),
    "HOLE HOUSE FARM": ("HOLEHOUSE", "Hole House Farm gas storage", 53.180, -2.440),
    "HILLTOP": ("HILLTOP", "Hill Top Farm gas storage", 53.200, -2.460),
}
# pipelines with a live flow: siteName -> (id, name, country, direction, route)
GAS_LINKS = {
    "Bacton INT Export": ("IUK", "Interconnector UK (IUK)", "Belgium", "export", ((52.858, 1.465), (52.10, 2.40), (51.330, 3.200))),
    "Bacton BBL Export": ("BBL", "BBL pipeline", "Netherlands", "export", ((52.858, 1.465), (52.90, 3.20), (52.930, 4.780))),
    "Moffat Export": ("MOFFAT", "Moffat interconnector", "Ireland", "export", ((55.310, -3.430), (54.800, -4.150), (54.20, -5.40), (53.640, -6.220))),
    "EASINGTON LANGELED": ("LANGELED", "Langeled pipeline", "Norway", "import", ((53.656, 0.115), (55.80, 1.50), (58.370, 1.900), (62.850, 6.950))),
}


def fetch_gas():
    d = get(f"{NG_API}/instantaneousflow/sites", timeout=40)
    flows = {}
    for g in d.get("instantaneousFlow") or []:
        for s in g.get("sites") or []:
            det = [x for x in (s.get("siteGasDetail") or []) if isinstance(x.get("flowRate"), (int, float))]
            if det:
                last = max(det, key=lambda x: x.get("applicableAt") or "")
                flows[s["siteName"]] = (round(last["flowRate"], 2), iso(last.get("applicableAt")), last.get("qualityIndicator") == "E")
    return {"published": d.get("publishedTime"), "gas_day": d.get("currentGasDay"), "flows": flows}


def src_gas():
    """GB gas: terminal, LNG, storage and interconnector flows and the system's supply,
    demand and linepack. The cheap published-time check runs every minute; the flows are
    refetched when National Gas publishes (every 12 min). Resent per 10-min slot (TTL 1800)."""
    cache = load("gas.json", {"published": None, "flows": {}})
    fetched = "unchanged"
    try:
        pub = get(f"{NG_API}/instantaneousflow/latestpublishedtime", timeout=20).get("latestPublishedTime")
    except Exception:
        pub = None
    if pub is None or pub != cache.get("published"):
        cache = fetch_gas()
        save("gas.json", cache)
        fetched = len(cache["flows"])
    f, t, out = cache.get("flows") or {}, slot(600), []
    val = lambda name: (f.get(name) or (None,))[0]
    for site, (sid, name, lat, lon, typ, prefix) in GAS_SITES.items():
        flow, at, est = f.get(site, (None, None, False))
        entries = [f"{k.replace(prefix, '').strip(' -') or k}:{v[0]}" for k, v in f.items()
                   if k.startswith(prefix) and k not in GAS_STORAGE and not k.endswith(("Terminal", "TERMINAL")) and v[0]]
        out.append(obs(f"GAS-{sid}", "utility-gas-terminal", t, {
            "lat": lat, "lon": lon, "name": name, "network": "gas", "site_type": typ, "flow_mcmd": flow,
            "flowing": bool(flow and flow > 0.05), "entries": ",".join(entries) or None, "estimated": est or None,
            "flow_at": at, "operator": "National Gas", "source": "national-gas"}))
    for site, (sid, name, lat, lon) in GAS_STORAGE.items():
        flow, at, est = f.get(site, (None, None, False))
        out.append(obs(f"GAS-{sid}", "utility-gas-storage", t, {
            "lat": lat, "lon": lon, "name": name, "network": "gas", "withdrawal_mcmd": flow,
            "flowing": bool(flow and flow > 0.05), "flow_at": at, "estimated": est or None, "source": "national-gas"}))
    for site, (sid, name, country, direction, route) in GAS_LINKS.items():
        flow, at, est = f.get(site, (None, None, False))
        out.append(obs(f"GAS-{sid}", "utility-gas-interconnector", t, {
            "lat": route[0][0], "lon": route[0][1], "name": f"{name} ({country})", "network": "gas", "country": country,
            "direction": direction if flow and flow > 0.05 else "idle", "flow_mcmd": flow, "flow_at": at,
            "line": ";".join(f"{a},{b}" for a, b in route), "route": "approximate", "source": "national-gas"}))
    out.append(obs("GB-GAS", "utility-gas-system", t, {
        "lat": 53.2, "lon": -1.6, "name": "GB gas transmission system", "network": "gas",
        "supply_mcmd": val("TOTAL SUPPLY"), "demand_mcmd": val("NTS Demand Flow"), "linepack_mcm": val("NTS Actual Linepack"),
        "ldz_mcmd": val("LDZ Offtake Flow"), "power_mcmd": val("Power Station Demand Flow"),
        "industrial_mcmd": val("Industrial Demand Flow"), "export_mcmd": val("Interconnector Export Demand Flow"),
        "storage_injection_mcmd": val("Storage Demand Flow"), "gas_day": cache.get("gas_day"),
        "published": iso(cache.get("published")), "operator": "National Gas", "source": "national-gas"}))
    return fetched, out


OSM_GAS_QUERY = ('[out:json][timeout:240];area["ISO3166-1"="GB"][admin_level=2]->.gb;'
                 'way["man_made"="pipeline"]["substance"="gas"]["operator"~"National Gas|National Grid",i](area.gb);'
                 'out tags geom qt;')


def fetch_gas_infra():
    """National Gas transmission pipelines from OpenStreetMap, merged per 1-degree tile."""
    tiles = {}
    for w in overpass(OSM_GAS_QUERY):
        g = w.get("geometry") or []
        if len(g) < 2:
            continue
        pts = simplify_line([(round(p["lat"], 4), round(p["lon"], 4)) for p in g], 0.0015)
        c = pts[len(pts) // 2]
        tiles.setdefault((int(math.floor(c[0])), int(math.floor(c[1]))), []).append(pts)
    items = {}
    for (la, lo), parts in tiles.items():
        flat = [p for part in parts for p in part]
        items[f"OSM-GASLINE-{la}-{lo}".replace("--", "-M")] = {
            "kind": "infra-gas-pipeline", "lat": round(sum(p[0] for p in flat) / len(flat), 4),
            "lon": round(sum(p[1] for p in flat) / len(flat), 4), "network": "gas", "name": "Gas transmission pipelines",
            "lines_n": len(parts), "line": "|".join(";".join(f"{a},{b}" for a, b in part) for part in parts), "source": "osm"}
    return items


def src_gas_infra():
    return weekly_infra("infra_gas.json", fetch_gas_infra)


# ---------------------------------------------------------------- 33. Utilities: telecoms
#
# Internet outages per country from IODA (Internet Intelligence Research Lab, Georgia Tech;
# keyless, no published licence: credit "IODA, Georgia Tech", polled every 10 min). A
# country is sent only while two or more of IODA's signals (BGP, active probing, telescope,
# Google traffic) score an outage in the last 6 h; one signal alone is often a measurement
# glitch. Countries are placed at Natural Earth label points (public domain). Telephone
# exchanges, data centres and submarine cables around the UK from OpenStreetMap (ODbL),
# weekly. TeleGeography's cable data (paid licence) and PeeringDB (operational use only, no
# redistribution) are not used.

IODA = "https://api.ioda.inetintel.cc.gatech.edu/v2"
NE_COUNTRIES = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/v5.1.2/geojson/ne_50m_admin_0_countries.geojson"


def country_points():
    def fetch():
        d = get(NE_COUNTRIES, timeout=60)
        out = {}
        for f in d.get("features") or []:
            p = f.get("properties") or {}
            code = p.get("ISO_A2_EH") if p.get("ISO_A2_EH") not in (None, "-99") else p.get("ISO_A2")
            if code and code != "-99" and p.get("LABEL_Y") is not None:
                out[code] = {"lat": round(p["LABEL_Y"], 3), "lon": round(p["LABEL_X"], 3), "name": p.get("NAME")}
        return out
    cache = load("country_points.json", None)
    if not cache:
        cache = fetch()
        save("country_points.json", cache)
    return cache


def src_internet_outages():
    cache = load("ioda.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    if time.time() - cache["fetched"] >= 600:
        until = int(time.time())
        d = get(f"{IODA}/outages/summary?entityType=country&from={until - 6 * 3600}&until={until}&limit=300", timeout=60)
        pts, items = country_points(), {}
        for row in d.get("data") or []:
            ent, scores = row.get("entity") or {}, row.get("scores") or {}
            signals = sorted({k.split(".")[0] for k in scores if k != "overall"})
            code, p = ent.get("code"), pts.get(ent.get("code") or "")
            if len(signals) < 2 or not p:
                continue
            items[f"NET-{code}"] = {
                "lat": p["lat"], "lon": p["lon"], "network": "telecom", "country": ent.get("name") or p["name"],
                "name": f"Internet disruption · {ent.get('name') or p['name']}", "signals": ",".join(signals),
                "signals_n": len(signals), "score": round(scores.get("overall") or 0), "events": row.get("event_cnt"),
                "window_h": 6, "source": "ioda"}
        cache = {"fetched": time.time(), "items": items}
        save("ioda.json", cache)
        fetched = len(items)
    return fetched, [obs(k, "utility-telecom-outage", slot(600), a) for k, a in cache["items"].items()]


OSM_TELECOM_QUERIES = {
    "sites": '[out:json][timeout:240];area["ISO3166-1"="GB"][admin_level=2]->.gb;'
             '(nwr["telecom"="exchange"](area.gb);nwr["telecom"="data_center"](area.gb);nwr["building"="data_center"](area.gb););'
             'out tags center qt;',
    # around the UK and Ireland, out to the near continent
    "cables": '[out:json][timeout:240];way["seamark:type"="cable_submarine"](45,-16,64,12);out tags geom qt;',
}


def fetch_telecom_infra():
    items = {}
    for e in overpass(OSM_TELECOM_QUERIES["sites"]):
        t, c = e.get("tags") or {}, e.get("center") or e
        if c.get("lat") is None:
            continue
        dc = t.get("telecom") == "data_center" or t.get("building") == "data_center"
        items[f"OSM-TEL-{e['type'][0]}{e['id']}"] = {
            "kind": "infra-telecom-datacentre" if dc else "infra-telecom-exchange", "lat": round(c["lat"], 5),
            "lon": round(c["lon"], 5), "network": "telecom",
            "name": clean(t.get("name"), 80) or ("Data centre" if dc else "Telephone exchange"),
            "operator": clean(t.get("operator"), 80), "source": "osm"}
    tiles = {}
    for w in overpass(OSM_TELECOM_QUERIES["cables"]):
        g = w.get("geometry") or []
        if len(g) < 2:
            continue
        pts = simplify_line([(round(p["lat"], 4), round(p["lon"], 4)) for p in g], 0.003)
        c = pts[len(pts) // 2]
        tiles.setdefault((int(math.floor(c[0] / 2)) * 2, int(math.floor(c[1] / 2)) * 2), []).append(pts)
    for (la, lo), parts in tiles.items():
        flat = [p for part in parts for p in part]
        items[f"OSM-CABLE-{la}-{lo}".replace("--", "-M")] = {
            "kind": "infra-telecom-cable", "lat": round(sum(p[0] for p in flat) / len(flat), 4),
            "lon": round(sum(p[1] for p in flat) / len(flat), 4), "network": "telecom", "name": "Submarine cables",
            "lines_n": len(parts), "line": "|".join(";".join(f"{a},{b}" for a, b in part) for part in parts), "source": "osm"}
    return items


def src_telecom_infra():
    return weekly_infra("infra_telecom.json", fetch_telecom_infra)


# ---- Cloudflare Radar internet outages (free API token, local only, never committed;
# data CC BY-NC 4.0, "Cloudflare Radar": non-commercial use). Outages that are ongoing or
# ended in the last 24 h, one entity per affected country (from the outage's locations
# or its networks' countries), with Cloudflare's cause and type.

CF_TOKEN = os.path.join(HERE, "secrets", "cloudflare-radar.token")
CF_RADAR = "https://api.cloudflare.com/client/v4/radar"


def src_cf_outages():
    cache = load("cf_outages.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    if time.time() - cache["fetched"] >= 900:
        token = open(CF_TOKEN).read().strip()
        req = urllib.request.Request(f"{CF_RADAR}/annotations/outages?dateRange=7d&limit=200&format=json",
                                     headers={**UA, "Authorization": f"Bearer {token}"})
        with urllib.request.urlopen(req, timeout=TIMEOUT) as r:
            d = json.load(r)
        if not d.get("success"):
            raise RuntimeError(f"Cloudflare Radar: {d.get('errors')}")
        pts, items, now = country_points(), {}, time.time()
        for a in (d.get("result") or {}).get("annotations") or []:
            end = iso(a.get("endDate"))
            if end and now - end > 86400:
                continue
            countries = {x.get("code"): x.get("name") for x in a.get("locationsDetails") or [] if x.get("code")}
            for asn in a.get("asnsDetails") or []:
                loc = asn.get("location") or {}
                if loc.get("code"):
                    countries.setdefault(loc["code"], loc.get("name"))
            o = a.get("outage") or {}
            nets = ", ".join(f"AS{x.get('asn')} {x.get('name')}" for x in (a.get("asnsDetails") or [])[:4]) or None
            for code, cname in countries.items():
                p = pts.get(code)
                if not p:
                    continue
                cause = (o.get("outageCause") or "").replace("_", " ").lower() or None
                items[f"CFR-{a['id']}-{code}"] = {
                    "lat": p["lat"], "lon": p["lon"], "network": "telecom", "country": cname or p["name"],
                    "name": f"Internet outage · {cname or p['name']}{f' ({cause})' if cause else ''}",
                    "cause": o.get("outageCause"), "outage_type": (o.get("outageType") or "").lower() or None,
                    "description": clean(a.get("description"), 240), "networks": nets, "started": iso(a.get("startDate")),
                    "ended": end, "ongoing": end is None, "link": a.get("linkedUrl") if a.get("linkedUrl") not in (None, "None") else None,
                    "source": "cloudflare-radar"}
        cache = {"fetched": time.time(), "items": items}
        save("cf_outages.json", cache)
        fetched = len(items)
    return fetched, [obs(k, "utility-telecom-outage", slot(900), a) for k, a in cache["items"].items()]


# ---- GIE AGSI+ (gas storage) and ALSI (LNG terminals), Europe (free API key, local only,
# never committed; "Source: GIE AGSI+ / ALSI" required). Published daily for the previous
# gas day (GB stopped reporting after Brexit). Country storage at Natural Earth label
# points; LNG terminals at the positions GIE gives. Refreshed daily in the background.

GIE_KEY = os.path.join(HERE, "secrets", "gie.key")


def gie(url):
    req = urllib.request.Request(url, headers={**UA, "x-key": open(GIE_KEY).read().strip()})
    with urllib.request.urlopen(req, timeout=40) as r:
        return json.load(r)


def fnum(v):
    try:
        return round(float(v), 2)
    except (TypeError, ValueError):
        return None


def fetch_gie():
    items, pts = {}, country_points()
    for c in {f["country"] for co in gie("https://agsi.gie.eu/api/about?show=listing") for f in co.get("facilities") or []}:
        try:
            rows = gie(f"https://agsi.gie.eu/api?country={c}&size=1").get("data") or []
        except Exception:
            continue
        r, p = (rows or [None])[0], pts.get(c)
        if not r or not p or fnum(r.get("full")) is None:
            continue
        items[f"GIE-STO-{c}"] = {
            "kind": "utility-gas-storage", "scope": "country", "lat": p["lat"], "lon": p["lon"], "network": "gas",
            "name": f"Gas storage · {r.get('name') or p['name']}", "country": r.get("name"), "full_pct": fnum(r.get("full")),
            "storage_twh": fnum(r.get("gasInStorage")), "working_twh": fnum(r.get("workingGasVolume")),
            "injection_gwh_d": fnum(r.get("injection")), "withdrawal_gwh_d": fnum(r.get("withdrawal")),
            "trend_pct": fnum(r.get("trend")), "gas_day": r.get("gasDayStart"), "flowing": (fnum(r.get("withdrawal")) or 0) > 1,
            "source": "gie-agsi"}
        time.sleep(0.5)
    for co in gie("https://alsi.gie.eu/api/about?show=listing"):
        for f in co.get("facilities") or []:
            if f.get("operational_end_date"):
                continue
            try:
                rows = gie(f"https://alsi.gie.eu/api?country={f['country']}&company={co['eic']}&facility={f['eic']}&size=1").get("data") or []
            except Exception:
                continue
            r = (rows or [None])[0]
            if not r or fnum(r.get("latitude")) is None:
                continue
            inv, dtmi = fnum((r.get("inventory") or {}).get("gwh")), fnum((r.get("dtmi") or {}).get("gwh"))
            send = fnum(r.get("sendOut"))
            items[f"GIE-LNG-{slug(f['eic'])}"] = {
                "kind": "utility-gas-lng", "lat": fnum(r["latitude"]), "lon": fnum(r["longitude"]), "network": "gas",
                "name": f"{r.get('name') or f['name']} (LNG)", "operator": co.get("short_name") or co.get("name"),
                "country": f.get("country"), "site_type": (r.get("type") or "").lower() or None,
                "inventory_gwh": inv, "full_pct": round(100 * inv / dtmi, 1) if inv is not None and dtmi else None,
                "sendout_gwh_d": send, "capacity_gwh_d": fnum(r.get("dtrs")), "flowing": (send or 0) > 1,
                "gas_day": r.get("gasDayStart"), "source": "gie-alsi"}
            time.sleep(0.5)
    return items


GIE_BG = {"thread": None}


def src_gie():
    """European gas storage and LNG terminals, refreshed daily in the background; the
    cached day is resent each 10-min slot (Heimdall TTL 1800 s)."""
    import threading
    cache = load("gie.json", None)
    fetched = "cached"
    if (cache is None or day_start() > cache.get("day", 0)) and GIE_BG["thread"] is None:
        def refresh():
            try:
                save("gie.json", {"day": day_start(), "items": fetch_gie()})
            except Exception as e:
                log_line("gie", f"background refetch failed ({type(e).__name__}: {e})")
            finally:
                GIE_BG["thread"] = None
        GIE_BG["thread"] = threading.Thread(target=refresh, name="gie", daemon=True)
        GIE_BG["thread"].start()
        fetched = "refreshing in background"
        if "--once" in sys.argv:
            GIE_BG["thread"].join()
            cache, fetched = load("gie.json", cache), "refetched"
    if cache is None:
        return fetched, []
    t = slot(600)
    return f"{fetched} {len(cache['items'])}", [
        obs(k, a["kind"], t, {x: y for x, y in a.items() if x != "kind"}) for k, a in cache["items"].items()]


# ---------------------------------------------------------------- EASA conflict zones

EASA_CZIB = "https://www.easa.europa.eu/en/domains/air-operations/czibs/export-json?page&_format=json"
# Bulletin areas that are not one country, and names that differ from Natural Earth's.
CZIB_AREAS = {"Persian Gulf and Gulf of Oman": (26.0, 55.0)}
CZIB_COUNTRY_ALIASES = {"Russian Federation": "Russia", "Yemen \u2013 Sana\u2019a Flight Information Region": "Yemen"}


def src_easa_czib():
    """EASA Conflict Zone Information Bulletins: the EU aviation regulator's active
    warnings to airlines about airspace over conflict zones (Iran, Iraq, the Gulf, Ukraine
    and others). Keyless JSON export; reproduction authorised with the source acknowledged
    ("Source: EASA"). Placed at the country's label point (or the sea area's centre).
    A bulletin issued or revised in the last 7 days is flagged, so Heimdall raises an alert:
    a new or tightened bulletin is an early sign regulators see risk rising. Refetched
    hourly, resent every 30 min."""
    cache = load("easa_czib.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 3600:
        d = get(EASA_CZIB, timeout=60)
        pts = {v["name"]: v for v in country_points().values()}
        items = {}
        for z in (d or {}).get("conflict_zones") or []:
            if (z.get("status") or "").lower() != "active":
                continue
            name = clean(re.sub(r"<[^>]+>", " ", z.get("name") or ""), 200)
            area = re.sub(r"^Airspace of (the )?", "", name or "")
            country = clean(z.get("country"))
            p = CZIB_AREAS.get(area)
            if not p:
                c = pts.get(CZIB_COUNTRY_ALIASES.get(area, area)) or pts.get(CZIB_COUNTRY_ALIASES.get(country or "", country or ""))
                p = (c["lat"], c["lon"]) if c else None
            if not p:
                continue
            # "updated" arrives as an HTML <time datetime="..."> element
            upd = re.search(r'datetime="([^"]+)"', z.get("updated") or "")
            issued, updated = iso(z.get("issued_date")), iso(upd.group(1) if upd else None)
            valid = z.get("valid_until_date") or ""
            m = re.match(r"(\d{2})/(\d{2})/(\d{4})$", valid.strip())
            items[f"CZIB-{z.get('Nid')}"] = {
                "lat": p[0], "lon": p[1], "name": f"EASA conflict-zone bulletin \u00b7 {area}", "area": area,
                "country": country, "issued_at": issued, "revised_at": updated,
                "valid_until": f"{m.group(3)}-{m.group(2)}-{m.group(1)}" if m else None,
                "new_7d": bool(issued and now - issued <= 7 * 86400),
                "revised_7d": bool(updated and now - updated <= 7 * 86400),
                "url": f"https://www.easa.europa.eu/en/node/{z.get('Nid')}", "source": "Source: EASA"}
        cache = {"fetched": now, "items": items}
        save("easa_czib.json", cache)
        fetched = len(items)
    return fetched, [obs(k, "airspace-advisory", slot(1800), a) for k, a in cache["items"].items()]


# ------------------------------------------------- FAA flight prohibitions and advisories (keyless)
#
# The FAA's "Prohibitions, Restrictions and Notices" page lists, per country or ocean area, the
# Special Federal Aviation Regulations (SFARs) and KICZ NOTAMs that bar or warn US operators
# (US Government work, public domain). One item per section, at the country's label point or
# the area's centre. The page carries no dates, so a document is "new" when this feed first
# sees it (not on the first fetch). Refetched every 6 h, resent every 30 min.

FAA_RESTRICTIONS = "https://www.faa.gov/air_traffic/publications/us_restrictions"
FAA_AREAS = {"Persian Gulf and Gulf of Oman": (26.0, 55.0), "Pacific Ocean": (8.0, -100.0),
             "Central America": (14.5, -87.0)}
FAA_COUNTRY_ALIASES = {"Korea, North": "North Korea", "Russian Federation": "Russia"}


def src_faa_restrictions():
    """US FAA flight prohibitions and advisories for US operators (SFARs and KICZ NOTAMs) per
    country or ocean area, from the FAA's public Prohibitions, Restrictions and Notices page."""
    cache = load("faa_restrictions.json", {"fetched": 0, "items": {}, "first_seen": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 6 * 3600:
        html = get_text(FAA_RESTRICTIONS, timeout=60)
        main = html.split('id="content"', 1)[-1]
        pts = {v["name"]: v for v in country_points().values()}
        first = cache.get("first_seen") or {}
        initial = not first
        items = {}
        parts = re.split(r"<h2[^>]*>(.*?)</h2>", main, flags=re.S)
        listing = False  # country sections run from the alphabetical index to the page footer
        for head, body in zip(parts[1::2], parts[2::2]):
            area = clean(re.sub(r"<[^>]+>|&nbsp;", " ", head), 80)
            if area and area.startswith("Alphabetical List"):
                listing = True
                continue
            if area and area.startswith("U.S. Department of Transportation"):
                break
            body = body.split("Back to top", 1)[0]  # most sections are a <ul>, Saudi Arabia a <p>
            if not listing or not area or area == "United States":
                continue
            docs = []
            for href, text in re.findall(r'<a href="([^"]+)"[^>]*>(.*?)</a>', body, re.S):
                title = clean(re.sub(r"<[^>]+>", "", text).replace("&nbsp;", " ").replace("&amp;", "&"), 200)
                if title and not href.startswith("#"):
                    docs.append((title, href))
            if not docs:
                continue
            p = FAA_AREAS.get(area)
            if not p:
                c = pts.get(FAA_COUNTRY_ALIASES.get(area, area))
                p = (c["lat"], c["lon"]) if c else None
            if not p:
                continue
            for title, _ in docs:
                first.setdefault(title, 0 if initial else int(now))
            titles = [t for t, _ in docs]
            items[f"FAA-{slug(area)}"] = {
                "lat": p[0], "lon": p[1], "name": f"FAA flight restriction · {area}", "area": area,
                "issuer": "FAA", "documents": " | ".join(titles)[:900], "document_count": len(docs),
                "prohibition": any("prohibition" in t.lower() for t in titles),
                "advisory": any("advisory" in t.lower() for t in titles),
                "new_doc_7d": any(first.get(t) and now - first[t] <= 7 * 86400 for t in titles),
                "url": FAA_RESTRICTIONS, "source": "US FAA (public domain)"}
        cache = {"fetched": now, "items": items, "first_seen": first}
        save("faa_restrictions.json", cache)
        fetched = len(items)
    return fetched, [obs(k, "airspace-advisory", slot(1800), a) for k, a in cache["items"].items()]


# ------------------------------------------------------------- OONI censorship measurements (keyless)
#
# OONI (Open Observatory of Network Interference; data CC BY 4.0, credit "OONI") collects web
# connectivity tests run by volunteers in each country. Per country: the share of tests that
# were anomalous or confirmed blocked since yesterday, against the 7 days before. A country is
# "rising" when that share is at least 10 points and 1.5 times above its baseline, which marks
# new blocking (often around protests, elections or conflict) rather than the steady
# censorship some countries always show. Refetched hourly, resent every 30 min.

OONI = "https://api.ooni.io/api/v1/aggregation"


def ooni_shares(since, until):
    d = get(f"{OONI}?test_name=web_connectivity&axis_x=probe_cc&since={since}&until={until}", timeout=90)
    return {r["probe_cc"]: r for r in (d or {}).get("result") or [] if r.get("probe_cc")}


def src_ooni():
    """Internet censorship per country from OONI web connectivity tests (CC BY 4.0): the share
    of tests anomalous or confirmed blocked since yesterday, against the previous 7 days."""
    cache = load("ooni.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 3600:
        day = lambda n: datetime.fromtimestamp(now + n * 86400, timezone.utc).strftime("%Y-%m-%d")
        recent = ooni_shares(day(-1), day(1))
        time.sleep(2)
        base = ooni_shares(day(-8), day(-1))
        pts = country_points()
        items = {}
        for cc, r in recent.items():
            n = r.get("measurement_count") or 0
            c = pts.get(cc)
            if n < 100 or not c:
                continue
            bad = (r.get("anomaly_count") or 0) + (r.get("confirmed_count") or 0)
            share = 100.0 * bad / n
            b = base.get(cc) or {}
            bn = b.get("measurement_count") or 0
            bshare = 100.0 * ((b.get("anomaly_count") or 0) + (b.get("confirmed_count") or 0)) / bn if bn >= 100 else None
            rising = bshare is not None and share >= bshare + 10 and share >= 1.5 * bshare
            if share < 10 and not rising:  # below 10% is mostly test noise (Germany shows ~6%)
                continue
            items[f"OONI-{cc}"] = {
                "lat": c["lat"], "lon": c["lon"], "name": f"Internet censorship tests · {c['name']}",
                "country": c["name"], "country_code": cc, "tests": n,
                "anomalies": r.get("anomaly_count"), "confirmed_blocked": r.get("confirmed_count"),
                "interference_pct": round(share, 1),
                "baseline_pct": round(bshare, 1) if bshare is not None else None, "rising": rising,
                "since": day(-1), "url": f"https://explorer.ooni.org/chart/mat?probe_cc={cc}&test_name=web_connectivity",
                "source": "OONI (CC BY 4.0)"}
        cache = {"fetched": now, "items": items}
        save("ooni.json", cache)
        fetched = len(items)
    return fetched, [obs(k, "utility-telecom-censorship", slot(1800), a) for k, a in cache["items"].items()]


# ------------------------------------------------------------- NOAA tsunami bulletins (keyless)
#
# The two US tsunami warning centres' Atom feeds (NTWC Palmer, Alaska and PTWC Honolulu; US
# Government work, public domain). Each feed holds the latest event; it is sent while its
# bulletin is under 24 hours old, with its category (Warning, Advisory, Watch, Threat,
# Information). Refetched every 5 min, resent every 10 min.

TSUNAMI_FEEDS = {"NTWC": "https://www.tsunami.gov/events/xml/PAAQAtom.xml",
                 "PTWC": "https://www.tsunami.gov/events/xml/PHEBAtom.xml"}


def src_tsunami():
    """Tsunami bulletins from the US warning centres (NOAA NTWC and PTWC, public domain) issued
    in the last 24 hours, with their category: Warning, Advisory, Watch, Threat or Information."""
    cache = load("tsunami.json", {"fetched": 0, "items": {}})
    fetched = "cached"
    now = time.time()
    if now - cache["fetched"] >= 300:
        items = {}
        for centre, url in TSUNAMI_FEEDS.items():
            xml = get_text(url, timeout=60)
            for e in re.findall(r"<entry>(.*?)</entry>", xml, re.S):
                tag = lambda t: (re.search(rf"<{t}>(.*?)</{t}>", e, re.S) or [None, None])[1]
                updated, lat, lon = iso(tag("updated")), num(tag("geo:lat")), num(tag("geo:long"))
                if not updated or lat is None or lon is None or now - updated > 86400:
                    continue
                summary = re.sub(r"<[^>]+>", " ", tag("summary") or "")
                field = lambda f: clean((re.search(rf"{f}:\s*(.*?)\s{{2,}}", summary + "  ") or [None, None])[1], 80)
                category = field("Category") or "Information"
                bulletin = re.search(r'title="Bulletin" href="([^"]+)"', e)
                items[f"TSUNAMI-{centre}-{int(updated)}"] = {
                    "lat": lat, "lon": lon, "name": f"Tsunami {category.lower()} · {clean(tag('title'), 80)}",
                    "centre": centre, "category": category, "region": field("Affected Region"),
                    "magnitude": field("Preliminary Magnitude"), "issued_at": updated,
                    "url": bulletin.group(1) if bulletin else url, "source": "NOAA tsunami.gov (public domain)"}
        cache = {"fetched": now, "items": items}
        save("tsunami.json", cache)
        fetched = len(items)
    return fetched, [obs(k, "event-tsunami", slot(600), a) for k, a in cache["items"].items()]


# ------------------------------------------------------------- GDELT news volume (keyless)
#
# The GDELT Project's DOC 2.0 API (free for any use with attribution, "GDELT Project") searches
# worldwide online news in 65 languages, updated every 15 minutes. For each watched topic: the
# number of matching articles in the last hour against its hourly median over 24 hours (a
# "surge" is 3x the median and at least 20 articles) and the latest headlines. GDELT asks for
# no more than one request every 5 s, so topics are fetched 6 s apart and a 429 ends the cycle
# (the runner backs off); topics not refreshed keep their last values. Refetched every 15 min.

GDELT_DOC = "https://api.gdeltproject.org/api/v2/doc/doc"
GDELT_TOPICS = {
    "hormuz": ("Strait of Hormuz and Gulf shipping", (26.6, 56.3),
               '("strait of hormuz" OR "gulf of oman" OR "persian gulf") (tanker OR ship OR vessel OR navy)'),
    "iran": ("Iran military activity", (32.4, 53.7), "iran (strike OR missile OR drone OR attack OR airspace)"),
    "red-sea": ("Red Sea and Houthi attacks", (15.5, 41.5), '(houthi OR "red sea" OR "bab el-mandeb") (attack OR missile OR ship)'),
    "levant": ("Israel, Gaza and Lebanon", (31.8, 35.0), "(israel OR gaza OR lebanon OR hezbollah) (strike OR attack OR rocket)"),
    "ukraine": ("Ukraine and Russia", (49.0, 32.0), "(ukraine OR russia) (missile OR drone OR strike OR offensive)"),
    "uk-infra": ("UK infrastructure disruption", (53.5, -1.5),
                 '("power cut" OR blackout OR flooding OR "rail disruption" OR cyberattack) (uk OR britain OR england OR scotland)'),
}


def src_gdelt():
    """Online news volume and latest headlines for watched topics (GDELT Project DOC 2.0 API,
    free with attribution): articles in the last hour against the 24-hour hourly median."""
    cache = load("gdelt.json", {"items": {}, "fetched": {}})
    now = time.time()
    refreshed = 0
    first = True
    for key, (title, (lat, lon), query) in GDELT_TOPICS.items():
        if now - cache["fetched"].get(key, 0) < 900:
            continue
        q = urllib.parse.quote(query)
        try:
            if not first:
                time.sleep(6)
            first = False
            vol = get(f"{GDELT_DOC}?query={q}&mode=timelinevolraw&timespan=24h&format=json", timeout=60)
            time.sleep(6)
            arts = get(f"{GDELT_DOC}?query={q}&mode=artlist&maxrecords=5&sort=datedesc&timespan=1h&format=json", timeout=60)
        except RateLimited:
            if not refreshed and not cache["items"]:
                raise
            log_line("gdelt-news", f"429 at topic {key}; keeping the rest until next cycle")
            break
        series = [(iso(p.get("date")), p.get("value") or 0)
                  for t in (vol or {}).get("timeline") or [] for p in t.get("data") or []]
        series = [(t, v) for t, v in series if t]
        hours = {}
        for t, v in series:
            hours[int(t // 3600)] = hours.get(int(t // 3600), 0) + v
        last_hour = sum(v for t, v in series if now - t <= 3600)
        past = sorted(v for h, v in hours.items() if h < int(now // 3600) - 1)
        median = past[len(past) // 2] if past else None
        heads = [(a.get("title") or "", a.get("domain") or "") for a in (arts or {}).get("articles") or []]
        cache["items"][f"GDELT-{key}"] = {
            "lat": lat, "lon": lon, "name": f"News: {title}", "topic": title, "query": query,
            "articles_1h": last_hour, "hourly_median_24h": median,
            "surge": bool(median is not None and last_hour >= 20 and last_hour >= 3 * max(median, 1)),
            "headlines": " | ".join(f"{clean(t, 140)} ({d})" for t, d in heads)[:900] or None,
            "url": f"https://api.gdeltproject.org/api/v2/doc/doc?query={q}&mode=artlist&timespan=1h",
            "source": "GDELT Project"}
        cache["fetched"][key] = now
        refreshed += 1
    save("gdelt.json", cache)
    return refreshed, [obs(k, "event-news", slot(900), a) for k, a in cache["items"].items()]


# ------------------------------------------------------------- Gulf watch (derived, no request of its own)
#
# Indicators for the Gulf and Iran built from what the other feeds already fetched, each against
# its normal level, standing in for sources that are closed to us (NOTAMs, UKMTO, commercial AIS):
#   civil aircraft over Iran and Iraq (airlines reroute within hours of a closure or a threat),
#   ships tracked in the Gulf and moving through Hormuz, military aircraft by role over the
#   region, GPS interference cells around the Gulf, and satellite fires at sea where no fire
#   has been seen before (a burning ship rather than a gas flare or an oil platform).
# Counts are distinct aircraft or ships in the last 60 minutes. The normal level is the median
# of the same hour on earlier days (up to 7), or of the last 24 hours while fewer than one day
# is stored; hours when a feed was down are left out. The first run seeds 24 hours of aircraft
# and ship counts from Heimdall's history (through the gateway, as the analyst) and every fire
# at sea FIRMS still holds (from 1 July, 5 days per request), so most indicators work at once;
# military and GPS counts learn from live. Offshore flares burn on and off, so a fire is "new"
# only where no fire was seen on any earlier day since then and no OpenStreetMap offshore
# platform is within 3 km. Iran itself has almost no ADS-B receivers (0-5 aircraft an hour are
# tracked there), so civil traffic over the Gulf states is the main flight indicator.

GW_STATE = "gulf_watch.json"
GW_WINDOW = 3600
GW_REGION = (10.0, 42.0, 30.0, 65.0)  # south, north, west, east: countries kept for the land mask
GW_GULF_BOX = (22.0, 30.5, 47.5, 60.0)  # Persian Gulf, Strait of Hormuz, Gulf of Oman (the AIS box)
GW_HORMUZ_BOX = (25.3, 27.2, 55.4, 57.6)
GW_MIL_BOX = (12.0, 40.0, 34.0, 63.0)  # Gulf states, Iran, Iraq, Levant, Red Sea
GW_FIRE_BOX = (10.0, 31.0, 32.0, 62.0)  # Red Sea, Gulf of Aden, Arabian Sea north, Gulf, Gulf of Oman
GW_FIRE_SINCE = "2026-07-01"  # the oldest day FIRMS's near-real-time VIIRS archive holds
GW_GULF_STATES_BOX = (22.0, 30.5, 46.0, 57.0)  # Kuwait, Bahrain, Qatar, UAE, eastern Saudi Arabia
GW_ROLES = {
    "tanker": {"K35R", "K35E", "K35T", "KC46", "K46", "A332", "A339", "MRTT", "DC10", "KC10", "A310", "IL78"},
    "surveillance": {"E3TF", "E3CF", "E737", "E7", "R135", "P8", "P8A", "E6", "E8", "U2", "Q4", "RQ4", "Q9", "MQ9",
                     "HRON", "TB2", "AKNC", "GLEX", "CL60", "B350", "PC12", "DH8C"},
    "airlift": {"C17", "C5M", "C5", "C130", "C30J", "A400", "IL76", "C2", "Y20", "C295", "CN35", "C27J", "B190"},
}
GW_SEED_HISTORY = "http://127.0.0.1:7710/api/v1"
GW_PASSWORDS = os.path.join(MID, "airports-passwords.txt")
GULF = {"air": {}, "mil": {}, "fires": [], "fires_at": 0}  # live memory: hex -> (t, region), filled by the feeds


def in_box(lat, lon, box):
    return box[0] <= lat <= box[1] and box[2] <= lon <= box[3]


def ring_contains(ring, lat, lon):
    """Even-odd point-in-polygon on a ring of [lon, lat] points (GeoJSON order)."""
    inside = False
    j = len(ring) - 1
    for i in range(len(ring)):
        xi, yi = ring[i]
        xj, yj = ring[j]
        if (yi > lat) != (yj > lat) and lon < (xj - xi) * (lat - yi) / ((yj - yi) or 1e-12) + xi:
            inside = not inside
        j = i
    return inside


def gw_countries():
    """Natural Earth 50 m outlines of the countries in GW_REGION: {code: [(bbox, outer ring)]}."""
    def fetch():
        d = get(NE_COUNTRIES, timeout=120)
        out = {}
        s, n, w, e = GW_REGION
        for f in d.get("features") or []:
            p = f.get("properties") or {}
            code = p.get("ISO_A2_EH") if p.get("ISO_A2_EH") not in (None, "-99") else p.get("ISO_A2")
            g = f.get("geometry") or {}
            polys = [g["coordinates"]] if g.get("type") == "Polygon" else g.get("coordinates") or []
            for poly in polys:
                ring = poly[0]
                xs, ys = [x for x, _ in ring], [y for _, y in ring]
                bbox = (min(ys), max(ys), min(xs), max(xs))
                if bbox[1] < s or bbox[0] > n or bbox[3] < w or bbox[2] > e:
                    continue
                out.setdefault(code or p.get("NAME"), []).append((bbox, [[round(x, 4), round(y, 4)] for x, y in ring]))
        return out
    cache = load("gulf_countries.json", None)
    if not cache:
        cache = fetch()
        save("gulf_countries.json", cache)
    return cache


def in_country(polys, lat, lon):
    return any(b[0] <= lat <= b[1] and b[2] <= lon <= b[3] and ring_contains(r, lat, lon) for b, r in polys)


def on_land(countries, lat, lon):
    return any(in_country(p, lat, lon) for p in countries.values())


def gulf_air_record(seen):
    """Remember which aircraft each adsb.fi cycle saw over Iran, Iraq and the military box."""
    try:
        countries = gw_countries()
    except Exception as e:
        log_line("gulf-watch", f"country outlines unavailable ({type(e).__name__}: {e})")
        return
    now = time.time()
    for hexid, (a, t) in seen.items():
        lat, lon = a.get("lat"), a.get("lon")
        if lat is None or lon is None or not in_box(lat, lon, GW_MIL_BOX):
            continue
        mil = (num(a.get("dbFlags"), int) or 0) & 1
        if mil:
            ty = (a.get("t") or "").upper()
            role = next((r for r, types in GW_ROLES.items() if ty in types), "other")
            GULF["mil"][hexid] = (now, role, clean(a.get("flight")) or hexid.upper(), ty or "?")
        elif a.get("alt_baro") != "ground":
            where = ("IR" if in_country(countries.get("IR", []), lat, lon) else "IQ" if in_country(countries.get("IQ", []), lat, lon)
                     else "GS" if in_box(lat, lon, GW_GULF_STATES_BOX) else None)
            if where:
                GULF["air"][hexid] = (now, where)
    GULF["cycles"] = [c for c in GULF.get("cycles", []) if now - c <= GW_WINDOW] + [now]
    for d in (GULF["air"], GULF["mil"]):
        for k in [k for k, v in d.items() if now - v[0] > GW_WINDOW]:
            d.pop(k)


def gulf_fire_record(rows):
    """Keep the day's FIRMS rows in the fire box for the fire-at-sea check."""
    GULF["fires"] = [r for r in rows if in_box(r[0], r[1], GW_FIRE_BOX)]
    GULF["fires_at"] = time.time()


def gw_cell(lat, lon):
    return f"{round(lat / 0.02) * 0.02:.2f},{round(lon / 0.02) * 0.02:.2f}"


def gw_neighbours(lat, lon):
    return {gw_cell(lat + dy * 0.02, lon + dx * 0.02) for dy in (-1, 0, 1) for dx in (-1, 0, 1)}


def gw_day(t):
    return int(t // 86400)


def gw_seed_fires(state):
    """Every fire at sea in the fire box since GW_FIRE_SINCE (Suomi NPP, 5 days per request),
    so flares and platforms that burn on and off are known. Resumable: progress is saved."""
    key = open(FIRMS_KEY).read().strip()
    countries = gw_countries()
    s, n, w, e = GW_FIRE_BOX
    day = state.get("fires_seed_day") or GW_FIRE_SINCE
    land = {}
    while day < utc()[:10]:
        text = get_text(f"https://firms.modaps.eosdis.nasa.gov/api/area/csv/{key}/VIIRS_SNPP_NRT/{w},{s},{e},{n}/5/{day}", timeout=180)
        if not text.startswith("latitude"):
            raise RuntimeError(f"FIRMS seed {day}: {text[:80]!r}")
        for f in csv.DictReader(io.StringIO(text)):
            lat, lon = float(f["latitude"]), float(f["longitude"])
            cell = gw_cell(lat, lon)
            if cell not in land:
                land[cell] = on_land(countries, lat, lon)
            if not land[cell]:
                days = state["fire_cells"].setdefault(cell, [])
                d = gw_day(iso(f["acq_date"] + "T00:00:00Z"))
                if d not in days:
                    days.append(d)
        day = (datetime.strptime(day, "%Y-%m-%d") + timedelta(days=5)).strftime("%Y-%m-%d")
        state["fires_seed_day"] = day
        save(GW_STATE, state)
        time.sleep(1)


def gw_platforms():
    """OpenStreetMap offshore platforms in the fire box, refetched weekly ((c) OpenStreetMap
    contributors, ODbL): cells within about 3 km of one are never a "new" fire."""
    def fetch():
        s, n, w, e = GW_FIRE_BOX
        els = overpass(f'[out:json][timeout:180];nwr["man_made"="offshore_platform"]({s},{w},{n},{e});out center;')
        cells = set()
        for el in els:
            lat, lon = el.get("lat") or (el.get("center") or {}).get("lat"), el.get("lon") or (el.get("center") or {}).get("lon")
            if lat is not None and lon is not None:
                cells |= {gw_cell(lat + dy * 0.02, lon + dx * 0.02) for dy in (-1, 0, 1) for dx in (-1, 0, 1)}
        return {"fetched": time.time(), "cells": sorted(cells), "platforms": len(els)}
    cache = load("gulf_platforms.json", None)
    if not cache or time.time() - cache["fetched"] > 7 * 86400:
        try:
            cache = fetch()
            save("gulf_platforms.json", cache)
        except Exception as e:
            log_line("gulf-watch", f"OpenStreetMap platforms unavailable ({type(e).__name__}: {e})")
            cache = cache or {"fetched": 0, "cells": [], "platforms": 0}
    return set(cache["cells"])


def gw_seed_history(state):
    """24 hours of distinct aircraft over Iran and Iraq and ships in the Gulf boxes, per hour,
    from Heimdall's history (one point per track every 5 min)."""
    pw = re.search(r"analyst \(internal\):\s*(\S+)", open(GW_PASSWORDS).read())
    if not pw:
        raise RuntimeError("no analyst line in the passwords file")
    req = urllib.request.Request(GW_SEED_HISTORY + "/login", data=json.dumps({"username": "analyst", "password": pw.group(1)}).encode(),
                                 headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as r:
        tok = json.load(r)["token"]
    countries = gw_countries()
    now = int(time.time())
    hours = {}
    for layer in ("air", "sea"):
        req = urllib.request.Request(f"{GW_SEED_HISTORY}/heimdall/history?from={now - 86400}&to={now}&layer={layer}&step=300",
                                     headers={"Authorization": "Bearer " + tok})
        with urllib.request.urlopen(req, timeout=300) as r:
            d = json.load(r)
        for tr in d.get("tracks") or []:
            sets = {}
            for t, lat, lon, alt in tr["points"]:
                if lat is None or lon is None:
                    continue
                h = int(t // 3600)
                hours.setdefault(h, {}).setdefault("coverage_" + layer, set()).add(tr["id"])
                if layer == "air" and (alt or 0) > 0:
                    if in_box(lat, lon, GW_MIL_BOX):
                        where = ("iran_air" if in_country(countries.get("IR", []), lat, lon) else "iraq_air" if in_country(countries.get("IQ", []), lat, lon)
                                 else "gulf_air" if in_box(lat, lon, GW_GULF_STATES_BOX) else None)
                        if where:
                            sets.setdefault((h, where), True)
                elif layer == "sea":
                    if in_box(lat, lon, GW_GULF_BOX):
                        sets.setdefault((h, "gulf_ships"), True)
                    if in_box(lat, lon, GW_HORMUZ_BOX):
                        sets.setdefault((h, "hormuz_ships"), True)
            for (h, k) in sets:
                hours[h][k] = hours[h].get(k, 0) + 1
    current = now // 3600
    for h, v in hours.items():
        if h >= current:
            continue  # the current hour is still filling
        row = {k: v.get(k, 0) for k in ("iran_air", "iraq_air", "gulf_air", "gulf_ships", "hormuz_ships")}
        row["coverage_air"], row["coverage_sea"] = len(v.get("coverage_air", ())), len(v.get("coverage_sea", ()))
        row["seeded"] = True
        state["hours"][str(h)] = row


def gw_baseline(state, key, cov_key, now):
    """(median normal level, how it was found) or (None, why not)."""
    hours = state["hours"]
    h = int(now // 3600)
    # Seeded hours count only if Heimdall had at least half its usual tracks then (feeds were
    # paused on some days); live hours are written only while their feed is running.
    covs = sorted(v.get(cov_key, 0) for v in hours.values() if v.get("seeded"))
    floor = covs[len(covs) // 2] * 0.5 if covs else 0
    ok = lambda r: r is not None and key in r and (not r.get("seeded") or r.get(cov_key, 0) >= max(floor, 1))
    same = [hours[str(h - 24 * k)][key] for k in range(1, 8) if ok(hours.get(str(h - 24 * k)))]
    if same:
        return sorted(same)[len(same) // 2], f"same hour on {len(same)} earlier day(s)"
    recent = [hours[str(h - k)][key] for k in range(2, 26) if ok(hours.get(str(h - k)))]
    if len(recent) >= 6:
        return sorted(recent)[len(recent) // 2], f"last {len(recent)} hours"
    return None, f"learning ({len(recent)} of 6 hours stored)"


def src_gulf_watch():
    """Gulf watch: civil aircraft over Iran and Iraq, ships in the Gulf and through Hormuz,
    military aircraft by role, GPS interference and new fires at sea, each against its normal
    level, derived from the other feeds (no request of its own except a one-off seed)."""
    state = load(GW_STATE, None) or {"hours": {}, "fire_cells": {}, "seeded": False, "fires_seeded": False}
    if not state.get("seeded"):
        try:
            gw_seed_history(state)
            state["seeded"] = True
        except Exception as e:
            log_line("gulf-watch", f"history seed failed ({type(e).__name__}: {e}); learning from live instead")
            state["seeded"] = True
    if not state.get("fires_seeded"):
        try:
            gw_seed_fires(state)
            state["fires_seeded"] = True
        except Exception as e:
            log_line("gulf-watch", f"FIRMS seed failed ({type(e).__name__}: {e}); retrying next cycle")
    now = time.time()
    h = str(int(now // 3600))
    countries = gw_countries()
    # An indicator is judged only once its feed has run for most of the 60-minute window.
    cycles = GULF.get("cycles", [])
    air_live = bool(cycles) and now - max(cycles) < 600 and now - min(cycles) >= GW_WINDOW - 600
    if AIS.get("status") != "connected":
        GULF.pop("ais_since", None)
    elif "ais_since" not in GULF:
        GULF["ais_since"] = now
    ais_live = now - GULF.get("ais_since", now) >= GW_WINDOW - 600
    gulf_ships = hormuz_ships = 0
    if ais_live:
        for p in list(AIS["positions"].values()):
            if p.get("lat") is None or p.get("lon") is None or now - p["t"] > GW_WINDOW:
                continue
            gulf_ships += in_box(p["lat"], p["lon"], GW_GULF_BOX)
            hormuz_ships += in_box(p["lat"], p["lon"], GW_HORMUZ_BOX) and (p.get("sog") or 0) >= 3
    mil = list(GULF["mil"].values())
    roles = {r: sum(1 for m in mil if m[1] == r) for r in ("tanker", "surveillance", "airlift", "other")}
    gnss_cells = set()
    for t, cells in GNSS["cycles"]:
        if now - t <= 900:
            for (la, lo), acs in cells.items():
                if in_box(la + 0.5, lo + 0.5, GW_MIL_BOX) and sum(acs.values()) >= 2:
                    gnss_cells.add((la, lo))
    values = {"iran_air": sum(1 for v in GULF["air"].values() if v[1] == "IR"),
              "iraq_air": sum(1 for v in GULF["air"].values() if v[1] == "IQ"),
              "gulf_air": sum(1 for v in GULF["air"].values() if v[1] == "GS"),
              "gulf_ships": gulf_ships, "hormuz_ships": hormuz_ships, "military": len(mil), "gnss_cells": len(gnss_cells)}
    row = state["hours"].setdefault(h, {})
    row.pop("seeded", None)  # a live value replaces the seeded one for this hour
    if air_live:
        row.update({k: values[k] for k in ("iran_air", "iraq_air", "gulf_air", "military", "gnss_cells")})
    if ais_live:
        row.update({k: values[k] for k in ("gulf_ships", "hormuz_ships")})
    for k in [k for k in state["hours"] if int(k) < int(h) - 24 * 8]:
        state["hours"].pop(k)

    # (key, title, point, coverage key, live?, direction, unit)
    specs = [("gulf_air", "Civil aircraft over the Gulf states", (25.0, 51.5), "coverage_air", air_live, "drop", "aircraft"),
             ("iran_air", "Civil aircraft over Iran (few receivers: 0-5 an hour tracked)", (32.4, 53.7), "coverage_air", air_live, "drop", "aircraft"),
             ("iraq_air", "Civil aircraft over Iraq", (33.2, 43.7), "coverage_air", air_live, "drop", "aircraft"),
             ("gulf_ships", "Ships tracked in the Gulf and Gulf of Oman", (26.8, 52.0), "coverage_sea", ais_live, "drop", "ships"),
             ("hormuz_ships", "Ships moving through the Strait of Hormuz", (26.4, 56.4), "coverage_sea", ais_live, "drop", "ships"),
             ("military", "Military aircraft over the Gulf and Middle East", (25.3, 51.4), "coverage_air", air_live, "rise", "aircraft"),
             ("gnss_cells", "GPS interference around the Gulf", (27.0, 56.0), "coverage_air", air_live, "rise", "cells")]
    out, changed = [], []
    t_obs = int(now)
    for key, title, (lat, lon), cov, live, direction, unit in specs:
        base, how = gw_baseline(state, key, cov, now)
        v = values[key]
        if not live:
            status, note = "warming up", "its feed has not yet run for the full 60 minutes"
        elif base is None:
            status, note = "learning", how
        elif direction == "drop":
            status = "elevated" if base >= 10 and v <= 0.6 * base else "normal"
            note = f"normal {base} ({how})"
        else:
            status = "elevated" if v >= max(2 * base, base + 4) else "normal"
            note = f"normal {base} ({how})"
        change = round(100.0 * (v - base) / base) if base else None
        attrs = {"lat": lat, "lon": lon, "name": f"Gulf watch · {title}", "indicator": title, "value": v, "unit": unit,
                 "baseline": base, "change_pct": change, "status": status, "baseline_note": note,
                 "watch": "gulf", "source": "derived (Heimdall feeds)"}
        if key == "military":
            attrs["roles"] = ", ".join(f"{r} {n}" for r, n in roles.items() if n) or None
            attrs["aircraft"] = ", ".join(sorted(f"{m[2]} ({m[3]})" for m in mil if m[1] != "other"))[:600] or None
        if status == "elevated":
            changed.append(f"{title}: {v} {unit} against a normal {base} ({change:+d} %)")
        out.append(obs(f"GW-{key.upper().replace('_', '-')}", "event-indicator", t_obs, attrs))

    # Fires at sea where nothing burned in the previous 5 days (flares and platforms burn daily).
    fires = 0
    platforms = gw_platforms() if state.get("fires_seeded") else set()
    new_fires = {}
    for lat, lon, at, frp, conf in GULF["fires"] if state.get("fires_seeded") else []:
        if on_land(countries, lat, lon) or gw_neighbours(lat, lon) & platforms:
            continue
        if any(d < gw_day(at) for c in gw_neighbours(lat, lon) for d in state["fire_cells"].get(c, [])):
            continue
        k = f"GW-FIRE-{lat:.2f}-{lon:.2f}"
        if k in new_fires and (new_fires[k][3] or 0) >= (frp or 0):
            continue
        new_fires[k] = (lat, lon, at, frp, conf)
    for k, (lat, lon, at, frp, conf) in new_fires.items():
        near = None
        for mmsi, p in list(AIS["positions"].items()):
            if p.get("lat") is None or p.get("lon") is None:
                continue
            d = math.hypot(p["lat"] - lat, (p["lon"] - lon) * math.cos(math.radians(lat))) * 111.0
            if d <= 20 and (near is None or d < near[0]):
                s = AIS["static"].get(mmsi, {})
                near = (d, clean(s.get("name") or p.get("name")) or f"MMSI {mmsi}", mmsi)
        fires += 1
        title = "New fire at sea"
        changed.append(f"{title} at {lat:.2f}, {lon:.2f} ({utc(at)})")
        out.append(obs(k, "event-fire-at-sea", int(at), {
            "lat": lat, "lon": lon, "name": f"Gulf watch · {title}", "indicator": title, "status": "elevated",
            "value": frp, "unit": "MW fire power", "confidence": conf, "detected_at": int(at),
            "baseline_note": f"no satellite fire within about 3 km on any day since {GW_FIRE_SINCE}, not on land, no mapped offshore platform within 3 km",
            "nearest_ship": f"{near[1]} ({near[0]:.0f} km, current AIS position)" if near else "none tracked within 20 km",
            "watch": "gulf", "source": "derived (NASA FIRMS, AISStream)"}))
    for lat, lon, at, _, _ in GULF["fires"] if state.get("fires_seeded") else []:
        if not on_land(countries, lat, lon):
            days = state["fire_cells"].setdefault(gw_cell(lat, lon), [])
            if gw_day(at) not in days:
                days.append(gw_day(at))
                del days[:-30]

    learning = [o["attrs"]["indicator"] for o in out if o["attrs"]["status"] in ("learning", "warming up")]
    summary = ("; ".join(changed) if changed else "nothing unusual") + (f". Not yet judged: {', '.join(learning)}" if learning else "")
    out.append(obs("GW-SUMMARY", "event-indicator", t_obs, {
        "lat": 27.8, "lon": 50.2, "name": "Gulf watch · summary", "indicator": "Summary",
        "status": "elevated" if changed else "normal", "elevated_count": len(changed), "summary": summary[:900],
        "watch": "gulf", "source": "derived (Heimdall feeds)"}))
    save(GW_STATE, state)
    return f"{len(changed)} elevated, {fires} new fires at sea, values {values}", out


SOURCES = {
    # utilities (GB): live power cuts, grid status and interconnectors, the fixed grid (weekly)
    "power-cuts": ("power-cuts", src_power_cuts, 60),
    # conflict-zone airspace warnings (EASA)
    "easa-czib": ("easa-czib", src_easa_czib, 3600),
    # US FAA prohibitions, OONI censorship, US tsunami centres, GDELT news volume
    "faa-restrictions": ("faa-restrictions", src_faa_restrictions, 1800),
    "ooni-censorship": ("ooni-censorship", src_ooni, 1800),
    "tsunami-warnings": ("tsunami-warnings", src_tsunami, 300),
    "gdelt-news": ("gdelt-news", src_gdelt, 900),
    # derived from adsb-fi, nasa-firms, aisstream and gnss-interference (in memory)
    "gulf-watch": ("gulf-watch", src_gulf_watch, 120),
    "gb-grid": ("gb-grid", src_gb_grid, 60),
    "power-infra": ("power-infra", src_power_infra, 3600),
    "storm-overflows": ("storm-overflows", src_overflows, 60),
    "reservoirs": ("reservoirs", src_reservoirs, 60),
    "water-infra": ("water-infra", src_water_infra, 3600),
    "gas-flows": ("gas-flows", src_gas, 60),
    "gas-infra": ("gas-infra", src_gas_infra, 3600),
    "gie-gas": ("gie-gas", src_gie, 60),
    "internet-outages": ("internet-outages", src_internet_outages, 60),
    "cloudflare-radar": ("cloudflare-radar", src_cf_outages, 60),
    "telecom-infra": ("telecom-infra", src_telecom_infra, 3600),
    "tfl-jamcams": ("tfl-jamcams", src_tfl_jamcams, 60),
    "custom-cameras": ("custom-cameras", src_custom_cameras, 60),
    "network-rail": ("network-rail", src_network_rail, 60),
    "national-highways": ("national-highways", src_national_highways, 60),
    "openaip": ("openaip", src_openaip, 3600),
    "nasa-firms": ("nasa-firms", src_firms, 1800),
    "aisstream": ("aisstream", src_aisstream, 60),
    "airports-metar": ("airports-metar", src_airports, 120),
    "digitraffic-ais": ("digitraffic-ais", src_digitraffic, 60),
    # 46 circles at >= 1.1 s apart take ~63 s, longer than the old 50 s interval: now 90 s
    "adsb-fi": ("adsb-fi", src_adsb_fi, 90),
    # derived from the aircraft adsb-fi fetched (in memory, last 15 min); no request of its own
    "gnss-interference": ("gnss-interference", src_gnss, 120),
    "sondehub": ("sondehub", src_sondehub, 60),
    "celestrak": ("celestrak", src_celestrak, 60),
    "usgs-quakes": ("usgs-quakes", src_usgs, 120),
    "ea-floods": ("ea-floods", src_floods, 120),
    # these four refetch on their own schedule (10/15/5 min) and resend per time slot, so the
    # 60 s cycle only keeps the resend close to the start of each slot
    "aviationweather-sigmet": ("aviationweather-sigmet", src_sigmet, 60),
    "metoffice-warnings": ("metoffice-warnings", src_metoffice, 60),
    "tfl-roads": ("tfl-roads", src_tfl, 60),
    "noaa-swpc": ("noaa-swpc", src_swpc, 600),
    # static: refetched and sent once a UTC day (an hourly cycle is a no-op the rest of the day)
    "crown-estate-wind": ("crown-estate-wind", src_windfarms, 3600),
    "nsta-platforms": ("nsta-platforms", src_platforms, 3600),
    # keyless, outside the UK (refetch on their own schedule, resend per slot)
    "gdacs": ("gdacs", src_gdacs, 60),
    "meteoalarm": ("meteoalarm", src_meteoalarm, 60),
    "sg-traffic-cams": ("sg-traffic-cams", src_sg_cams, 300),
    "nz-traffic-cams": ("nz-traffic-cams", src_nz_cams, 60),
    # regional keyless feeds (each refetches on its own schedule and resends per slot)
    "eccc-alerts": ("ca-alerts", src_eccc_alerts, 60),
    "drivebc-cams": ("ca-cams", src_drivebc_cams, 60),
    "drivebc-events": ("regional-extra", src_drivebc_events, 60),
    "mrwa-roads": ("au-traffic", src_mrwa_roads, 60),
    "nzta-roads": ("regional-extra", src_nzta_roads, 60),
    "metservice-warnings": ("regional-extra", src_metservice, 60),
}
BACKOFF = {}  # source -> (until, seconds)


def log_line(source, text):
    line = f"{utc()} {source:<12} {text}"
    print(line, flush=True)
    with open(LOG, "a") as f:
        f.write(line + "\n")


# While midgard-app's tools/mac-app/record.py records, state/record_to holds a directory and the
# recorder's pid ("<dir>\npid=<n>"): every batch is also copied there as <time_ns>-<feed>.jsonl
# (written aside, then renamed), so sources sharing a feed file (regional-extra) cannot overwrite
# each other before the recorder sees them. A hook whose recorder has gone is removed, so a
# killed recorder cannot leave copies piling up; and a failed copy never stops the live send.
RECORD_TO = os.path.join(STATE, "record_to")


def record_copy(feed, path):
    try:
        with open(RECORD_TO) as f:
            lines = f.read().split("\n")
    except OSError:
        return
    try:
        target = lines[0].strip()
        pid = next((int(x[4:]) for x in lines[1:] if x.startswith("pid=")), None)
        alive = pid is not None
        if alive:
            try:
                os.kill(pid, 0)
            except ProcessLookupError:
                alive = False
            except PermissionError:
                pass
        if not alive or not target or not os.path.isdir(target):
            os.remove(RECORD_TO)
            log_line("runner", f"removed stale recording hook ({target or 'no directory'})")
            return
        name = f"{time.time_ns()}-{feed}.jsonl"
        tmp = os.path.join(target, "." + name)
        with open(path, "rb") as src, open(tmp, "wb") as dst:
            dst.write(src.read())
        os.replace(tmp, os.path.join(target, name))
    except Exception as e:  # recording is an aid: the live send goes ahead regardless
        log_line(feed, f"recording copy failed ({type(e).__name__}: {e})")


def send(feed, observations):
    """Submit new observations; returns (sent, hsource summary)."""
    last = load(f"last_sent/{feed}.json", {})
    fresh = [o for o in observations if last.get(o["entity"], -1) < o["observedAt"]]
    if not fresh:
        return 0, "nothing new to send"
    os.makedirs(OUT, exist_ok=True)
    path = os.path.join(OUT, f"{feed}.jsonl")
    with open(path, "w") as f:
        for o in fresh:
            f.write(json.dumps(o, separators=(",", ":")) + "\n")
    record_copy(feed, path)
    in_container = "/mid/" + os.path.relpath(path, MID)
    p = subprocess.run([HSOURCE, "--service", feed, "--batch", in_container], stdin=subprocess.DEVNULL,
                       capture_output=True, text=True, timeout=300)
    text = (p.stdout + p.stderr).strip()
    m = re.search(r"batch: (\d+) accepted, (\d+) refused, (\d+) unreadable", text)
    if m:
        # the batch reached Heimdall: record what was sent (refused ids must not be retried either)
        for o in fresh:
            last[o["entity"]] = o["observedAt"]
        cutoff = time.time() - 86400
        save(f"last_sent/{feed}.json", {k: v for k, v in last.items() if v >= cutoff})
        return len(fresh), f"accepted={m.group(1)} refused={m.group(2)} unreadable={m.group(3)}"
    return len(fresh), f"hsource FAILED (exit {p.returncode}): {text.splitlines()[-1] if text else 'no output'}"


# Each source's last run and its totals, written to state/status.json after every run
# so the Forge console can show the feeds' live health (counts and outcomes only; no
# data). Totals carry over a restart.
STATUS = load("status.json", {}).get("sources", {})


def describe(fn):
    """A source's description: the first sentence of its function's docstring."""
    doc = " ".join((fn.__doc__ or "").split())
    return doc.split(". ")[0].rstrip(".") if doc else ""


def record(name, outcome, message, t0, fetched=None, sent=None):
    feed, fn, every = SOURCES[name]
    s = STATUS.setdefault(name, {"runs": 0, "failures": 0})
    s.update({"feed": feed, "every": every, "description": describe(fn) or SOURCE_NOTES.get(name, ""),
              "lastRun": int(t0), "durationS": round(time.time() - t0, 1), "outcome": outcome,
              "message": message[:300], "fetched": fetched, "sent": sent})
    m = re.search(r"accepted=(\d+) refused=(\d+)", message)
    s["accepted"], s["refused"] = (int(m.group(1)), int(m.group(2))) if m else (None, None)
    s["runs"] += 1
    if outcome == "failed":
        s["failures"] += 1
    else:
        s["lastSuccess"] = int(t0)
    save("status.json", {"generatedAt": int(time.time()), "sources": STATUS})


# For sources whose function has no docstring.
SOURCE_NOTES = {
    "power-infra": "GB electricity network from OpenStreetMap: power lines, substations and plants, refreshed weekly",
    "water-infra": "GB water network from OpenStreetMap: treatment works and water works, refreshed weekly",
    "gas-infra": "GB gas network from OpenStreetMap: transmission pipelines and terminals, refreshed weekly",
    "telecom-infra": "GB telecoms from OpenStreetMap: exchanges, data centres and cables, refreshed weekly",
    "internet-outages": "Internet outages by country and region (IODA, Georgia Tech)",
    "cloudflare-radar": "Internet outages and traffic anomalies (Cloudflare Radar, CC BY-NC 4.0)",
}


def run(name):
    feed, fn, _ = SOURCES[name]
    t0 = time.time()
    until, wait = BACKOFF.get(name, (0, 0))
    if t0 < until:
        return
    try:
        fetched, observations = fn()
        BACKOFF.pop(name, None)
        sent, result = send(feed, observations)
        log_line(name, f"fetched={fetched} sent={sent} {result} ({time.time() - t0:.1f}s)")
        failed = "FAILED" in result
        record(name, "failed" if failed else ("ok" if sent else "nothing new"), result, t0, fetched, sent)
    except RateLimited:
        wait = min(max(wait * 2, 120), 1800)
        BACKOFF[name] = (time.time() + wait, wait)
        log_line(name, f"HTTP 429 rate limited; backing off {wait} s")
        record(name, "rate limited", f"HTTP 429; backing off {wait} s", t0)
    except urllib.error.HTTPError as e:
        log_line(name, f"HTTP {e.code} {e.reason}; skipped this cycle")
        record(name, "failed", f"HTTP {e.code} {e.reason}", t0)
    except Exception as e:  # one source failing must never stop the others
        log_line(name, f"FAILED {type(e).__name__}: {e}")
        record(name, "failed", f"{type(e).__name__}: {e}", t0)


def main(argv):
    if "--once" in argv:
        names = [a for a in argv[argv.index("--once") + 1:] if not a.startswith("-")] or list(SOURCES)
        for n in names:
            if n not in SOURCES:
                sys.exit(f"unknown source {n}; known: {', '.join(SOURCES)}")
        for n in names:
            run(n)
        return
    log_line("runner", f"started pid {os.getpid()}")
    due = {n: 0.0 for n in SOURCES}
    while True:
        for n, (_, _, every) in SOURCES.items():
            if time.time() >= due[n]:
                due[n] = time.time() + every
                run(n)
        time.sleep(1)


if __name__ == "__main__":
    try:
        main(sys.argv[1:])
    except KeyboardInterrupt:
        pass
