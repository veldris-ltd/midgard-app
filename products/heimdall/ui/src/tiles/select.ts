/* Which tiles the globe draws and which it fetches, for one camera. [FR-H-1]
 *
 * A quadtree walk from `minZoom`: a tile is split while its texels would look larger
 * than a screen pixel and it is not at `maxZoom`. A tile outside the ground in view
 * (the camera's footprint, or the horizon) is dropped. A split happens only when all
 * of the visible children are ready, so the picture never has holes; until then the
 * parent is drawn and the children are wanted. The budget caps what is drawn plus
 * what is wanted, so detail is fetched only for the area in view, and only so much
 * of it. Pure. */
import { EARTH_RADIUS_KM, angularDistanceDeg } from '../globe/projection.ts';
import { EQUATOR_KM, type Tile, children, tileBounds, tileXToLon, tileYToLat } from './tileMath.ts';

export interface SelectOptions {
  minZoom: number;
  maxZoom: number;
  /** Most tiles drawn plus wanted. */
  budget: number;
  /** Viewport height in CSS pixels, and width / height. */
  viewportPx: number;
  aspect: number;
  fovDeg: number;
  /** Texels per tile side. */
  tilePx?: number;
  /** Split while a texel would cover more than this many screen pixels. */
  texelPx?: number;
}

export interface Selection {
  /** Ready tiles to draw; none overlaps another. */
  draw: Tile[];
  /** Tiles to fetch, nearest the camera first. */
  want: Tile[];
}

const RAD = Math.PI / 180;

/**
 * The angle at the Earth's centre between the point below the camera and the farthest
 * ground in view (the corner of the screen, or the horizon), in degrees.
 */
export function groundViewAngleDeg(altKm: number, fovDeg: number, aspect: number): number {
  const d = 1 + altKm / EARTH_RADIUS_KM;
  const half = Math.atan(Math.tan((fovDeg * RAD) / 2) * Math.sqrt(1 + aspect * aspect));
  const s = d * Math.sin(half);
  if (s >= 1) return Math.acos(1 / d) / RAD;
  return (Math.asin(s) - half) / RAD;
}

interface Node {
  t: Tile;
  /** Angle from the camera's ground point to the tile's centre, and the tile's radius. */
  centreDeg: number;
  radiusDeg: number;
}

function node(t: Tile, cam: { lat: number; lon: number }): Node {
  const b = tileBounds(t);
  const c = { lat: tileYToLat(t.y + 0.5, t.z), lon: tileXToLon(t.x + 0.5, t.z) };
  const midLon = (b.west + b.east) / 2;
  const edge = [
    { lat: b.north, lon: b.west }, { lat: b.north, lon: b.east },
    { lat: b.south, lon: b.west }, { lat: b.south, lon: b.east },
    { lat: b.north, lon: midLon }, { lat: b.south, lon: midLon },
    { lat: c.lat, lon: b.west }, { lat: c.lat, lon: b.east },
  ];
  const radiusDeg = Math.max(...edge.map((p) => angularDistanceDeg(c, p)));
  return { t, centreDeg: angularDistanceDeg(cam, c), radiusDeg };
}

export function selectGlobeTiles(
  cam: { lat: number; lon: number; altKm: number },
  o: SelectOptions,
  ready: (t: Tile) => boolean,
): Selection {
  const tilePx = o.tilePx ?? 256;
  const texelPx = o.texelPx ?? 1;
  const view = groundViewAngleDeg(cam.altKm, o.fovDeg, o.aspect);
  const d = 1 + cam.altKm / EARTH_RADIUS_KM;
  const kmPerPxPerKm = (2 * Math.tan((o.fovDeg * RAD) / 2)) / Math.max(1, o.viewportPx);

  const visible = (n: Node) => n.centreDeg - n.radiusDeg < view;
  const wantsSplit = (n: Node) => {
    if (n.t.z >= o.maxZoom) return false;
    const a = Math.max(0, n.centreDeg - n.radiusDeg) * RAD;
    const distKm = Math.sqrt(d * d + 1 - 2 * d * Math.cos(a)) * EARTH_RADIUS_KM;
    const lat = Math.min(85, Math.abs(tileYToLat(n.t.y + 0.5, n.t.z)));
    const texelKm = (EQUATOR_KM * Math.cos(lat * RAD)) / 2 ** n.t.z / tilePx;
    return texelKm > texelPx * Math.max(distKm, 1e-3) * kmPerPxPerKm;
  };

  const n0 = 2 ** o.minZoom;
  let queue: Node[] = [];
  for (let y = 0; y < n0; y++) {
    for (let x = 0; x < n0; x++) {
      const n = node({ z: o.minZoom, x, y }, cam);
      if (visible(n)) queue.push(n);
    }
  }
  const draw: Node[] = [];
  const want: Node[] = [];
  let used = queue.length;
  const emit = (n: Node) => (ready(n.t) ? draw : want).push(n);

  while (queue.length) {
    // Coarse and near first, so the budget goes where it shows most.
    queue.sort((a, b) => a.t.z - b.t.z || a.centreDeg - b.centreDeg);
    const next: Node[] = [];
    for (const n of queue) {
      if (!ready(n.t) || !wantsSplit(n)) {
        emit(n);
        continue;
      }
      const kids = children(n.t).map((t) => node(t, cam)).filter(visible);
      if (kids.length === 0) {
        emit(n);
        continue;
      }
      if (kids.every((k) => ready(k.t))) {
        if (used + kids.length - 1 > o.budget) {
          emit(n);
        } else {
          used += kids.length - 1;
          next.push(...kids);
        }
        continue;
      }
      emit(n);
      const missing = kids.filter((k) => !ready(k.t));
      if (used + missing.length <= o.budget) {
        used += missing.length;
        want.push(...missing);
      }
    }
    queue = next;
  }

  want.sort((a, b) => a.centreDeg - b.centreDeg || a.t.z - b.t.z);
  return { draw: draw.map((n) => n.t), want: want.map((n) => n.t) };
}
