/* The operator's watchlist: tracks starred in the inspector, by entity key, kept in this
 * browser (localStorage, guarded: blocked storage just means the list lasts the page).
 * Each entry remembers its label and when it was last in the picture, so the summary
 * can say "last seen 12 min ago" after it has gone. [FR-H-1] */
import * as React from 'react';

const KEY = 'heimdall.watchlist';

export interface WatchEntry {
  key: string;
  label: string;
  /** Unix s the entity was last in the picture (0: not yet seen). */
  lastSeen: number;
}

function read(): WatchEntry[] {
  try {
    const v = JSON.parse(window.localStorage.getItem(KEY) ?? '[]');
    return Array.isArray(v) ? v.filter((e) => e && typeof e.key === 'string').map((e) => ({ key: e.key, label: String(e.label ?? e.key), lastSeen: Number(e.lastSeen) || 0 })) : [];
  } catch {
    return [];
  }
}

function write(list: WatchEntry[]): void {
  try { window.localStorage.setItem(KEY, JSON.stringify(list)); } catch { /* storage blocked: kept for this page only */ }
}

export interface Watchlist {
  entries: WatchEntry[];
  has: (key: string) => boolean;
  toggle: (key: string, label: string) => void;
  /** Record which watched keys are in the picture now (label and last-seen time). */
  seen: (present: Map<string, string>, now: number) => void;
}

export function useWatchlist(): Watchlist {
  const [entries, setEntries] = React.useState<WatchEntry[]>(read);
  const update = React.useCallback((f: (l: WatchEntry[]) => WatchEntry[]) => {
    setEntries((l) => {
      const next = f(l);
      if (next !== l) write(next);
      return next;
    });
  }, []);
  const has = React.useCallback((key: string) => entries.some((e) => e.key === key), [entries]);
  const toggle = React.useCallback((key: string, label: string) => {
    update((l) => (l.some((e) => e.key === key) ? l.filter((e) => e.key !== key) : [{ key, label, lastSeen: Math.floor(Date.now() / 1000) }, ...l]));
  }, [update]);
  const seen = React.useCallback((present: Map<string, string>, now: number) => {
    update((l) => {
      let changed = false;
      const next = l.map((e) => {
        const label = present.get(e.key);
        if (label === undefined) return e;
        // Write at most once a minute per entry (enough for "last seen").
        if (now - e.lastSeen < 60 && label === e.label) return e;
        changed = true;
        return { ...e, label, lastSeen: now };
      });
      return changed ? next : l;
    });
  }, [update]);
  return { entries, has, toggle, seen };
}
