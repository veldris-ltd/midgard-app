#!/usr/bin/env node
// Builds the Heimdall globe's bundled outlines from Natural Earth. [FR-H-1]
//
//   node tools/heimdall-outlines/build.mjs              # downloads the four files
//   node tools/heimdall-outlines/build.mjs --from DIR   # uses files already in DIR
//
// Natural Earth (naturalearthdata.com) is public domain. The GeoJSON comes from the
// project's own repository at the commit of a fixed tag, and each file must match the SHA-256
// pinned below, or nothing is written. The output is deterministic: the same inputs
// always give byte-identical files, so a rerun on a clean tree shows no diff.
//
// Output, in products/heimdall/ui/src/globe/data/:
//   ne-110m.json  world view: coastlines and land borders at 1:110m
//   ne-50m.json   close in: land polygons (the fill, and the coastline from their
//                 rings) and land borders at 1:50m
//
// Coordinates are quantised to 0.01 degrees (about 1 km) and delta-encoded per ring
// or line: [lon0, lat0, dlon1, dlat1, ...] in hundredths of a degree.
import { createHash } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const VERSION = 'v5.1.2';
const COMMIT = 'f1890d9f152c896d250a77557a5751a93d494776';
// Fetched by the commit the tag names, so a moved tag cannot change what is read.
const BASE = `https://raw.githubusercontent.com/nvkelso/natural-earth-vector/${COMMIT}/geojson`;
const SOURCES = {
  ne_110m_coastline: '851f581ff5ffb844deed8ae1a9ce22e3c4bb3d74fa342cadb5d8e39b41ae7c3c',
  ne_110m_admin_0_boundary_lines_land: 'd42479fd79552cca4eec7f85fcdca717a790d29ff06be7676f1af0568c6d3f7c',
  ne_50m_land: 'e874b27a51d146452be360cafb3cc50c86001074a67d534113e6534682f9826b',
  ne_50m_admin_0_boundary_lines_land: '2faac4f6b34386f3d21b6e018cf151f241f00e5c936d44dd17d7d9bfb147fa48',
};
const QUANTUM = 0.01;

const here = dirname(fileURLToPath(import.meta.url));
const outDir = resolve(here, '../../products/heimdall/ui/src/globe/data');

async function inputs(from) {
  const files = {};
  for (const [name, sha] of Object.entries(SOURCES)) {
    const path = join(from, `${name}.geojson`);
    const bytes = readFileSync(path);
    const got = createHash('sha256').update(bytes).digest('hex');
    if (got !== sha) throw new Error(`${name}.geojson: sha256 ${got}, expected ${sha}`);
    files[name] = JSON.parse(bytes.toString('utf8'));
  }
  return files;
}

async function download(dir) {
  for (const name of Object.keys(SOURCES)) {
    const res = await fetch(`${BASE}/${name}.geojson`);
    if (!res.ok) throw new Error(`${name}: HTTP ${res.status}`);
    writeFileSync(join(dir, `${name}.geojson`), Buffer.from(await res.arrayBuffer()));
  }
}

/** One ring or line, quantised, consecutive duplicates dropped, delta-encoded. */
function encode(coords) {
  const out = [];
  let px = null;
  let py = null;
  for (const [lon, lat] of coords) {
    const x = Math.round(lon / QUANTUM);
    const y = Math.round(lat / QUANTUM);
    if (x === px && y === py) continue;
    if (px === null) out.push(x, y);
    else out.push(x - px, y - py);
    px = x;
    py = y;
  }
  return out.length >= 4 ? out : null;
}

function lines(fc) {
  const out = [];
  for (const f of fc.features) {
    const g = f.geometry;
    const parts = g.type === 'LineString' ? [g.coordinates] : g.type === 'MultiLineString' ? g.coordinates : [];
    for (const p of parts) {
      const e = encode(p);
      if (e) out.push(e);
    }
  }
  return out;
}

function polygons(fc) {
  const out = [];
  for (const f of fc.features) {
    const g = f.geometry;
    const polys = g.type === 'Polygon' ? [g.coordinates] : g.type === 'MultiPolygon' ? g.coordinates : [];
    for (const poly of polys) {
      const rings = poly.map(encode).filter(Boolean);
      if (rings.length) out.push(rings);
    }
  }
  return out;
}

function source(names) {
  return {
    name: 'Natural Earth',
    licence: 'Public domain',
    url: 'https://www.naturalearthdata.com/',
    version: VERSION,
    commit: COMMIT,
    files: names.map((n) => ({ file: `geojson/${n}.geojson`, sha256: SOURCES[n] })),
  };
}

async function main() {
  const args = process.argv.slice(2);
  const i = args.indexOf('--from');
  let from = null;
  if (i >= 0) {
    from = args[i + 1];
    if (!from || from.startsWith('--')) throw new Error('--from needs a directory');
  }
  let tmp = null;
  if (!from) {
    tmp = mkdtempSync(join(tmpdir(), 'ne-'));
    from = tmp;
    await download(tmp);
  }
  try {
    const ne = await inputs(from);
    const header = { format: 'midgard-heimdall-outlines/1', quantum: QUANTUM };
    const world = {
      ...header,
      scale: '1:110m',
      source: source(['ne_110m_coastline', 'ne_110m_admin_0_boundary_lines_land']),
      coast: lines(ne.ne_110m_coastline),
      borders: lines(ne.ne_110m_admin_0_boundary_lines_land),
    };
    const close = {
      ...header,
      scale: '1:50m',
      source: source(['ne_50m_land', 'ne_50m_admin_0_boundary_lines_land']),
      land: polygons(ne.ne_50m_land),
      borders: lines(ne.ne_50m_admin_0_boundary_lines_land),
    };
    for (const [file, data] of [['ne-110m.json', world], ['ne-50m.json', close]]) {
      const path = join(outDir, file);
      writeFileSync(path, JSON.stringify(data) + '\n');
      const sha = createHash('sha256').update(readFileSync(path)).digest('hex');
      console.log(`${file}  ${sha}`);
    }
  } finally {
    if (tmp) rmSync(tmp, { recursive: true, force: true });
  }
}

main().catch((e) => {
  console.error(`heimdall-outlines: ${e.message}`);
  process.exit(1);
});
