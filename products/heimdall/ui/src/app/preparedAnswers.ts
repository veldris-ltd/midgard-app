/* Prepared Mimir answers: each real answer Mimir gives is kept in this browser for
 * two hours, keyed by the exact question (and the alert or item it was about), so
 * asking the same thing again — in a demonstration, say — shows that answer at once
 * instead of waiting minutes on a laptop CPU. A replayed answer is shown exactly as
 * Mimir gave it, with that call's figures; it is not a new model call and is not
 * recorded again in the audit ledger. Kept in localStorage, so a private window or
 * cleared site data simply asks Mimir afresh. */

const KEY = 'heimdall.mimir.prepared';
const KEEP_MS = 2 * 60 * 60 * 1000;

export interface Prepared<T> {
  value: T;
  /** When Mimir gave the answer, epoch ms. */
  at: number;
}

type Store = Record<string, Prepared<unknown>>;

function load(): Store {
  try {
    const raw = window.localStorage.getItem(KEY);
    const all = raw ? (JSON.parse(raw) as Store) : {};
    const now = Date.now();
    return Object.fromEntries(Object.entries(all).filter(([, p]) => now - p.at < KEEP_MS));
  } catch {
    return {};
  }
}

/** The key for a question about `about` (an alert id, item key or area). */
export function preparedKey(kind: 'ask' | 'advise', about: string, question: string): string {
  return `${kind}|${about}|${question.trim().toLowerCase().replace(/\s+/g, ' ')}`;
}

export function getPrepared<T>(key: string): Prepared<T> | null {
  return (load()[key] as Prepared<T> | undefined) ?? null;
}

export function putPrepared<T>(key: string, value: T): void {
  try {
    const all = load();
    all[key] = { value, at: Date.now() };
    window.localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    /* storage unavailable: nothing is prepared, Mimir is simply asked each time */
  }
}
