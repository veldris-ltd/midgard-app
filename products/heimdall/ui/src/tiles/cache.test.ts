/* The tile cache: least recently used out first, never one still in use. [FR-H-1] */
import { test } from 'node:test';
import assert from 'node:assert/strict';

import { LruCache } from './cache.ts';

test('keeps at most its capacity, evicting the least recently used', () => {
  const evicted: string[] = [];
  const c = new LruCache<number>(2, (k) => evicted.push(k));
  c.set('a', 1);
  c.set('b', 2);
  assert.equal(c.get('a'), 1); // a is now the most recent
  c.set('c', 3);
  assert.deepEqual(evicted, ['b']);
  assert.equal(c.has('b'), false);
  assert.equal(c.size, 2);
  assert.deepEqual([...c.keys()], ['a', 'c']);
});

test('peek does not refresh an entry', () => {
  const c = new LruCache<number>(2);
  c.set('a', 1);
  c.set('b', 2);
  assert.equal(c.peek('a'), 1);
  c.set('c', 3);
  assert.equal(c.has('a'), false);
});

test('setting a key again replaces it and disposes the old value', () => {
  const evicted: [string, number][] = [];
  const c = new LruCache<number>(2, (k, v) => evicted.push([k, v]));
  c.set('a', 1);
  c.set('a', 5);
  assert.deepEqual(evicted, [['a', 1]]);
  assert.equal(c.get('a'), 5);
  assert.equal(c.size, 1);
});

test('protected entries are not evicted; the cache may run over until they are released', () => {
  const evicted: string[] = [];
  const c = new LruCache<number>(2, (k) => evicted.push(k));
  c.protect = (k) => k === 'a' || k === 'b';
  c.set('a', 1);
  c.set('b', 2);
  c.set('c', 3);
  assert.deepEqual(evicted, []);
  assert.equal(c.size, 3);
  c.protect = () => false;
  c.trim();
  assert.deepEqual(evicted, ['a']);
  assert.equal(c.size, 2);
});

test('delete and clear dispose what they remove', () => {
  const evicted: string[] = [];
  const c = new LruCache<number>(4, (k) => evicted.push(k));
  c.set('a', 1);
  c.set('b', 2);
  c.set('c', 3);
  assert.equal(c.delete('b'), true);
  assert.equal(c.delete('zz'), false);
  c.clear();
  assert.deepEqual(evicted, ['b', 'a', 'c']);
  assert.equal(c.size, 0);
});
