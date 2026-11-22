/* The Heimdall mock's tracks carry plausible positions over the UK and its seas, for
 * the 3D globe, while keeping the 2D reference layout's x/y. Run with
 * `pnpm --filter @midgard/sdk test`. [AC-25.2] [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { HeimdallMock } from './heimdall';

test('every mock track has a lat/lon over the UK and its seas', async () => {
  const picture = await new HeimdallMock({ latencyMs: 0 }).getPicture();
  assert.equal(picture.tracks.length, 5);
  for (const t of picture.tracks) {
    assert.equal(typeof t.lat, 'number', `${t.id} has no lat`);
    assert.equal(typeof t.lon, 'number', `${t.id} has no lon`);
    assert.ok(t.lat! >= 49 && t.lat! <= 61, `${t.id} lat ${t.lat} is outside the UK box`);
    assert.ok(t.lon! >= -11 && t.lon! <= 3, `${t.id} lon ${t.lon} is outside the UK box`);
    assert.ok(t.x >= 0 && t.x <= 100 && t.y >= 0 && t.y <= 100, `${t.id} keeps its 2D position`);
  }
});
