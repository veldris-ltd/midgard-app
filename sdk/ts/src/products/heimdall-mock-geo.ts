/* Heimdall mock: a worldwide picture and its history, for the offline 3D demo. [FR-P3-3] [FR-P3-7]
 *
 * Every mock entity moves on a fixed path that is a pure function of time (airliners
 * shuttling between airports, balloons rising from their launch sites, satellites on
 * circular orbits), so `getPicture` and `getHistory` agree with each other without any
 * stored state, and the module stays deterministic: nothing is random at run time (the
 * fleet is drawn from a seeded generator) and the clock is passed in by the caller.
 *
 * The reference layout's five tracks are placed in the UK and Ireland by reading their
 * x/y percentages as positions in that box, so the 2D reference view and the globe show
 * the same entities with the same ids. */
import type {
  HeimdallHistory,
  HeimdallHistoryPoint,
  HeimdallHistoryQuery,
  HeimdallLayerId,
  HeimdallTrack,
  HeimdallTrackDetail,
} from './heimdall';

/** The UK and Ireland (the same box as `UK_AND_IRELAND` in heimdall-http, which this
 *  module cannot import without a cycle). */
const REGION = { west: -11, east: 3, south: 49, north: 61.5 };
const R_KM = 6371;
const MU = 398600.4418; // km³/s², Earth's gravitational parameter
const EARTH_RATE = 7.2921159e-5; // rad/s, sidereal rotation
const FT_PER_M = 3.28084;
const KT_PER_KMS = 1943.84;
const D2R = Math.PI / 180;
const R2D = 180 / Math.PI;

interface MockState {
  lat: number;
  lon: number;
  altM: number | null;
  hdg: number;
  /** Ground speed, knots. */
  spd: number;
  onGround?: boolean;
  /** Kind-specific facts for the inspector at this moment. */
  facts?: { label: string; value: string }[];
}

interface MockEntity {
  id: string;
  kind: string;
  layer: HeimdallLayerId;
  cls: string;
  sources: string[];
  callsign?: string;
  squawk?: string;
  category?: string;
  name?: string;
  label?: string;
  rationale?: string;
  station?: boolean;
  magnitude?: number;
  severityLevel?: number;
  anomalous?: boolean;
  /** When the entity first appeared (unix s), for the inspector. */
  firstSeen: (t: number) => number;
  /** Where the entity is at unix time `t`, or null while it is not in the picture. */
  at: (t: number) => MockState | null;
}

// ---- small geodesy ----------------------------------------------------------

const wrapLon = (lon: number) => ((((lon + 180) % 360) + 360) % 360) - 180;

function bearing(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const p1 = lat1 * D2R, p2 = lat2 * D2R, dl = (lon2 - lon1) * D2R;
  const y = Math.sin(dl) * Math.cos(p2);
  const x = Math.cos(p1) * Math.sin(p2) - Math.sin(p1) * Math.cos(p2) * Math.cos(dl);
  return (Math.atan2(y, x) * R2D + 360) % 360;
}

