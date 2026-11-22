/* What the 2D geographic map draws from one VersaTiles vector tile (OpenStreetMap
 * data in the Shortbread schema): fills, lines and place labels, by class. Pure: it
 * takes the decoded tile (`@mapbox/vector-tile`'s shape) and returns plain arrays in
 * tile units, ready to become canvas paths. [FR-H-1] */

export type FillClass = 'water' | 'green' | 'built' | 'bare' | 'building';
export type LineClass = 'water' | 'motorway' | 'major' | 'minor' | 'local' | 'rail' | 'ferry' | 'country' | 'region';

export const FILL_CLASSES: readonly FillClass[] = ['green', 'built', 'bare', 'water', 'building'];
export const LINE_CLASSES: readonly LineClass[] = ['water', 'region', 'country', 'ferry', 'rail', 'local', 'minor', 'major', 'motorway'];

type Props = Record<string, string | number | boolean | undefined>;

export interface MapLabel {
  text: string;
  x: number;
  y: number;
  /** Lower draws first and wins collisions. */
  rank: number;
  population: number;
}

export interface MapTileData {
  extent: number;
  /** Polygons, as lists of rings, each ring a flat [x0, y0, x1, y1, …]. */
  fills: Record<FillClass, number[][][]>;
  /** Lines, each a flat [x0, y0, x1, y1, …]. */
  lines: Record<LineClass, number[][]>;
  labels: MapLabel[];
}

/** The decoded tile, as `@mapbox/vector-tile` gives it. */
export interface VectorTileLike {
  layers: Record<string, {
    length: number;
    extent: number;
    feature(i: number): { type: number; properties: Props; loadGeometry(): { x: number; y: number }[][] };
  }>;
}

const GREEN = new Set(['forest', 'wood', 'park', 'grass', 'meadow', 'heath', 'scrub', 'nature_reserve', 'golf_course', 'village_green', 'garden', 'recreation_ground', 'allotments', 'cemetery', 'grave_yard', 'wetland', 'marsh', 'bog', 'swamp']);
const BUILT = new Set(['residential', 'commercial', 'industrial', 'retail', 'railway', 'brownfield', 'garages', 'military', 'quarry', 'landfill']);
const BARE = new Set(['bare_rock', 'scree', 'shingle', 'sand', 'beach', 'glacier', 'mud']);

export function classifyFill(layer: string, p: Props): FillClass | null {
  if (layer === 'ocean' || layer === 'water_polygons' || layer === 'dam_polygons') return 'water';
  if (layer === 'buildings') return 'building';
  if (layer === 'land' || layer === 'sites') {
    const k = String(p.kind ?? '');
    if (GREEN.has(k)) return 'green';
    if (BUILT.has(k)) return 'built';
    if (BARE.has(k)) return 'bare';
  }
  return null;
}

/** The line class, or null when the feature is not drawn at zoom `z`. */
export function classifyLine(layer: string, p: Props, z: number): LineClass | null {
  const k = String(p.kind ?? '');
  if (layer === 'water_lines') return k === 'river' || k === 'canal' || z >= 12 ? 'water' : null;
  if (layer === 'ferries') return 'ferry';
  if (layer === 'boundaries') {
    if (p.maritime === true || p.maritime === 'true') return null;
    const level = Number(p.admin_level);
    if (level === 2) return 'country';
    if (level === 4) return 'region';
    return null;
  }
  if (layer === 'streets') {
    if (k === 'rail' || p.rail === true) return k === 'rail' ? 'rail' : null;
    if (k === 'motorway') return 'motorway';
    if (k === 'trunk' || k === 'primary') return 'major';
    if (k === 'secondary' || k === 'tertiary') return z >= 9 ? 'minor' : null;
    if (k === 'residential' || k === 'unclassified' || k === 'living_street' || k === 'service' || k === 'pedestrian') return z >= 12 ? 'local' : null;
  }
  return null;
}

/** A place's label rank at zoom `z`, or null when it is not labelled there. */
export function placeRank(kind: string, z: number): number | null {
  if (kind === 'capital' || kind === 'state_capital') return 0;
  if (kind === 'city') return 0;
  if (kind === 'town') return 1;
  if (kind === 'village') return z >= 11 ? 2 : null;
  if (kind === 'suburb' || kind === 'hamlet') return z >= 13 ? 3 : null;
  return null;
}

const flat = (pts: { x: number; y: number }[]) => {
  const out = new Array<number>(pts.length * 2);
  pts.forEach((p, i) => { out[i * 2] = p.x; out[i * 2 + 1] = p.y; });
  return out;
};

export function extractFeatures(tile: VectorTileLike, z: number): MapTileData {
  const fills = Object.fromEntries(FILL_CLASSES.map((c) => [c, [] as number[][][]])) as Record<FillClass, number[][][]>;
  const lines = Object.fromEntries(LINE_CLASSES.map((c) => [c, [] as number[][]])) as Record<LineClass, number[][]>;
  const labels: MapLabel[] = [];
  let extent = 4096;
  for (const [name, layer] of Object.entries(tile.layers)) {
    extent = layer.extent || extent;
    for (let i = 0; i < layer.length; i++) {
      const f = layer.feature(i);
      if (f.type === 3) {
        const c = classifyFill(name, f.properties);
        if (c) fills[c].push(f.loadGeometry().map(flat));
      } else if (f.type === 2) {
        const c = classifyLine(name, f.properties, z);
        if (c) for (const l of f.loadGeometry()) lines[c].push(flat(l));
      } else if (f.type === 1 && name === 'place_labels') {
        const rank = placeRank(String(f.properties.kind ?? ''), z);
        const text = String(f.properties.name_en ?? f.properties.name ?? '');
        if (rank === null || !text) continue;
        const pt = f.loadGeometry()[0]?.[0];
        if (!pt || pt.x < 0 || pt.y < 0 || pt.x >= layer.extent || pt.y >= layer.extent) continue;
        labels.push({ text, x: pt.x, y: pt.y, rank, population: Number(f.properties.population) || 0 });
      }
    }
  }
  labels.sort((a, b) => a.rank - b.rank || b.population - a.population);
  return { extent, fills, lines, labels };
}
