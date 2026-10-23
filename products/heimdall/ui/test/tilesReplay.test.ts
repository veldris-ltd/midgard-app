// Unit tests for the offline close-up tiles' manifest, clipping and tile choice (`node --test`, types stripped).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { clipToRecorded, parseTilesManifest, pickRecorded, recordedTileUrl } from '../src/app/globe/tilesReplay.ts';

const box = { west: -12, east: 4, south: 48, north: 62 };
const level = (z: number, r: [number, number, number, number], tiles: [number, number][], missing: [number, number][] = []) =>
  ({ z, x0: r[0], x1: r[1], y0: r[2], y1: r[3], tiles, missing });
const terrain = () => [
  level(7, [59, 65, 35, 44], [[63, 42], [62, 41]]),
  level(9, [238, 261, 142, 177], [[255, 170], [255, 171], [240, 150]], [[254, 170]]),
  level(10, [477, 523, 285, 355], [[511, 340]]),
];
const manifest = (over: Record<string, unknown> = {}) => ({
  version: 1, captured: 1792767141, box: { ...box }, regions: ['uk'],
  sources: {
    terrarium: { attribution: 'Mapzen Terrain Tiles', link: 'x', licence: 'y', levels: terrain() },
    s2cloudless: { attribution: 'EOxCloudless https://cloudless.eox.at by EOX IT Services GmbH (Contains modified Copernicus Sentinel data 2016)', link: 'x', licence: 'CC BY 4.0', levels: [level(8, [118, 130, 71, 88], [[127, 85]])] },
  },
  ...over,
});
// London, about 20 km across (the globe's closest view).
const london = { west: -0.3, east: 0.05, south: 51.4, north: 51.6 };

test('a captured manifest parses', () => {
  const m = parseTilesManifest(manifest());
  assert.deepEqual(m.box, box);
  assert.equal(m.sources.terrarium.levels.length, 3);
  assert.match(m.sources.s2cloudless.attribution, /^EOxCloudless/);
});

test('a bad manifest is refused', () => {
  for (const bad of [
    null, 'x', { ...manifest(), version: 2 }, manifest({ box: { west: 4, east: -12, south: 48, north: 62 } }),
    manifest({ box: { west: 'a', east: 4, south: 48, north: 62 } }),
    manifest({ sources: {} }),
    manifest({ sources: { ...manifest().sources, terrarium: { levels: [] } } }),
    manifest({ sources: { ...manifest().sources, terrarium: { levels: [level(9, [0, 1, 0, 1], []), level(7, [0, 1, 0, 1], [])] } } }),
    manifest({ sources: { ...manifest().sources, terrarium: { levels: [level(7, [0, 1, 0, 1], [[1.5, 2]])] } } }),
    manifest({ sources: { ...manifest().sources, terrarium: { levels: [level(7, [3, 1, 0, 1], [])] } } }),
  ]) {
    assert.throws(() => parseTilesManifest(bad));
  }
});

test('the close-up is clipped to the recorded box', () => {
  assert.deepEqual(clipToRecorded(london, box), london);
  assert.deepEqual(clipToRecorded({ west: 2, east: 6, south: 50, north: 52 }, box), { west: 2, east: 4, south: 50, north: 52 });
  assert.equal(clipToRecorded({ west: 10, east: 12, south: 50, north: 52 }, box), null); // Germany: the bundled map
  assert.equal(clipToRecorded({ west: 3.999, east: 8, south: 50, north: 52 }, box), null); // a sliver
});

test('the finest recorded zoom at or below the one wanted', () => {
  const src = { levels: terrain() };
  const p = pickRecorded(src, london, 12, 100)!;
  assert.equal(p.z, 10);
  assert.deepEqual([p.x0, p.x1, p.y0, p.y1], [511, 512, 340, 340]);
  assert.equal(p.stored(511, 340), true);
  assert.equal(p.stored(512, 340), false); // in range, not stored: sea
  assert.equal(pickRecorded(src, london, 9, 100)!.z, 9);
  assert.equal(pickRecorded(src, london, 6, 100), null);
});

test('a level with a missing tile in the box, or too many tiles, is passed over', () => {
  const src = { levels: terrain() };
  // Around tile 9/254/170 (missing) at zoom 9: the zoom 7 level is used instead.
  const west = { west: -1.2, east: -0.75, south: 51.6, north: 51.9 };
  assert.equal(pickRecorded({ levels: src.levels.slice(0, 2) }, west, 9, 100)!.z, 7);
  // The whole box at zoom 10 is ~3,300 tiles.
  assert.equal(pickRecorded(src, box, 10, 100)!.z, 7);
});

test('recorded tile URLs keep the sources\' path shapes, under /tiles/', () => {
  assert.equal(recordedTileUrl('terrarium', 10, 511, 340), '/tiles/terrarium/10/511/340.png');
  assert.equal(recordedTileUrl('s2cloudless', 11, 1023, 681), '/tiles/s2cloudless/11/681/1023.jpg');
});
