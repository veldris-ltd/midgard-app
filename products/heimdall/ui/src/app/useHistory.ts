/* Reads and keeps the recorded history behind the replay timeline and the trails.
 * [FR-P3-7]
 *
 * The window (15 min to 24 h, or the trail length if that is longer) is read once when
 * it is chosen, then every `REFRESH_SECS` only the newest minutes are read and merged
 * in. A gateway without history (`HeimdallHistoryUnavailableError`) leaves the store
 * empty and the status `unavailable`; it is asked again every minute, so the timeline
 * comes alive when the server does. The selected entity's path is read separately at
 * full resolution (step 0) so the highlighted path is not down-sampled. */
import * as React from 'react';
import { HeimdallHistoryUnavailableError } from '@midgard/sdk';
import type { HeimdallGateway, HeimdallHistoryPoint } from '@midgard/sdk';
import { mergeHistory, stepFor, type HistoryStore } from './history';

const REFRESH_SECS = 30;
const RETRY_SECS = 60;

export interface HistoryState {
  status: 'loading' | 'ready' | 'unavailable';
  /** Why history is unavailable, for the timeline's tooltip. */
  message?: string;
  store: HistoryStore;
  /** The span the store covers, unix s. */
  from: number;
  to: number;
  truncated: boolean;
}

const EMPTY: HistoryStore = new Map();

export function useHistory(gateway: HeimdallGateway, windowSecs: number, trailSecs: number): HistoryState {
  const [state, setState] = React.useState<HistoryState>({ status: 'loading', store: EMPTY, from: 0, to: 0, truncated: false });
  const span = Math.max(windowSecs, trailSecs);

  React.useEffect(() => {
    let live = true;
    let timer: number | undefined;
    const step = stepFor(windowSecs);
    let store: HistoryStore = EMPTY;
    let to = 0;

    const fail = (err: unknown) => {
      if (!live) return;
      const unavailable = err instanceof HeimdallHistoryUnavailableError;
      setState((s) => ({
        ...s,
        status: unavailable || !s.store.size ? 'unavailable' : s.status,
        message: err instanceof Error ? err.message : String(err),
      }));
      timer = window.setTimeout(unavailable || !to ? full : top, (unavailable ? RETRY_SECS : REFRESH_SECS) * 1000);
    };

    // The whole window, once.
    const full = () => {
      const now = Date.now() / 1000;
      setState((s) => (s.status === 'ready' ? s : { ...s, status: s.status === 'unavailable' ? 'unavailable' : 'loading' }));
      gateway.getHistory({ from: now - span, to: now, step }).then((h) => {
        if (!live) return;
        store = mergeHistory(EMPTY, h, now - span);
        to = h.to;
        setState({ status: 'ready', store, from: now - span, to, truncated: h.truncated });
        timer = window.setTimeout(top, REFRESH_SECS * 1000);
      }, fail);
    };

    // Only the newest minutes, overlapping the last read by a couple of steps.
    const top = () => {
      const now = Date.now() / 1000;
      const from = Math.max(now - span, to - Math.max(2 * step, 60));
      gateway.getHistory({ from, to: now, step }).then((h) => {
        if (!live) return;
        store = mergeHistory(store, h, now - span);
        to = Math.max(to, h.to);
        setState((s) => ({ status: 'ready', store, from: now - span, to, truncated: s.truncated || h.truncated }));
        timer = window.setTimeout(top, REFRESH_SECS * 1000);
      }, fail);
    };

    full();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [gateway, windowSecs, span]);

  return state;
}

/** The selected entity's whole path over the window at full resolution, re-read on each
 *  history refresh; null while there is none (or no history). */
export function useEntityPath(gateway: HeimdallGateway, key: string | null, windowSecs: number, refreshedTo: number): HeimdallHistoryPoint[] | null {
  const [path, setPath] = React.useState<{ key: string; points: HeimdallHistoryPoint[] } | null>(null);
  React.useEffect(() => {
    if (!key || !refreshedTo) return;
    let live = true;
    const now = Date.now() / 1000;
    gateway.getHistory({ from: now - windowSecs, to: now, entity: key, step: 0 }).then(
      (h) => { if (live) setPath({ key, points: h.tracks.find((t) => t.id === key)?.points ?? [] }); },
      () => { if (live) setPath(null); },
    );
    return () => { live = false; };
  }, [gateway, key, windowSecs, refreshedTo]);
  return path && path.key === key ? path.points : null;
}
