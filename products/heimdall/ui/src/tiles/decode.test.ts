/* Decoding untrusted tiles within limits, fuzz-style: raster tiles of the wrong size
 * are refused before decoding, and vector tiles that claim too much (a decompression
 * bomb of repeated ClosePath commands, too many features or points) are refused
 * before any geometry is built. Hostile tiles are generated here, not committed.
 * [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { crc32, deflateSync } from 'node:zlib';
import { VectorTile } from '@mapbox/vector-tile';
import { PbfReader, PbfWriter } from 'pbf';

import { readPng } from '../../test-support/png.ts';
import { type ImageDeps, TileRefused, VECTOR_LIMITS, checkVectorTile, decodeImagery, decodeTerrain, decodeVector, geometryPoints, rasterHeader } from './decode.ts';

const fixture = (name: string) => new Uint8Array(readFileSync(new URL(`../../fixtures/tiles/${name}`, import.meta.url)));
const TERRAIN_PNG = fixture('terrarium-6-31-19.png');
const IMAGERY_JPG = fixture('s2cloudless-2016-6-31-19.jpg');
const VECTOR_MVT = fixture('versatiles-osm-8-125-78.mvt');

// --- generated images ----------------------------------------------------------------

function chunk(type: string, body: Uint8Array): Uint8Array {
  const out = new Uint8Array(12 + body.length);
  const v = new DataView(out.buffer);
  v.setUint32(0, body.length);
  out.set(Buffer.from(type, 'latin1'), 4);
  out.set(body, 8);
  v.setUint32(8 + body.length, crc32(out.subarray(4, 8 + body.length)));
  return out;
}

/** A valid-looking PNG header claiming `w` × `h`, with a tiny compressed body. */
function pngClaiming(w: number, h: number): Uint8Array {
  const ihdr = new Uint8Array(13);
  const v = new DataView(ihdr.buffer);
  v.setUint32(0, w);
  v.setUint32(4, h);
  ihdr.set([8, 2, 0, 0, 0], 8);
  const parts = [
    new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', new Uint8Array(deflateSync(Buffer.alloc(64)))),
    chunk('IEND', new Uint8Array(0)),
  ];
  return Buffer.concat(parts);
}

/** A JPEG header (SOI, an APP0, SOF0) claiming `w` × `h`. */
function jpegClaiming(w: number, h: number): Uint8Array {
  return new Uint8Array([
    0xff, 0xd8,
    0xff, 0xe0, 0x00, 0x04, 0x00, 0x00,
    0xff, 0xc0, 0x00, 0x0b, 0x08, h >> 8, h & 0xff, w >> 8, w & 0xff, 0x01, 0x01, 0x11, 0x00,
    0xff, 0xd9,
  ]);
}

/** A stand-in decoder that records whether it ran, and decodes PNGs with the test reader. */
function fakeDeps(o: { claim?: { w: number; h: number } } = {}) {
  const seen = { decoded: 0, closed: 0 };
  const deps: ImageDeps = {
    async createImageBitmap(blob) {
      seen.decoded++;
      const bytes = new Uint8Array(await blob.arrayBuffer());
      // With a claimed size, blank pixels of that size; else the test reader's own.
      const png = o.claim
        ? { data: new Uint8ClampedArray(o.claim.w * o.claim.h * 4) }
        : bytes[0] === 0x89 ? readPng(bytes) : null;
      const width = o.claim?.w ?? (png as { width?: number } | null)?.width ?? 256;
      const height = o.claim?.h ?? (png as { height?: number } | null)?.height ?? 256;
      return { width, height, png, close: () => seen.closed++ } as unknown as ImageBitmap;
    },
    pixels(bmp) {
      return (bmp as unknown as { png: { data: Uint8ClampedArray } }).png.data;
    },
  };
  return { deps, seen };
}

test('headers: the recorded PNG and JPEG are read as 256 × 256', () => {
  assert.deepEqual(rasterHeader(TERRAIN_PNG), { format: 'png', width: 256, height: 256 });
  assert.deepEqual(rasterHeader(IMAGERY_JPG), { format: 'jpeg', width: 256, height: 256 });
  assert.deepEqual(rasterHeader(pngClaiming(512, 512)), { format: 'png', width: 512, height: 512 });
  assert.deepEqual(rasterHeader(jpegClaiming(300, 200)), { format: 'jpeg', width: 300, height: 200 });
  for (const junk of [new Uint8Array(0), new Uint8Array([0x89, 0x50]), VECTOR_MVT, new Uint8Array(64).fill(0xff), Buffer.from('<html>not a tile</html>')]) {
    assert.equal(rasterHeader(junk), null);
  }
});

test('the recorded tiles decode: terrain heights and an imagery bitmap', async () => {
  const { deps } = fakeDeps();
  const grid = await decodeTerrain(TERRAIN_PNG, deps);
  assert.equal(grid.width, 256);
  assert.equal(grid.data.length, 256 * 256);
  const bmp = await decodeImagery(IMAGERY_JPG, deps);
  assert.equal(bmp.width, 256);
});

