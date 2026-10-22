/* Prepared Mimir answers, from two stores:
 *
 * - Shipped (never expires): the Mac app ships real Mimir answers captured over its
 *   recorded hour in `prepared-answers.json` and sets `window.__MIDGARD_PREPARED__` to
 *   that file's URL (`<origin>/home/prepared-answers.json`). It is fetched once (the
 *   promise is cached), and any failure — no file, bad JSON, wrong version — leaves the
 *   store empty. Unset (the Vite dev servers, a plain build), nothing changes.
 * - This browser (two hours): each real answer Mimir gives is kept in localStorage,
 *   keyed by the exact question (and the alert or item it was about), so asking the same
 *   thing again — in a demonstration, say — shows that answer at once instead of waiting
 *   minutes on a laptop CPU. A private window or cleared site data asks Mimir afresh.
 *
 * `getPrepared` checks the shipped store first, then localStorage. A replayed answer is
 * shown exactly as Mimir gave it, with that call's figures; it is not a new model call
 * and is not recorded again in the audit ledger.
 *
 * Keys (`preparedKey`): `<kind>|<about>|<question>`, the question trimmed, lower-cased
 * and with each run of whitespace made one space. In this browser `about` is the area
 * label (ask) or the alert id, else the item key (advise). Shipped keys (`shippedKey`)
 * do not depend on the area or on alert ids, which change every replay loop:
 *   - map questions:    `ask|*|<question>`            (any area)
 *   - decision support: `advise|entity:<alert subject entity key>|<question>`
 *                       (the alert's `entity`; the question is often empty: the default)
 *
 * The shipped file (version 1), written by `tools/mac-app/prepare-answers.py`:
 *
 *   {
 *     "version": 1,
 *     "recording": "hour-1",
 *     "created": <epoch ms>,
 *     "entries": { "<shipped key>": { "value": <HeimdallAnswer | HeimdallAdvice>, "at": <epoch ms> } },
 *     "guide": {
 *       "questions": [{"question": "<question as shown>", "region": "UK & Ireland" | "Gulf" | …}, …],
 *       "alerts": [{ "key": "<shipped key>", "label": "…", "subject": "<entity key>", "kind": "…" }, …]
 *     }
 *   }
 *
 * `guide` is for the home page's Presenter's guide; this module reads only `entries`. */

const KEY = 'heimdall.mimir.prepared';
const KEEP_MS = 2 * 60 * 60 * 1000;
/** A local file: give up on it after this and ask Mimir live. */
const SHIPPED_TIMEOUT_MS = 4000;

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

function normalise(question: string): string {
  return question.trim().toLowerCase().replace(/\s+/g, ' ');
}

/** The key for a question about `about` (an alert id, item key or area). */
export function preparedKey(kind: 'ask' | 'advise', about: string, question: string): string {
  return `${kind}|${about}|${normalise(question)}`;
}

/** The shipped store's key: `ask` ignores `about` (any area); `advise` takes the alert's
 *  subject entity key. */
export function shippedKey(kind: 'ask' | 'advise', about: string, question: string): string {
  return kind === 'ask' ? preparedKey('ask', '*', question) : preparedKey('advise', `entity:${about}`, question);
}

let shipped: Store = Object.create(null) as Store;
let shippedLoad: Promise<void> | null = null;

/** The shipped file's entries, or {} if it is not a version 1 file. */
export function parseShipped(file: unknown): Store {
  if (!file || typeof file !== 'object') return {};
  const { version, entries } = file as { version?: unknown; entries?: unknown };
  if (version !== 1 || !entries || typeof entries !== 'object' || Array.isArray(entries)) return {};
  // No prototype: a key such as `constructor` or `__proto__` is just a key.
  const out: Store = Object.create(null) as Store;
  for (const [k, p] of Object.entries(entries as Record<string, unknown>)) {
    if (p && typeof p === 'object' && 'value' in p && typeof (p as Prepared<unknown>).at === 'number'
      && (p as Prepared<unknown>).value && typeof (p as Prepared<unknown>).value === 'object') {
      out[k] = { value: (p as Prepared<unknown>).value, at: (p as Prepared<unknown>).at };
    }
  }
  return out;
}

/** Fetches the shipped answers once (later calls share the first's promise). Never
 *  rejects: a missing or bad file leaves the shipped store empty. */
export function loadShipped(): Promise<void> {
  if (shippedLoad) return shippedLoad;
  const url = typeof window !== 'undefined' ? window.__MIDGARD_PREPARED__ : undefined;
  if (!url || typeof fetch !== 'function') return (shippedLoad = Promise.resolve());
  const abort = typeof AbortController === 'function' ? new AbortController() : null;
  const timer = abort ? setTimeout(() => abort.abort(), SHIPPED_TIMEOUT_MS) : null;
  shippedLoad = fetch(url, abort ? { signal: abort.signal } : undefined)
    .then((r) => (r.ok ? r.json() : null))
    .then((file) => { shipped = parseShipped(file); })
    .catch(() => { /* no shipped answers: Mimir is asked live */ })
    .finally(() => { if (timer) clearTimeout(timer); });
  return shippedLoad;
}

/** The first of `keys` found: each in the shipped store, then in this browser. */
export function getPrepared<T>(...keys: string[]): Prepared<T> | null {
  for (const k of keys) if (Object.hasOwn(shipped, k)) return shipped[k] as Prepared<T>;
  const local = load();
  for (const k of keys) if (Object.hasOwn(local, k)) return local[k] as Prepared<T>;
  return null;
}

/** `getPrepared` once the shipped answers are loaded (a local file, so quick). */
export async function findPrepared<T>(...keys: string[]): Promise<Prepared<T> | null> {
  await loadShipped();
  return getPrepared<T>(...keys);
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

// Start loading at once, so the shipped answers are usually in before the first click.
if (typeof window !== 'undefined') void loadShipped();