function distanceKm(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const p1 = lat1 * D2R, p2 = lat2 * D2R;
  const a = Math.sin((p2 - p1) / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(((lon2 - lon1) * D2R) / 2) ** 2;
  return 2 * R_KM * Math.asin(Math.min(1, Math.sqrt(a)));
}

/** The point a fraction `f` of the way along the great circle from 1 to 2. */
function along(lat1: number, lon1: number, lat2: number, lon2: number, f: number): [number, number] {
  const d = distanceKm(lat1, lon1, lat2, lon2) / R_KM;
  if (d < 1e-9) return [lat1, lon1];
  const p1 = lat1 * D2R, l1 = lon1 * D2R, p2 = lat2 * D2R, l2 = lon2 * D2R;
  const a = Math.sin((1 - f) * d) / Math.sin(d);
  const b = Math.sin(f * d) / Math.sin(d);
  const x = a * Math.cos(p1) * Math.cos(l1) + b * Math.cos(p2) * Math.cos(l2);
  const y = a * Math.cos(p1) * Math.sin(l1) + b * Math.cos(p2) * Math.sin(l2);
  const z = a * Math.sin(p1) + b * Math.sin(p2);
  return [Math.atan2(z, Math.hypot(x, y)) * R2D, Math.atan2(y, x) * R2D];
}

/** Move `km` from a point along a bearing. */
function offset(lat: number, lon: number, brgDeg: number, km: number): [number, number] {
  const d = km / R_KM, brg = brgDeg * D2R, p1 = lat * D2R, l1 = lon * D2R;
  const p2 = Math.asin(Math.sin(p1) * Math.cos(d) + Math.cos(p1) * Math.sin(d) * Math.cos(brg));
  const l2 = l1 + Math.atan2(Math.sin(brg) * Math.sin(d) * Math.cos(p1), Math.cos(d) - Math.sin(p1) * Math.sin(p2));
  return [p2 * R2D, wrapLon(l2 * R2D)];
}

/** A seeded generator (mulberry32), so the mock fleet is the same on every load. */
function seeded(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// ---- the fleet --------------------------------------------------------------

const AIRPORTS: [string, number, number][] = [
  ['EGLL', 51.47, -0.45], ['EGKK', 51.15, -0.18], ['EGCC', 53.35, -2.27], ['EGBB', 52.45, -1.75],
  ['EGPH', 55.95, -3.36], ['EGPF', 55.87, -4.43], ['EGAA', 54.66, -6.22], ['EIDW', 53.42, -6.27],
  ['EICK', 51.84, -8.49], ['EGNT', 55.04, -1.69], ['LFPG', 49.01, 2.55], ['EHAM', 52.31, 4.76],
  ['EDDF', 50.03, 8.57], ['EKCH', 55.62, 12.65], ['ENGM', 60.19, 11.1], ['BIKF', 63.98, -22.6],
  ['LEMD', 40.47, -3.56], ['KJFK', 40.64, -73.78], ['CYYZ', 43.68, -79.63], ['OMDB', 25.25, 55.36],
];
const OPERATORS = ['BAW', 'EZY', 'RYR', 'EIN', 'KLM', 'AFR', 'DLH', 'SAS', 'VIR', 'LOG', 'ICE', 'UAE', 'AAL', 'TOM'];

/** Pressure altitude in feet from a reference alt string ("FL340" → 34000). */
const refFeet = (alt: string): number | null => {
  const fl = /FL(\d+)/.exec(alt);
  return fl ? Number(fl[1]) * 100 : null;
};

function airliner(id: string, callsign: string, from: [string, number, number], to: [string, number, number], cruiseFt: number, spdKt: number, phase: number, squawk: string): MockEntity {
  const D = distanceKm(from[1], from[2], to[1], to[2]);
  const kms = spdKt / KT_PER_KMS;
  const leg = D / kms; // seconds per leg
  const climbKm = Math.min(200, D / 3);
  const at = (t: number): MockState => {
    const s = (((t / leg + phase) % 2) + 2) % 2;
    const out = s < 1;
    const f = out ? s : s - 1;
    const [a, b] = out ? [from, to] : [to, from];
    const [lat, lon] = along(a[1], a[2], b[1], b[2], f);
    const toEnd = Math.min(f, 1 - f) * D;
    const ft = Math.round((cruiseFt * Math.min(1, toEnd / climbKm)) / 25) * 25;
    const onGround = ft < 100;
    return {
      lat, lon, altM: onGround ? 0 : ft / FT_PER_M,
      hdg: bearing(lat, lon, b[1], b[2]),
      spd: onGround ? 12 : Math.round(spdKt * (0.55 + 0.45 * Math.min(1, toEnd / climbKm))),
      onGround,
      facts: [{ label: 'ROUTE', value: `${a[0]} → ${b[0]}` }],
    };
  };
  return {
    id, kind: 'air-track', layer: 'air', cls: 'AIRCRAFT', sources: phase > 0.5 ? ['adsb-lol', 'adsb-fi'] : ['adsb-lol'],
    callsign, squawk, category: 'A3',
    label: squawk === '7700' ? 'emergency' : undefined,
    rationale: squawk === '7700' ? 'squawk 7700 (general emergency)' : undefined,
    anomalous: squawk === '7700',
    firstSeen: (t) => t - ((((t / leg + phase) % 1) + 1) % 1) * leg,
    at,
  };
}

/** A radiosonde launched every 12 h from `site`: rises at 5 m/s to burst near 33 km,
 *  falls for 40 minutes, then is gone until the next launch. Drifts with the wind. */
function balloon(id: string, name: string, site: [number, number], launchOffsetH: number, windDeg: number, windMs: number): MockEntity {
  const cycle = 12 * 3600;
  const rise = 33000 / 5;
  const fall = 40 * 60;
  const launchOf = (t: number) => Math.floor((t - launchOffsetH * 3600) / cycle) * cycle + launchOffsetH * 3600;
  return {
    id, kind: 'air-balloon', layer: 'air', cls: 'BALLOON', sources: ['sondehub'], name,
    firstSeen: launchOf,
    at: (t) => {
      const dt = t - launchOf(t);
      if (dt > rise + fall) return null;
      const altM = dt <= rise ? dt * 5 : 33000 * (1 - (dt - rise) / fall);
      const vv = dt <= rise ? 5 : -33000 / fall;
      const [lat, lon] = offset(site[0], site[1], windDeg, (windMs * dt) / 1000);
      return {
        lat, lon, altM, hdg: windDeg, spd: Math.round(windMs * 1.944),
        facts: [{ label: 'TYPE', value: 'RS41-SGP' }, { label: 'ASCENT', value: `${vv.toFixed(1)} m/s` }, { label: 'DRIFT', value: `${windMs} m/s @ ${windDeg}°` }],
      };
    },
  };
}

/** A circular orbit: inclination, altitude (km), right ascension of the ascending node
 *  and argument of latitude at t = 0 (degrees). */
function satellite(id: string, name: string, group: string, incDeg: number, altKm: number, raanDeg: number, u0Deg: number, station = false): MockEntity {
  const a = R_KM + altKm;
  const n = Math.sqrt(MU / a ** 3);
  const i = incDeg * D2R, raan = raanDeg * D2R;
  const sub = (t: number): [number, number] => {
    const u = u0Deg * D2R + n * t;
    const x = Math.cos(raan) * Math.cos(u) - Math.sin(raan) * Math.sin(u) * Math.cos(i);
    const y = Math.sin(raan) * Math.cos(u) + Math.cos(raan) * Math.sin(u) * Math.cos(i);
    const z = Math.sin(u) * Math.sin(i);
    return [Math.asin(z) * R2D, wrapLon((Math.atan2(y, x) - EARTH_RATE * (t % 86164.0905)) * R2D)];
  };
  const kms = Math.sqrt(MU / a);
  return {
    id, kind: 'space-satellite', layer: 'space', cls: station ? 'SPACE STATION' : 'SATELLITE', sources: ['celestrak'], name, station,
    firstSeen: (t) => Math.floor(t / 86400) * 86400,
    at: (t) => {
      const [lat, lon] = sub(t);
      const [la2, lo2] = sub(t + 10);
      return {
        lat, lon, altM: altKm * 1000, hdg: bearing(lat, lon, la2, lo2), spd: Math.round(kms * KT_PER_KMS),
        facts: [{ label: 'GROUP', value: group }, { label: 'ALTITUDE', value: `${Math.round(altKm).toLocaleString()} km` }, { label: 'SPEED', value: `${kms.toFixed(2)} km/s` }],
      };
    },
  };
}

/** An earthquake `hoursAgo` before the start of the current hour (so it holds still for
 *  the hour); absent before it happened. */
function quake(id: string, place: string, lat: number, lon: number, mag: number, depthKm: number, hoursAgo: number): MockEntity {
  const when = (t: number) => Math.floor(t / 3600) * 3600 - hoursAgo * 3600;
  return {
    id, kind: 'event-earthquake', layer: 'events', cls: 'EARTHQUAKE', sources: ['usgs'], name: place, magnitude: mag,
    firstSeen: when,
    at: (t) => (t < when(t) ? null : {
      lat, lon, altM: -depthKm * 1000, hdg: 0, spd: 0,
      facts: [{ label: 'MAGNITUDE', value: `M${mag.toFixed(1)}` }, { label: 'DEPTH', value: `${depthKm} km` }, { label: 'PLACE', value: place }],
    }),
  };
}

function flood(id: string, area: string, river: string, lat: number, lon: number, level: number, hoursAgo: number): MockEntity {
  const raised = (t: number) => Math.floor(t / 3600) * 3600 - hoursAgo * 3600;
  const severity = ['Severe Flood Warning', 'Flood Warning', 'Flood Alert', 'Warning no longer in force'][level - 1];
  return {
    id, kind: 'event-flood', layer: 'events', cls: 'FLOOD WARNING', sources: ['ea-flood'], name: area, severityLevel: level,
    firstSeen: raised,
    at: (t) => (t < raised(t) ? null : {
      lat, lon, altM: 0, hdg: 0, spd: 0,
      facts: [{ label: 'SEVERITY', value: `${level} · ${severity}` }, { label: 'RIVER', value: river }, { label: 'AREA', value: area }],
    }),
  };
}

let FLEET: MockEntity[] | null = null;

function fleet(reference: HeimdallTrack[]): MockEntity[] {
  if (FLEET) return FLEET;
  const out: MockEntity[] = [];
  // The reference layout's tracks, read as positions in the UK box, moving to and fro
  // along their heading.
  for (const r of reference) {
    const lat = REGION.north - (r.y / 100) * (REGION.north - REGION.south);
    const lon = REGION.west + (r.x / 100) * (REGION.east - REGION.west);
    const ft = refFeet(r.alt);
    const kms = Math.max(r.spd, 2) / KT_PER_KMS;
    const span = r.layer === 'air' ? 120 : 6; // km each way
    const period = (4 * span) / kms;
    const kind = r.layer === 'air' ? 'air-track' : r.layer === 'maritime' ? 'sea-vessel' : 'land-unit';
    out.push({
      id: r.id, kind, layer: r.layer, cls: r.cls, sources: Array.from({ length: r.feeds }, (_, k) => ['NORTH-2', 'MAR-RAD-1', 'ADS-B-EU', 'SAT-IR-7'][k % 4]),
      anomalous: r.track === 'anomalous', callsign: r.layer === 'air' ? r.id : undefined,
      firstSeen: (t) => t - 3 * 3600,
      at: (t) => {
        const ph = (((t / period) % 1) + 1) % 1; // triangle wave over -span..span
        const s = ph < 0.5 ? -span + 4 * span * ph : 3 * span - 4 * span * ph;
        const [la, lo] = offset(lat, lon, r.hdg, s);
        return {
          lat: la, lon: lo, altM: ft === null ? 0 : ft / FT_PER_M,
          hdg: ph < 0.5 ? r.hdg : (r.hdg + 180) % 360, spd: r.spd,
        };
      },
    });
  }
  const rnd = seeded(20261017);
  const pick = <T>(xs: T[]): T => xs[Math.floor(rnd() * xs.length)];
  for (let k = 0; k < 90; k++) {
    const a = pick(AIRPORTS);
    let b = pick(AIRPORTS);
    while (b === a) b = pick(AIRPORTS);
    const D = distanceKm(a[1], a[2], b[1], b[2]);
    const cruise = D < 500 ? 22000 + Math.round(rnd() * 8) * 1000 : 33000 + Math.round(rnd() * 8) * 1000;
    const op = pick(OPERATORS);
    const callsign = `${op}${Math.floor(10 + rnd() * 980)}${rnd() < 0.3 ? 'A' : ''}`;
    const icao = (0x400000 + Math.floor(rnd() * 0x3fffff)).toString(16);
    const squawk = k === 7 ? '7700' : String(1000 + Math.floor(rnd() * 6000)).padStart(4, '0');
    out.push(airliner(icao, callsign, a, b, cruise, 430 + Math.round(rnd() * 60), rnd() * 2, squawk));
  }
  out.push(
    // Launch times staggered two hours apart, so one or two are always aloft.
    balloon('sonde-camborne', 'Camborne RS41', [50.22, -5.33], 0, 70, 18),
    balloon('sonde-lerwick', 'Lerwick RS41', [60.14, -1.18], 2, 95, 24),
    balloon('sonde-valentia', 'Valentia RS41', [51.94, -10.24], 4, 60, 15),
    balloon('sonde-albemarle', 'Albemarle RS41', [55.02, -1.88], 6, 80, 20),
    balloon('sonde-herstmonceux', 'Herstmonceux RS41', [50.89, 0.32], 8, 65, 12),
    balloon('sonde-payerne', 'Payerne RS41', [46.81, 6.94], 10, 40, 9),
  );
  out.push(
    satellite('sat-25544', 'ISS (ZARYA)', 'stations', 51.64, 420, 30, 0, true),
    satellite('sat-48274', 'CSS (TIANHE)', 'stations', 41.47, 385, 120, 90, true),
    satellite('sat-43013', 'NOAA 20', 'weather', 98.7, 825, 200, 10),
    satellite('sat-40697', 'SENTINEL-2A', 'resource', 98.6, 786, 260, 200),
    satellite('sat-33591', 'NOAA 19', 'weather', 99.1, 850, 80, 300),
  );
  for (let p = 0; p < 6; p++) {
    for (let s = 0; s < 5; s++) {
      out.push(satellite(`sat-${44000 + p * 10 + s}`, `STARLINK-${1000 + p * 10 + s}`, 'starlink', 53, 550, p * 60, s * 72 + p * 12));
    }
  }
  for (let p = 0; p < 6; p++) {
    out.push(satellite(`sat-gps-${p}`, `GPS BIIF-${p + 1}`, 'gps-ops', 55, 20200, p * 60, p * 40));
  }
  // Geostationary: the orbital rate equals the earth's, so these hold their longitude.
  const geoAltKm = Math.cbrt(MU / EARTH_RATE ** 2) - R_KM;
  for (const [id, name, lon] of [['sat-geo-1', 'ASTRA 2E', 28.2], ['sat-geo-2', 'EUTELSAT 10B', 10], ['sat-geo-3', 'GOES 16', -75.2], ['sat-geo-4', 'METEOSAT 11', 0]] as [string, string, number][]) {
    out.push(satellite(id, name, 'geo', 0, geoAltKm, lon, 0));
  }
  out.push(
    quake('us7000q1', '41 km E of Miyako, Japan', 39.64, 142.45, 5.8, 35, 5),
    quake('us7000q2', 'Off the coast of central Chile', -33.1, -72.3, 6.1, 22, 14),
    quake('us7000q3', 'Reykjanes Peninsula, Iceland', 63.88, -22.3, 3.4, 6, 2),
    quake('us7000q4', 'Western Turkey', 39.1, 28.2, 4.7, 10, 9),
    quake('us7000q5', 'Southern Alaska', 60.2, -152.9, 4.2, 90, 20),
    quake('eq-uk-1', 'Near Llandovery, Wales', 52.0, -3.8, 2.3, 7, 3),
  );
  out.push(
    flood('fw-severn', 'Tewkesbury and the Severn Ham', 'River Severn', 51.99, -2.16, 2, 6),
    flood('fw-ouse', 'York city centre riverside', 'River Ouse', 53.96, -1.08, 3, 10),
    flood('fw-thames', 'Oxford, Osney and Botley', 'River Thames', 51.75, -1.27, 3, 4),
    flood('fw-derwent', 'Cockermouth', 'River Derwent', 54.66, -3.36, 1, 2),
    flood('fw-trent', 'Gainsborough riverside', 'River Trent', 53.4, -0.78, 4, 18),
  );
  FLEET = out;
  return out;
}

// ---- picture + history ------------------------------------------------------

const ftAlt = (st: MockState, kind: string): string => {
  if (st.onGround) return 'GND';
  if (st.altM === null) return '—';
  if (kind === 'space-satellite') return `${Math.round(st.altM / 1000).toLocaleString()} km`;
  if (kind === 'event-earthquake') return `${Math.round(-st.altM / 1000)} km deep`;
  if (kind.startsWith('event-') || kind === 'sea-vessel') return 'SURFACE';
  if (kind === 'land-unit') return 'GND';
  const ft = Math.round(st.altM * FT_PER_M);
  return ft >= 6000 ? `FL${Math.round(ft / 100).toString().padStart(3, '0')}` : `${ft.toLocaleString()} ft`;
};

/** The worldwide picture at unix time `now`. */
export function mockGeoTracks(reference: HeimdallTrack[], now: number): HeimdallTrack[] {
  const out: HeimdallTrack[] = [];
  for (const e of fleet(reference)) {
    const st = e.at(now);
    if (!st) continue;
    const air = e.kind.startsWith('air-');
    const detail: HeimdallTrackDetail = {
      key: e.id,
      kind: e.kind,
      name: e.name,
      callsign: e.callsign,
      squawk: e.squawk,
      category: e.category,
      onGround: !!st.onGround,
      lat: st.lat,
      lon: st.lon,
      altM: st.altM ?? undefined,
      magnitude: e.magnitude,
      severityLevel: e.severityLevel,
      station: e.station,
      facts: st.facts,
      label: e.label,
      rationale: e.rationale,
      sources: e.sources,
      observations: Math.round((now - e.firstSeen(now)) / 5),
      firstSeen: Math.round(e.firstSeen(now)),
      lastSeen: Math.round(now) - (e.id.length % 7),
      estimatedSecs: 0,
    };
    out.push({
      id: e.callsign ?? e.id,
      cls: e.cls,
      conf: e.anomalous ? 0.54 : 0.97,
      x: ((st.lon - REGION.west) / (REGION.east - REGION.west)) * 100,
      y: ((REGION.north - st.lat) / (REGION.north - REGION.south)) * 100,
      lat: st.lat,
      lon: st.lon,
      layer: e.layer,
      track: e.anomalous ? 'anomalous' : 'stable',
      hdg: Math.round(st.hdg),
      spd: st.spd,
      alt: ftAlt(st, e.kind),
      feeds: e.sources.length,
      altFt: air && st.altM !== null ? Math.round(st.altM * FT_PER_M) : undefined,
      detail,
    });
  }
  return out;
}

/** Recorded positions over `q`, sampled every `step` seconds (at least 15 s, the mock's
 *  report interval), on a grid aligned to the step so incremental reads line up. */
export function mockHistory(reference: HeimdallTrack[], q: HeimdallHistoryQuery, now: number): HeimdallHistory {
  const to = Math.min(q.to, now);
  const from = Math.max(q.from, now - 7 * 86400);
  const step = Math.max(15, q.step ?? 0);
  const tracks: HeimdallHistory['tracks'] = [];
  for (const e of fleet(reference)) {
    if (q.layer && e.layer !== q.layer) continue;
    if (q.entity && e.id !== q.entity) continue;
    const points: HeimdallHistoryPoint[] = [];
    const push = (t: number) => {
      const st = e.at(t);
      if (st) points.push([t, +st.lat.toFixed(5), +st.lon.toFixed(5), st.altM === null ? null : Math.round(st.altM)]);
    };
    // Events do not move: their first and latest positions are enough.
    if (e.layer === 'events') {
      const t0 = Math.max(from, e.firstSeen(to));
      if (t0 <= to) { push(t0); if (to > t0) push(to); }
    } else {
      for (let t = Math.ceil(from / step) * step; t < to; t += step) push(t);
      push(to);
    }
    if (points.length) tracks.push({ id: e.id, kind: e.kind, layer: e.layer, points });
  }
  return { from: q.from, to, truncated: false, tracks };
}
