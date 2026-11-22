/* From a decoded VersaTiles (Shortbread) vector tile to what the map draws: fills,
 * lines and place labels by class. On the recorded tile. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { VectorTile } from '@mapbox/vector-tile';
import { PbfReader } from 'pbf';

import { classifyFill, classifyLine, extractFeatures, placeRank } from './features.ts';

const tile = () => new VectorTile(new PbfReader(readFileSync(new URL('../../fixtures/tiles/versatiles-osm-8-125-78.mvt', import.meta.url))));

test('fills: water, green and built-up land, bare ground and buildings', () => {
  assert.equal(classifyFill('ocean', {}), 'water');
  assert.equal(classifyFill('water_polygons', { kind: 'river' }), 'water');
  assert.equal(classifyFill('land', { kind: 'forest' }), 'green');
  assert.equal(classifyFill('land', { kind: 'park' }), 'green');
  assert.equal(classifyFill('land', { kind: 'residential' }), 'built');
  assert.equal(classifyFill('land', { kind: 'bare_rock' }), 'bare');
  assert.equal(classifyFill('land', { kind: 'farmland' }), null);
  assert.equal(classifyFill('buildings', {}), 'building');
  assert.equal(classifyFill('pois', { kind: 'cafe' }), null);
});

test('lines: roads by importance and only close enough in, rail, water, ferries and boundaries', () => {
  assert.equal(classifyLine('streets', { kind: 'motorway' }, 6), 'motorway');
  assert.equal(classifyLine('streets', { kind: 'trunk', link: true }, 8), 'major');
  assert.equal(classifyLine('streets', { kind: 'primary' }, 8), 'major');
  assert.equal(classifyLine('streets', { kind: 'secondary' }, 8), null);
  assert.equal(classifyLine('streets', { kind: 'secondary' }, 10), 'minor');
  assert.equal(classifyLine('streets', { kind: 'residential' }, 11), null);
  assert.equal(classifyLine('streets', { kind: 'residential' }, 13), 'local');
  assert.equal(classifyLine('streets', { kind: 'rail', rail: true }, 8), 'rail');
  assert.equal(classifyLine('water_lines', { kind: 'river' }, 10), 'water');
  assert.equal(classifyLine('ferries', { kind: 'ferry' }, 10), 'ferry');
  assert.equal(classifyLine('boundaries', { admin_level: 2 }, 5), 'country');
  assert.equal(classifyLine('boundaries', { admin_level: 4 }, 5), 'region');
  assert.equal(classifyLine('boundaries', { admin_level: 2, maritime: true }, 5), null);
  assert.equal(classifyLine('boundaries', { admin_level: 8 }, 12), null);
});

test('place ranks: cities before towns before villages; others not labelled', () => {
  assert.ok(placeRank('city', 8)! < placeRank('town', 8)!);
  assert.ok(placeRank('town', 12)! < placeRank('village', 12)!);
  assert.equal(placeRank('village', 8), null);
  assert.equal(placeRank('locality', 12), null);
});

test('the recorded tile over Tayside: rivers and lochs, roads, rail and towns', () => {
  const d = extractFeatures(tile(), 8);
  assert.equal(d.extent, 4096);
  assert.ok(d.fills.water.length >= 80, `water ${d.fills.water.length}`);
  assert.equal(d.fills.green.length, 1);
  // Three trunk and primary roads and one railway, as multi-part lines.
  assert.ok(d.lines.major.length >= 3, `major ${d.lines.major.length}`);
  assert.ok(d.lines.rail.length >= 1);
  assert.equal(d.lines.minor.length, 0);
  // Rings and lines are flat [x0, y0, x1, y1, …] in tile units.
  const ring = d.fills.water[0][0];
  assert.ok(ring.length >= 6 && ring.length % 2 === 0);
  // Labels lie inside the tile (its buffer belongs to the neighbours), most important first.
  assert.ok(d.labels.every((l) => l.x >= 0 && l.x < 4096 && l.y >= 0 && l.y < 4096));
  // Dundee and Perth sit in the buffer, south of this tile: the tile below labels them.
  assert.deepEqual(d.labels.slice(0, 2).map((l) => l.text), ['Forfar', 'Blairgowrie and Rattray']);
  assert.ok(!d.labels.some((l) => ['Dundee', 'Perth', 'Inverness', 'Montrose'].includes(l.text)));
  assert.ok(d.labels.some((l) => l.text === 'Pitlochry'));
});
