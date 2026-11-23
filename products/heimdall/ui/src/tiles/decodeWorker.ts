/* The tile decode worker: a same-origin module worker, its own file in dist/ (never a
 * blob: URL), so a CSP with `worker-src 'self'` holds. Untrusted tile bytes are decoded
 * here, off the page's thread, within the limits of decode.ts; the pool (decodePool.ts)
 * gives each tile about 2 s and replaces a worker that runs over. [FR-H-1] */
/// <reference lib="webworker" />
import { type ImageDeps, decodeImagery, decodeTerrain, decodeVector } from './decode.ts';
import type { DecodeJob, DecodeReply } from './decodePool.ts';

const scope = self as unknown as DedicatedWorkerGlobalScope;

const deps: ImageDeps = {
  createImageBitmap: (blob, options) => scope.createImageBitmap(blob, options),
  pixels(bmp) {
    const canvas = new OffscreenCanvas(bmp.width, bmp.height);
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    if (!ctx) throw new Error('terrain: no 2D canvas in the worker');
    ctx.drawImage(bmp, 0, 0);
    return ctx.getImageData(0, 0, bmp.width, bmp.height).data;
  },
};

scope.onmessage = async (e: MessageEvent<DecodeJob>) => {
  const job = e.data;
  const bytes = new Uint8Array(job.bytes);
  try {
    if (job.kind === 'imagery') {
      const bmp = await decodeImagery(bytes, deps);
      scope.postMessage({ id: job.id, ok: true, value: bmp } satisfies DecodeReply, [bmp]);
    } else if (job.kind === 'terrain') {
      const grid = await decodeTerrain(bytes, deps);
      scope.postMessage({ id: job.id, ok: true, value: grid } satisfies DecodeReply, [grid.data.buffer]);
    } else {
      const data = decodeVector(bytes, job.z);
      scope.postMessage({ id: job.id, ok: true, value: data } satisfies DecodeReply);
    }
  } catch (err) {
    const error = err instanceof Error ? err : new Error(String(err));
    scope.postMessage({ id: job.id, ok: false, name: error.name, message: error.message } satisfies DecodeReply);
  }
};
