/* The rain radar recorded for the offline build (the Mac app): the manifest
 * tools/mac-app/capture-radar.py writes to <recording>/radar/manifest.json, served by the
 * app at /radar/, and which recorded frame to show when. Pure (no DOM, no build
 * constants), so it is unit tested (test/radarReplay.test.ts).
 *
 * Time shift: the newest recorded frame is "now" when the radar is first read (the
 * anchor); from then the frames advance one step at a time as live frames would (every
 * 10 minutes) and loop: after the newest comes the oldest again, labelled with its shifted
 * time, so the clock on the chip keeps moving forward. */

export interface RadarBox { west: number; east: number; south: number; north: number }

export interface RecordedFrame {
  /** Seconds after the first recorded frame. */
  offset: number;
  /** Folder of its tiles under /radar/: frames/<n>. */
  path: string;
  /** Tiles that could not be captured, [x, y]. */
  missing: [number, number][];
}

export interface RadarManifest {
  version: 1;
  attribution: string;
  link: string;
  step: number;
  zoom: number;
  tileSize: number;
  box: RadarBox;
  tiles: { x0: number; x1: number; y0: number; y1: number };
  frames: RecordedFrame[];
}

const isInt = (v: unknown): v is number => typeof v === 'number' && Number.isInteger(v);

/** Checks a fetched manifest against what the overlay draws; throws with the reason. */
export function parseManifest(raw: unknown, want: { zoom: number; tileSize: number; box: RadarBox }): RadarManifest {
  const m = raw as Partial<RadarManifest> | null;
  if (!m || typeof m !== 'object' || m.version !== 1) throw new Error('radar manifest: unknown version');
  if (m.zoom !== want.zoom || m.tileSize !== want.tileSize) throw new Error('radar manifest: other zoom or tile size');
  const b = m.box;
  if (!b || b.west !== want.box.west || b.east !== want.box.east || b.south !== want.box.south || b.north !== want.box.north) {
    throw new Error('radar manifest: other box');
  }
  const t = m.tiles;
  if (!t || ![t.x0, t.x1, t.y0, t.y1].every(isInt) || t.x1 < t.x0 || t.y1 < t.y0) throw new Error('radar manifest: bad tile range');
  if (!isInt(m.step) || m.step <= 0) throw new Error('radar manifest: bad step');
  const frames = m.frames;
  if (!Array.isArray(frames) || frames.length === 0) throw new Error('radar manifest: no frames');
  let last = -1;
  for (const f of frames) {
    if (!f || !isInt(f.offset) || f.offset <= last) throw new Error('radar manifest: frame offsets not increasing');
    if (typeof f.path !== 'string' || !/^frames\/\d+$/.test(f.path)) throw new Error('radar manifest: bad frame path');
    if (!Array.isArray(f.missing) || !f.missing.every((p) => Array.isArray(p) && p.length === 2 && p.every(isInt))) {
      throw new Error('radar manifest: bad missing list');
    }
    last = f.offset;
  }
  return {
    version: 1,
    attribution: typeof m.attribution === 'string' ? m.attribution : 'Weather data by RainViewer',
    link: typeof m.link === 'string' ? m.link : '',
    step: m.step, zoom: m.zoom, tileSize: m.tileSize, box: b, tiles: t, frames,
  };
}

/** The recorded frame to show at `now` (unix s), the radar having been first read at
 * `anchor` (unix s), with the time to label it with. */
export function frameAt(m: Pick<RadarManifest, 'frames' | 'step'>, anchor: number, now: number): { frame: RecordedFrame; index: number; time: number } {
  const newest = m.frames[m.frames.length - 1].offset;
  const period = newest + m.step;
  const elapsed = Math.max(0, now - anchor);
  const pos = (newest + elapsed) % period;
  let index = 0;
  for (let i = 0; i < m.frames.length; i++) if (m.frames[i].offset <= pos) index = i;
  const frame = m.frames[index];
  return { frame, index, time: Math.round(now - (pos - frame.offset)) };
}

/** The tile's URL under the app's /radar/ (relative to the site root). */
export function recordedTileUrl(frame: RecordedFrame, x: number, y: number): string {
  return `/radar/${frame.path}/${x}/${y}.png`;
}