test('a large-dimension PNG is refused from its header, before the decoder sees it', async () => {
  for (const [w, h] of [[65_535, 65_535], [2_000_000_000, 1], [100_000, 256], [256, 100_000], [4096, 4096], [1024, 1024]]) {
    const { deps, seen } = fakeDeps();
    const png = pngClaiming(w, h);
    assert.ok(png.length < 200, 'the hostile tile is tiny');
    await assert.rejects(decodeTerrain(png, deps), TileRefused, `${w} × ${h}`);
    await assert.rejects(decodeImagery(png, deps), TileRefused, `${w} × ${h}`);
    assert.equal(seen.decoded, 0, 'never decoded');
  }
});

test('only 256 × 256 tiles, or 512 × 512 terrain; PNG terrain only', async () => {
  const { deps, seen } = fakeDeps({ claim: { w: 512, h: 512 } });
  assert.equal((await decodeTerrain(pngClaiming(512, 512), deps)).width, 512);
  await assert.rejects(decodeImagery(pngClaiming(512, 512), deps), TileRefused);
  for (const [w, h] of [[255, 255], [257, 257], [256, 512], [300, 300], [0, 0], [1, 1]]) {
    await assert.rejects(decodeTerrain(pngClaiming(w, h), deps), TileRefused, `${w} × ${h}`);
    await assert.rejects(decodeImagery(jpegClaiming(w, h), deps), TileRefused, `${w} × ${h}`);
  }
  await assert.rejects(decodeTerrain(jpegClaiming(256, 256), deps), /not a PNG/);
  assert.equal(seen.decoded, 1);
});

test('a decoder that disagrees with the header is refused, and its bitmap closed', async () => {
  const { deps, seen } = fakeDeps({ claim: { w: 8192, h: 8192 } });
  await assert.rejects(decodeImagery(IMAGERY_JPG, deps), TileRefused);
  assert.equal(seen.closed, 1);
});

// --- generated vector tiles ---------------------------------------------------------

const command = (id: number, count: number) => (count << 3) | id;

/** One layer of features, each with these geometry integers, in the MVT encoding. */
function vectorTile(layers: { name: string; features: number; geometry: number[]; type?: number }[]): Uint8Array {
  const pbf = new PbfWriter();
  for (const l of layers) {
    pbf.writeMessage(3, (_: unknown, p: PbfWriter) => {
      p.writeVarintField(15, 2);
      p.writeStringField(1, l.name);
      for (let i = 0; i < l.features; i++) {
        p.writeMessage(2, (_f: unknown, f: PbfWriter) => {
          f.writeVarintField(3, l.type ?? 3);
          f.writePackedVarint(4, l.geometry);
        }, null);
      }
      p.writeVarintField(5, 4096);
    }, null);
  }
  return pbf.finish();
}

test('the recorded vector tile is within the limits and decodes', () => {
  const d = decodeVector(VECTOR_MVT, 8);
  assert.ok(d.fills.water.length > 0);
  assert.ok(d.labels.length > 0);
});

test('point counting matches the library\'s own geometry, feature by feature', () => {
  const tile = new VectorTile(new PbfReader(VECTOR_MVT));
  let n = 0;
  for (const layer of Object.values(tile.layers)) {
    for (let i = 0; i < layer.length; i++) {
      const built = layer.feature(i).loadGeometry().reduce((s, r) => s + r.length, 0);
      assert.equal(geometryPoints(layer.feature(i), Infinity), built);
      n += built;
    }
  }
  assert.ok(n > 10_000 && n < VECTOR_LIMITS.points, String(n));
});

test('a decompression bomb, a ClosePath repeated 100 million times in a few bytes, is refused before it is built (a ClosePath count must be 1)', () => {
  // MoveTo(0,0), LineTo(1,0), LineTo(0,1), then ClosePath × 2^27 − 1.
  const geometry = [command(1, 1), 0, 0, command(2, 2), 2, 0, 1, 2, command(7, 2 ** 27 - 1)];
  const bomb = vectorTile([{ name: 'land', features: 1, geometry }]);
  assert.ok(bomb.length < 64, `${bomb.length} bytes`);
  const t0 = performance.now();
  assert.throws(() => decodeVector(bomb, 8), (e: unknown) => e instanceof TileRefused && /ClosePath count/.test(e.message));
  assert.ok(performance.now() - t0 < 200, 'refused at once, nothing built');
  // Many small ones add up the same way.
  const many = vectorTile([{ name: 'land', features: 200, geometry: [command(1, 1), 0, 0, command(7, 10_000)] }]);
  assert.throws(() => decodeVector(many, 8), /malformed: ClosePath count/);
});

