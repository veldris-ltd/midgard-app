/* The map tile source setting fails closed: none, mirror or public exactly, a valid
 * https mirror only with mirror, and the retired switch refused. [FR-H-1] [NFR-PRV-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { TileConfigError, parseTileMirror, parseTileSource, tileConfig } from './tileSource.ts';

test('unset is none; none, mirror and public are taken as written', () => {
  assert.equal(parseTileSource(undefined), 'none');
  assert.equal(parseTileSource('none'), 'none');
  assert.equal(parseTileSource('mirror'), 'mirror');
  assert.equal(parseTileSource('public'), 'public');
});

test('an empty value, a typo, another case, padding or an old switch value fails', () => {
  for (const v of ['', ' ', 'None', 'PUBLIC', ' public', 'public ', 'publc', 'pubic', 'mirrors', 'mirorr', 'off', 'on', 'true', 'false', '0', '1', 'yes', 'no', 'public,mirror']) {
    assert.throws(() => parseTileSource(v), (e: unknown) => e instanceof TileConfigError && /none, mirror or public/.test(e.message), JSON.stringify(v));
  }
});

test('a mirror is an https origin with an optional plain base path, canonical, trailing slash dropped', () => {
  assert.equal(parseTileMirror('https://tiles.enclosure.internal'), 'https://tiles.enclosure.internal');
  assert.equal(parseTileMirror('https://tiles.enclosure.internal/'), 'https://tiles.enclosure.internal');
  assert.equal(parseTileMirror('https://tiles.enclosure.internal/heimdall'), 'https://tiles.enclosure.internal/heimdall');
  assert.equal(parseTileMirror('https://tiles.enclosure.internal/a/b-c_d.e~f/'), 'https://tiles.enclosure.internal/a/b-c_d.e~f');
  assert.equal(parseTileMirror('https://127.0.0.1:8443/m'), 'https://127.0.0.1:8443/m');
});

test('a mirror that is missing, not https, carries credentials, a query or a fragment, or is not canonical fails', () => {
  const bad = [
    undefined,
    '',
    'tiles.enclosure.internal',
    'http://tiles.enclosure.internal',
    'ftp://tiles.enclosure.internal',
    '//tiles.enclosure.internal',
    'https://user:pw@tiles.enclosure.internal',
    'https://user@tiles.enclosure.internal',
    'https://tiles.enclosure.internal?x=1',
    'https://tiles.enclosure.internal/?',
    'https://tiles.enclosure.internal#a',
    'https://tiles.enclosure.internal/a/../b',
    'https://tiles.enclosure.internal/a/./b',
    'https://tiles.enclosure.internal/%2e%2e/b',
    'https://tiles.enclosure.internal/a%20b',
    'https://tiles.enclosure.internal//a',
    'https://tiles.enclosure.internal/a//',
    'https:\\\\tiles.enclosure.internal',
    'https://TILES.enclosure.internal',
    'https://tiles.enclosure.internal:443',
    ' https://tiles.enclosure.internal',
    'https://tiles.enclosure.internal/a b',
  ];
  for (const v of bad) assert.throws(() => parseTileMirror(v), TileConfigError, JSON.stringify(v));
});

test('the whole setting: mirror needs a mirror, the others refuse one, and the retired switch fails', () => {
  assert.deepEqual(tileConfig({}), { mode: 'none', mirror: '' });
  assert.deepEqual(tileConfig({ source: 'public' }), { mode: 'public', mirror: '' });
  assert.deepEqual(tileConfig({ source: 'mirror', mirror: 'https://m.internal/t/' }), { mode: 'mirror', mirror: 'https://m.internal/t' });
  assert.throws(() => tileConfig({ source: 'mirror' }), /required/);
  assert.throws(() => tileConfig({ source: 'none', mirror: 'https://m.internal' }), /not mirror/);
  assert.throws(() => tileConfig({ mirror: 'https://m.internal' }), /not mirror/);
  assert.throws(() => tileConfig({ source: 'public', mirror: 'https://m.internal' }), /not mirror/);
  assert.throws(() => tileConfig({ source: '' }), TileConfigError);
  for (const legacy of ['off', 'on', '']) assert.throws(() => tileConfig({ legacy }), /retired/);
  assert.throws(() => tileConfig({ source: 'public', legacy: 'on' }), /retired/);
});
