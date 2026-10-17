/* An aircraft's flights, from Heimdall's recorded history. [FR-H-1]
 *
 * Heimdall keeps every reported position of a track (its history, up to the retention
 * window), but not "flights". The console splits the positions into flights itself: a
 * flight is a run of airborne positions, ended by a position on the ground or by a gap
 * of more than FLIGHT_GAP with no report. Where a flight starts low or ends on the
 * ground, the nearest known airport within AIRPORT_RADIUS_KM is taken as its departure
 * or arrival; otherwise it is shown as first seen or last seen en route. These are
 * inferences from positions, and the sidebar says so. */
import * as React from 'react';
import type { HeimdallGateway, HeimdallHistoryPoint } from '@midgard/sdk';

/** Seconds without a report after which a flight is taken to have ended. */
const FLIGHT_GAP = 20 * 60;
/** Below this height (metres) a position counts as on the ground. */
const GROUND_M = 30;
/** Below this height (metres) a flight's first or last position is near an airfield. */
const LOW_M = 1500;
const AIRPORT_RADIUS_KM = 25;
/** Flights shorter than this (seconds) are noise, not flights. */
const MIN_FLIGHT = 3 * 60;
/** How far back the sidebar reads: Heimdall's default retention. */
const LOOKBACK = 24 * 3600;

export interface KnownAirport {
  icao: string;
  name: string;
  lat: number;
  lon: number;
}

export interface Flight {
  points: HeimdallHistoryPoint[];
  start: number;
  end: number;
  /** Departure airport, or null when the flight was first seen en route. */
  from: KnownAirport | null;
  /** Arrival airport, or null when it was last seen en route or is still flying. */
  to: KnownAirport | null;
  /** Still airborne and reporting. */
  current: boolean;
  /** Ended by a report on the ground (a landing), not by losing contact. */
  landed: boolean;
  distanceNm: number;
  maxAltM: number | null;
}

const D2R = Math.PI / 180;
function km(lat1: number, lon1: number, lat2: number, lon2: number): number {
  const p1 = lat1 * D2R, p2 = lat2 * D2R, dl = (lon2 - lon1) * D2R;
  const h = Math.sin((p2 - p1) / 2) ** 2 + Math.cos(p1) * Math.cos(p2) * Math.sin(dl / 2) ** 2;
  return 2 * 6371 * Math.asin(Math.min(1, Math.sqrt(h)));
}

function nearest(airports: KnownAirport[], lat: number, lon: number): KnownAirport | null {
  let best: KnownAirport | null = null, bestKm = AIRPORT_RADIUS_KM;
  for (const a of airports) {
    const d = km(lat, lon, a.lat, a.lon);
    if (d < bestKm) { best = a; bestKm = d; }
  }
  return best;
}

const airborne = (p: HeimdallHistoryPoint) => p[3] !== null && p[3] > GROUND_M;

/** Split a track's recorded positions into flights, newest first. */
export function splitFlights(points: HeimdallHistoryPoint[], airports: KnownAirport[], now = Date.now() / 1000): Flight[] {
  const flights: Flight[] = [];
  let run: HeimdallHistoryPoint[] = [];
  let before: HeimdallHistoryPoint | null = null; // the ground position just before the run
  const close = (after: HeimdallHistoryPoint | null) => {
    if (run.length >= 3 && run[run.length - 1][0] - run[0][0] >= MIN_FLIGHT) {
      const first = run[0], last = run[run.length - 1];
      let nm = 0, maxAlt: number | null = null;
      for (let i = 0; i < run.length; i++) {
        if (i) nm += km(run[i - 1][1], run[i - 1][2], run[i][1], run[i][2]) / 1.852;
        if (run[i][3] !== null) maxAlt = Math.max(maxAlt ?? 0, run[i][3]!);
      }
      const dep = before ?? (first[3] !== null && first[3] < LOW_M ? first : null);
      const current = !after && now - last[0] < 5 * 60;
      const arr = after ?? (!current && last[3] !== null && last[3] < LOW_M ? last : null);
      flights.push({
        points: [...(before ? [before] : []), ...run, ...(after ? [after] : [])],
        start: first[0],
        end: last[0],
        from: dep ? nearest(airports, dep[1], dep[2]) : null,
        to: arr ? nearest(airports, arr[1], arr[2]) : null,
        current,
        landed: !!after,
        distanceNm: nm,
        maxAltM: maxAlt,
      });
    }
    run = [];
  };
  for (const p of points) {
    if (airborne(p)) {
      if (run.length && p[0] - run[run.length - 1][0] > FLIGHT_GAP) {
        close(null);
        before = null;
      }
      run.push(p);
    } else {
      if (run.length) close(p);
      before = p;
    }
  }
  if (run.length) close(null);
  return flights.reverse();
}

/** An aircraft's flights over Heimdall's whole retention window, read when it is
 *  selected and again every minute while it stays selected. */
export function useFlights(gateway: HeimdallGateway, key: string | null, airports: KnownAirport[]): { flights: Flight[] | null; since: number | null } {
  const [state, setState] = React.useState<{ key: string; points: HeimdallHistoryPoint[] } | null>(null);
  const [tick, setTick] = React.useState(0);
  React.useEffect(() => {
    if (!key) return;
    const timer = window.setInterval(() => setTick((n) => n + 1), 60_000);
    return () => window.clearInterval(timer);
  }, [key]);
  React.useEffect(() => {
    if (!key) return;
    let live = true;
    const now = Date.now() / 1000;
    // A minute inside the retention, so the window is never refused as too long.
    gateway.getHistory({ from: now - LOOKBACK + 60, to: now, entity: key, step: 15 }).then(
      (h) => { if (live) setState({ key, points: h.tracks.find((t) => t.id === key)?.points ?? [] }); },
      () => { if (live) setState(null); },
    );
    return () => { live = false; };
  }, [gateway, key, tick]);
  const flights = React.useMemo(
    () => (state && state.key === key ? splitFlights(state.points, airports) : null),
    [state, key, airports],
  );
  return { flights, since: state && state.key === key && state.points.length ? state.points[0][0] : null };
}
