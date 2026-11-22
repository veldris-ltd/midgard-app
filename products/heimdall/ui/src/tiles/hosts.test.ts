/* The tile host allow-list: three sources, HTTPS only, exact paths. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { TILE_HOSTS, TILE_SOURCES, allowedTileUrl, tileUrl } from './hosts.ts';

test('the three sources build HTTPS URLs on their own hosts', () => {
  const t = { z: 6, x: 31, y: 19 };
  assert.equal(tileUrl('terrain', t), 'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png');
  // EOX's WMTS orders the path by row (y) and then column (x).
  assert.equal(tileUrl('imagery', t), 'https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/g/6/19/31.jpg');
  assert.equal(tileUrl('vector', t), 'https://tiles.versatiles.org/tiles/osm/6/31/19');
  assert.deepEqual([...TILE_HOSTS].sort(), ['s3.amazonaws.com', 'tiles.maps.eox.at', 'tiles.versatiles.org']);
});

test('the imagery is the 2016 Sentinel-2 cloudless layer and no other', () => {
  assert.match(TILE_SOURCES.imagery.pathPrefix, /\/s2cloudless_3857\//);
  assert.equal(allowedTileUrl('https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2018_3857/default/g/6/19/31.jpg'), null);
  assert.equal(allowedTileUrl('https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2024_3857/default/g/6/19/31.jpg'), null);
  assert.equal(allowedTileUrl('https://tiles.maps.eox.at/wmts/1.0.0/terrain-light_3857/default/g/6/19/31.jpg'), null);
});

test('tileUrl refuses tiles outside a source\'s zoom range or the tile grid', () => {
  assert.throws(() => tileUrl('vector', { z: 15, x: 0, y: 0 }), /zoom/);
  assert.throws(() => tileUrl('imagery', { z: 15, x: 0, y: 0 }), /zoom/);
  assert.throws(() => tileUrl('terrain', { z: 3, x: 8, y: 0 }), /tile/);
  assert.throws(() => tileUrl('terrain', { z: 3, x: 1.5, y: 0 }), /tile/);
});

test('every URL the sources build is allowed, and names its source', () => {
  for (const id of ['terrain', 'imagery', 'vector'] as const) {
    assert.equal(allowedTileUrl(tileUrl(id, { z: 4, x: 7, y: 5 })), id);
  }
});

test('anything else is refused', () => {
  const refused = [
    'http://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png',
    'https://s3.amazonaws.com/some-other-bucket/terrarium/6/31/19.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/normal/6/31/19.png',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png?x=1',
    'https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png#a',
    'https://s3.amazonaws.com:8443/elevation-tiles-prod/terrarium/6/31/19.png',
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
  for (const u of refused) assert.equal(allowedTileUrl(u), null, u);
});

test('the README lists every tile host a CSP must allow', async () => {
  const { readFileSync } = await import('node:fs');
  const readme = readFileSync(new URL('../../README.md', import.meta.url), 'utf8');
  for (const host of TILE_HOSTS) assert.ok(readme.includes(`https://${host}`), host);
});
