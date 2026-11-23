/* The map tile source setting fails closed: none, mirror or public exactly, a valid
 * https mirror only with mirror, and the retired switch refused. [FR-H-1] [NFR-PRV-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { TileConfigError, isPublicIpLiteral, parseTileMirror, parseTileSource, tileConfig } from './tileSource.ts';

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
  assert.deepEqual(tileConfig({}), { mode: 'none', mirror: '', warnings: [] });
  assert.deepEqual(tileConfig({ source: 'public' }), { mode: 'public', mirror: '', warnings: [] });
  assert.deepEqual(tileConfig({ source: 'mirror', mirror: 'https://m.internal/t/' }), { mode: 'mirror', mirror: 'https://m.internal/t', warnings: [] });
  assert.throws(() => tileConfig({ source: 'mirror' }), /required/);
  assert.throws(() => tileConfig({ source: 'none', mirror: 'https://m.internal' }), /not mirror/);
  assert.throws(() => tileConfig({ mirror: 'https://m.internal' }), /not mirror/);
  assert.throws(() => tileConfig({ source: 'public', mirror: 'https://m.internal' }), /not mirror/);
  assert.throws(() => tileConfig({ source: '' }), TileConfigError);
  for (const legacy of ['off', 'on', '']) assert.throws(() => tileConfig({ legacy }), /retired/);
  assert.throws(() => tileConfig({ source: 'public', legacy: 'on' }), /retired/);
});

test('a mirror on a public tile host, or anywhere under its domain, stops the build', () => {
  for (const v of [
    'https://s3.amazonaws.com', 'https://s3.amazonaws.com/elevation-tiles-prod', 'https://amazonaws.com',
    'https://elevation-tiles-prod.s3.amazonaws.com', 'https://my-bucket.s3.eu-west-2.amazonaws.com/tiles',
    'https://tiles.maps.eox.at', 'https://maps.eox.at', 'https://eox.at', 'https://a.eox.at/x',
    'https://tiles.versatiles.org', 'https://versatiles.org/tiles', 'https://download.versatiles.org',
  ]) {
    assert.throws(() => parseTileMirror(v), (e: unknown) => e instanceof TileConfigError && /public tile host/.test(e.message), v);
  }
  // Look-alikes that are not those domains are not refused for it.
  for (const v of ['https://notamazonaws.com', 'https://eox.at.enclosure.internal', 'https://versatiles.internal']) {
    assert.equal(parseTileMirror(v), v);
  }
});

test('a mirror on a public IP address builds with a warning; a private or loopback one without', () => {
  for (const host of ['8.8.8.8', '203.0.113.7', '172.32.0.1', '[2001:db8::1]']) {
    const c = tileConfig({ source: 'mirror', mirror: `https://${host}/m` });
    assert.equal(c.warnings.length, 1, host);
    assert.match(c.warnings[0], /public IP address/);
  }
  for (const host of ['10.1.2.3', '172.16.0.1', '172.31.255.255', '192.168.1.10', '127.0.0.1', '169.254.1.1', '100.64.0.1', '[::1]', '[fd00::1]', '[fe80::1]', 'tiles.enclosure.internal']) {
    assert.deepEqual(tileConfig({ source: 'mirror', mirror: `https://${host}/m` }).warnings, [], host);
  }
  assert.equal(isPublicIpLiteral('tiles.example'), false);
});

test('an IPv4-mapped IPv6 mirror is judged by its IPv4 address, as URL writes it', () => {
  // A canonical mirror is written as URL writes it: 10.0.0.1, 192.168.1.10, 127.0.0.1, 172.16.0.1.
  for (const host of ['[::ffff:a00:1]', '[::ffff:c0a8:10a]', '[::ffff:7f00:1]', '[::ffff:ac10:1]']) {
    assert.deepEqual(tileConfig({ source: 'mirror', mirror: `https://${host}/m` }).warnings, [], host);
  }
  // 8.8.8.8 and 203.0.113.7.
  for (const host of ['[::ffff:808:808]', '[::ffff:cb00:7107]']) {
    assert.equal(tileConfig({ source: 'mirror', mirror: `https://${host}/m` }).warnings.length, 1, host);
  }
  assert.equal(isPublicIpLiteral('[::ffff:a00:1]'), false);
  assert.equal(isPublicIpLiteral('[::ffff:808:808]'), true);
});

test('a public build is refused unless the console is in mock mode', () => {
  assert.equal(tileConfig({ source: 'public', dataMode: 'mock' }).mode, 'public');
  assert.throws(() => tileConfig({ source: 'public', dataMode: 'http' }), /mock mode/);
  assert.equal(tileConfig({ source: 'mirror', mirror: 'https://m.internal', dataMode: 'http' }).mode, 'mirror');
  assert.equal(tileConfig({ dataMode: 'http' }).mode, 'none');
});
