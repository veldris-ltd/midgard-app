/* The tile allow-lists: the mirror's one origin and fixed layout, and the demo build's
 * three public hosts. HTTPS only, exact paths, nothing a URL parser would rewrite.
 * [FR-H-1] [NFR-PRV-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { MAX_TILE_BYTES, mirrorSources, tileSet } from './hosts.ts';
import { publicSources } from './publicHosts.ts';

const PUBLIC = tileSet(publicSources());
const MIRROR = tileSet(mirrorSources('https://tiles.enclosure.internal/heimdall'));
const BARE = tileSet(mirrorSources('https://10.0.0.5:8443'));

// --- public (demo) --------------------------------------------------------------

test('public: the three sources build HTTPS URLs on their own hosts', () => {
  const t = { z: 6, x: 31, y: 19 };
  assert.equal(PUBLIC.url('terrain', t), 'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png');
  // EOX's WMTS orders the path by row (y) and then column (x).
  assert.equal(PUBLIC.url('imagery', t), 'https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/g/6/19/31.jpg');
  assert.equal(PUBLIC.url('vector', t), 'https://tiles.versatiles.org/tiles/osm/6/31/19');
  assert.deepEqual([...PUBLIC.origins].sort(), ['https://s3.amazonaws.com', 'https://tiles.maps.eox.at', 'https://tiles.versatiles.org']);
});

test('public: the imagery is the 2016 Sentinel-2 cloudless layer and no other', () => {
  assert.match(PUBLIC.sources.imagery.pathPrefix, /\/s2cloudless_3857\//);
  assert.equal(PUBLIC.allowed('https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2018_3857/default/g/6/19/31.jpg'), null);
  assert.equal(PUBLIC.allowed('https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2024_3857/default/g/6/19/31.jpg'), null);
  assert.equal(PUBLIC.allowed('https://tiles.maps.eox.at/wmts/1.0.0/terrain-light_3857/default/g/6/19/31.jpg'), null);
});

test('url refuses tiles outside a source\'s zoom range or the tile grid', () => {
  for (const set of [PUBLIC, MIRROR]) {
    assert.throws(() => set.url('vector', { z: 15, x: 0, y: 0 }), /zoom/);
    assert.throws(() => set.url('imagery', { z: 15, x: 0, y: 0 }), /zoom/);
    assert.throws(() => set.url('terrain', { z: 3, x: 8, y: 0 }), /tile/);
    assert.throws(() => set.url('terrain', { z: 3, x: 1.5, y: 0 }), /tile/);
  }
});

test('every URL a set builds is allowed by it, and names its source; never by the other set', () => {
  for (const id of ['terrain', 'imagery', 'vector'] as const) {
    const t = { z: 4, x: 7, y: 5 };
    assert.equal(PUBLIC.allowed(PUBLIC.url(id, t)), id);
    assert.equal(MIRROR.allowed(MIRROR.url(id, t)), id);
    assert.equal(BARE.allowed(BARE.url(id, t)), id);
    assert.equal(MIRROR.allowed(PUBLIC.url(id, t)), null);
    assert.equal(PUBLIC.allowed(MIRROR.url(id, t)), null);
    assert.equal(BARE.allowed(MIRROR.url(id, t)), null);
  }
});

test('public: anything else is refused', () => {
  const refused = [
    'http://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png',
    'https://s3.amazonaws.com/some-other-bucket/terrarium/6/31/19.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/normal/6/31/19.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png?x=1',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png#a',
    'https://s3.amazonaws.com:8443/elevation-tiles-prod/terrarium/6/31/19.png',
    'https://s3.amazonaws.com:443/elevation-tiles-prod/terrarium/6/31/19.png',
    'https://user:pw@s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png',
    'https://s3.amazonaws.com.example.com/elevation-tiles-prod/terrarium/6/31/19.png',
    'https://evil.s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/../../x.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/64/19.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/16/1/1.png',
    'https://tiles.versatiles.org/tiles/osm/6/31/19.pbf',
    'https://tiles.versatiles.org/tiles/osm/15/0/0',
    'https://tiles.versatiles.org/assets/glyphs/x.pbf',
    'https://example.com/tiles/osm/6/31/19',
    'data:image/png;base64,AAAA',
    'blob:https://tiles.versatiles.org/123',
    '/tiles/osm/6/31/19',
    'not a url',
    // Spellings a URL parser normalises to an allowed tile: only the exact form passes.
    '//s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png',
    'https:\\\\s3.amazonaws.com\\elevation-tiles-prod\\terrarium\\6\\31\\19.png',
    'https://s3.amazonaws.com\\elevation-tiles-prod/terrarium/6/31/19.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/%2e%2e/19.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/x/%2e%2e/6/31/19.png',
    'https://S3.AMAZONAWS.COM/elevation-tiles-prod/terrarium/6/31/19.png',
    'https://s3.amazonaws.com./elevation-tiles-prod/terrarium/6/31/19.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/06/31/19.png',
    ' https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png',
  ];
  for (const u of refused) assert.equal(PUBLIC.allowed(u), null, u);
});

// --- mirror -----------------------------------------------------------------------

test('mirror: one origin, the fixed layout, z/x/y throughout', () => {
  const t = { z: 6, x: 31, y: 19 };
  assert.equal(MIRROR.url('terrain', t), 'https://tiles.enclosure.internal/heimdall/terrarium/6/31/19.png');
  assert.equal(MIRROR.url('imagery', t), 'https://tiles.enclosure.internal/heimdall/s2cloudless-2016/6/31/19.jpg');
  assert.equal(MIRROR.url('vector', t), 'https://tiles.enclosure.internal/heimdall/osm/6/31/19.pbf');
  assert.deepEqual([...MIRROR.origins], ['https://tiles.enclosure.internal']);
  assert.equal(BARE.url('vector', t), 'https://10.0.0.5:8443/osm/6/31/19.pbf');
  assert.deepEqual([...BARE.origins], ['https://10.0.0.5:8443']);
  assert.throws(() => mirrorSources('http://tiles.enclosure.internal'), /https/);
});

test('mirror: look-alike origins are refused', () => {
  const path = '/heimdall/terrarium/6/31/19.png';
  const refused = [
    `http://tiles.enclosure.internal${path}`,
    `https://tiles.enclosure.internal.evil.example${path}`,
    `https://evil.tiles.enclosure.internal${path}`,
    `https://tiles-enclosure.internal${path}`,
    `https://tiles.enclosure.interna${path}`,
    `https://tiles.enclosure.internal.${path}`,
    `https://TILES.enclosure.internal${path}`,
    `https://tiles.enclosure.internal:8443${path}`,
    `https://tiles.enclosure.internal:443${path}`,
    `https://user@tiles.enclosure.internal${path}`,
    `https://tiles.enclosure.internal@evil.example${path}`,
    `https://xn--tiles-enclosure.internal${path}`,
    `wss://tiles.enclosure.internal${path}`,
    `//tiles.enclosure.internal${path}`,
  ];
  for (const u of refused) assert.equal(MIRROR.allowed(u), null, u);
  // A mirror on a port: the port is part of the origin, so no other port, nor none.
  assert.equal(BARE.allowed('https://10.0.0.5/osm/6/31/19.pbf'), null);
  assert.equal(BARE.allowed('https://10.0.0.5:8444/osm/6/31/19.pbf'), null);
  assert.equal(BARE.allowed('https://10.0.0.50:8443/osm/6/31/19.pbf'), null);
});

test('mirror: look-alike paths are refused', () => {
  const o = 'https://tiles.enclosure.internal';
  const refused = [
    `${o}/terrarium/6/31/19.png`,
    `${o}/heimdall2/terrarium/6/31/19.png`,
    `${o}/heimdal/terrarium/6/31/19.png`,
    `${o}/x/heimdall/terrarium/6/31/19.png`,
    `${o}/heimdall/heimdall/terrarium/6/31/19.png`,
    `${o}/heimdall/terrarium/6/31/19.jpg`,
    `${o}/heimdall/terrarium/6/31/19.png.gz`,
    `${o}/heimdall/terrarium/6/31/19`,
    `${o}/heimdall/terrarium-v2/6/31/19.png`,
    `${o}/heimdall/s2cloudless-2017/6/31/19.jpg`,
    `${o}/heimdall/s2cloudless/6/31/19.jpg`,
    `${o}/heimdall/s2cloudless-2016/6/31/19.png`,
    `${o}/heimdall/osm/6/31/19`,
    `${o}/heimdall/osm/6/31/19.mvt`,
    `${o}/heimdall/osm/6/31/19.pbf/`,
    `${o}/heimdall/osm/15/0/0.pbf`,
    `${o}/heimdall/osm/6/64/0.pbf`,
    `${o}/heimdall/osm/06/31/19.pbf`,
    `${o}/heimdall/osm/6/31/19.pbf?x=1`,
    `${o}/heimdall/osm/6/31/19.pbf#x`,
    `${o}/heimdall/osm/6/31/19.pbf?`,
    `${o}/heimdall/../heimdall/osm/6/31/19.pbf`,
    `${o}/heimdall/./osm/6/31/19.pbf`,
    `${o}/heimdall/%6fsm/6/31/19.pbf`,
    `${o}/heimdall/osm/6/31/%31%39.pbf`,
    `${o}/heimdall//osm/6/31/19.pbf`,
    `${o}/Heimdall/osm/6/31/19.pbf`,
    `${o}/heimdall/OSM/6/31/19.pbf`,
    `${o}\\heimdall\\osm\\6\\31\\19.pbf`,
    `${o}/heimdall/osm/6/31/19.pbf `,
    `${o}/heimdall/glyphs/x.pbf`,
    `${o}/heimdall/`,
    `${o}/`,
  ];
  for (const u of refused) assert.equal(MIRROR.allowed(u), null, u);
});

test('the byte caps: 1 MB for terrain, 2 MB for imagery and vector tiles', () => {
  assert.deepEqual(MAX_TILE_BYTES, { terrain: 1024 * 1024, imagery: 2 * 1024 * 1024, vector: 2 * 1024 * 1024 });
});

test('the README lists every public tile host a CSP must allow, and the mirror layout', () => {
  const readme = readFileSync(new URL('../../README.md', import.meta.url), 'utf8');
  for (const origin of PUBLIC.origins) assert.ok(readme.includes(origin), origin);
  for (const p of ['/terrarium/{z}/{x}/{y}.png', '/s2cloudless-2016/{z}/{x}/{y}.jpg', '/osm/{z}/{x}/{y}.pbf']) assert.ok(readme.includes(p), p);
});