test('too many features in one layer, or in the tile, or too many points, is too complex', () => {
  const point = [command(1, 1), 2, 2];
  assert.throws(() => decodeVector(vectorTile([{ name: 'pois', features: VECTOR_LIMITS.featuresPerLayer + 1, geometry: point, type: 1 }]), 14), /too complex.*one layer/);
  const per = Math.floor(VECTOR_LIMITS.featuresPerLayer * 0.9);
  const layers = Array.from({ length: Math.ceil(VECTOR_LIMITS.features / per) + 1 }, (_, i) => ({ name: `l${i}`, features: per, geometry: point, type: 1 }));
  assert.throws(() => decodeVector(vectorTile(layers), 14), /too complex.*features/);
  // A long line of zero steps: two bytes a point.
  const line = [command(1, 1), 0, 0, command(2, 60_000), ...new Array(120_000).fill(0)];
  const long = vectorTile([{ name: 'streets', features: 1, geometry: line, type: 2 }]);
  assert.throws(() => checkVectorTile(new VectorTile(new PbfReader(long)), { ...VECTOR_LIMITS, points: 50_000 }), /too complex.*points/);
  assert.doesNotThrow(() => checkVectorTile(new VectorTile(new PbfReader(long))));
});

test('an unknown geometry command is refused', () => {
  const bad = vectorTile([{ name: 'land', features: 1, geometry: [command(1, 1), 0, 0, command(5, 3)] }]);
  assert.throws(() => decodeVector(bad, 8), TileRefused);
});

// --- re-audit: command counts the library reads as signed ---------------------------

test('a "negative" ClosePath, then a huge one, and a MoveTo: refused, not counted as 2 points', () => {
  // loadGeometry takes cmdLen >> 3 as signed: 0x80000007 is a ClosePath of count
  // -268,435,455 (run once), 0x7fffffff a ClosePath of 268,435,455 (each repeat a
  // point). The old counter added the two and saw almost nothing.
  const geometry = [command(1, 1), 0, 0, 0x80000007, 0x7fffffff, command(1, 1), 2, 2];
  const bomb = vectorTile([{ name: 'land', features: 1, geometry }]);
  assert.ok(bomb.length < 64, `${bomb.length} bytes`);
  const t0 = performance.now();
  assert.throws(() => decodeVector(bomb, 8), (e: unknown) => e instanceof TileRefused && /malformed: command integer/.test(e.message));
  assert.ok(performance.now() - t0 < 100);
  // Each half alone is refused too.
  assert.throws(() => decodeVector(vectorTile([{ name: 'land', features: 1, geometry: [command(1, 1), 0, 0, 0x80000007] }]), 8), /command integer/);
  assert.throws(() => decodeVector(vectorTile([{ name: 'land', features: 1, geometry: [command(1, 1), 0, 0, 0x7fffffff] }]), 8), /ClosePath count/);
  // Any command integer of 2^31 or more, whatever the command.
  for (const c of [0x80000001, 0x80000002, 0xfffffff9, 0xffffffff]) {
    assert.throws(() => decodeVector(vectorTile([{ name: 'land', features: 1, geometry: [c, 0, 0] }]), 8), /command integer/, c.toString(16));
  }
});

test('a ClosePath count other than 1 is refused', () => {
  for (const n of [2, 3, 1000]) {
    assert.throws(() => decodeVector(vectorTile([{ name: 'land', features: 1, geometry: [command(1, 1), 0, 0, command(2, 2), 2, 0, 1, 2, command(7, n)] }]), 8), /ClosePath count/, String(n));
  }
  // Count 1 is the spec, and passes.
  assert.doesNotThrow(() => decodeVector(vectorTile([{ name: 'land', features: 1, geometry: [command(1, 1), 0, 0, command(2, 2), 2, 0, 1, 2, command(7, 1)] }]), 8));
});

test('a huge MoveTo or LineTo count with no parameters left is refused', () => {
  for (const g of [[command(2, 1_000_000)], [command(1, 1), 0, 0, command(2, 2 ** 28 - 1)], [command(1, 5), 0, 0], [command(1, 1), 0, 0, command(2, 3), 2, 2]]) {
    assert.throws(() => decodeVector(vectorTile([{ name: 'streets', features: 1, geometry: g, type: 2 }]), 8), /malformed/, JSON.stringify(g));
  }
});

test('the count is exactly what loadGeometry builds, LineTo and ClosePath before any MoveTo included', () => {
  const cases = [
    [command(1, 1), 0, 0, command(2, 2), 2, 0, 1, 2, command(7, 1)],
    [command(2, 2), 2, 0, 1, 2, command(1, 1), 0, 0, command(2, 1), 4, 4],
    [command(7, 1), command(1, 2), 0, 0, 2, 2, command(7, 1), command(7, 1)],
    [command(1, 0), command(2, 0), command(1, 1), 6, 6],
  ];
  for (const geometry of cases) {
    const tile = new VectorTile(new PbfReader(vectorTile([{ name: 'land', features: 1, geometry }])));
    const built = tile.layers.land.feature(0).loadGeometry().reduce((n, r) => n + r.length, 0);
    assert.equal(geometryPoints(tile.layers.land.feature(0), Infinity), built, JSON.stringify(geometry));
  }
});
