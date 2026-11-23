/* The bundled Natural Earth outlines, as same-origin assets. [FR-H-1]
 *
 * The two JSON files ship beside the console's scripts (hashed, like any asset) and
 * are fetched from there when the globe opens, so they stay out of the JavaScript
 * chunk. Same origin, no credentials: still no outbound call. */
import worldUrl from './data/ne-110m.json?url';
import closeUrl from './data/ne-50m.json?url';
import type { OutlineFile } from './outlines.ts';
import type { GlobeOutlines } from './renderer.ts';

const FORMAT = 'midgard-heimdall-outlines/1';

async function load(url: string): Promise<OutlineFile> {
  const res = await fetch(url, { credentials: 'omit' });
  if (!res.ok) throw new Error(`outlines: HTTP ${res.status}`);
  const data = (await res.json()) as OutlineFile;
  if (data?.format !== FORMAT) throw new Error('outlines: unexpected format');
  return data;
}

export async function loadOutlines(): Promise<GlobeOutlines> {
  const [world, close] = await Promise.all([load(worldUrl), load(closeUrl)]);
  return { world, close };
}
