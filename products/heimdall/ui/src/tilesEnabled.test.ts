/* The map tiles switch's value, as vite.config.ts reads VITE_HEIMDALL_TILES. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { tilesEnabled } from './tilesEnabled.ts';

test('on unless set to off, false, 0 or no, in any case and with any surrounding space', () => {
  for (const v of [undefined, '', 'on', 'true', '1', 'yes', 'anything']) assert.equal(tilesEnabled(v), true, String(v));
  for (const v of ['off', 'false', '0', 'no', 'OFF', 'No', ' off ', '\tfalse\n', ' 0', 'NO ']) assert.equal(tilesEnabled(v), false, JSON.stringify(v));
});
