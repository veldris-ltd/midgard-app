/* Decoding the bundled Natural Earth outlines. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';

import { coastFromLand, decodeLine, type OutlineFile } from './outlines.ts';

const load = (f: string) => JSON.parse(readFileSync(new URL(`./data/${f}`, import.meta.url), 'utf8')) as OutlineFile;

test('decodeLine undoes the delta encoding in hundredths of a degree', () => {
  const pts = decodeLine([-420, 5735, 10, -5, -20, 0], 0.01);
  assert.deepEqual(Array.from(pts), [-4.2, 57.35, -4.1, 57.3, -4.3, 57.3]);
});

test('coastFromLand drops the cuts along the antimeridian and the south pole', () => {
  const q = 0.01;
  // A square island split at 180°: its edge on the meridian is not coast.
  const ring = [17900, 100, 100, 0, 0, -200, -100, 0, 0, 200];
  const lines = coastFromLand([[ring]], q);
  assert.deepEqual(lines.map((l) => Array.from(l)), [[180, -1, 179, -1, 179, 1, 180, 1]]);
  // A ring reaching the pole: the run along -90° is dropped.
  const pole = [0, -8000, 1000, -1000, -2000, 0, 1000, 1000];
  const polar = coastFromLand([[pole]], q).map((l) => Array.from(l));
  assert.deepEqual(polar, [[-10, -90, 0, -80, 10, -90]]);
});

test('the bundled files are Natural Earth at a pinned version, and decode in range', () => {
  for (const f of ['ne-110m.json', 'ne-50m.json']) {
    const d = load(f);
    assert.equal(d.format, 'midgard-heimdall-outlines/1');
    assert.equal(d.source.name, 'Natural Earth');
    assert.equal(d.source.licence, 'Public domain');
    assert.match(d.source.version, /^v\d+\.\d+\.\d+$/);
    for (const s of d.source.files) assert.match(s.sha256, /^[0-9a-f]{64}$/);
    const lines = [...(d.coast ?? []), ...(d.borders ?? []), ...(d.land ?? []).flat()];
    assert.ok(lines.length > 100, f);
    for (const l of lines) {
      const p = decodeLine(l, d.quantum);
      for (let i = 0; i < p.length; i += 2) {
        assert.ok(Math.abs(p[i]) <= 180 && Math.abs(p[i + 1]) <= 90, `${f}: ${p[i]}, ${p[i + 1]}`);
      }
    }
  }
  const close = load('ne-50m.json');
  assert.ok(coastFromLand(close.land!, close.quantum).length >= close.land!.length);
});
