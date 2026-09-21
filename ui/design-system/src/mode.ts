/* Midgard colour-mode manager (light / dark / system).
   Ported from docs/design/docs/mode.js. Persists the choice in
   localStorage('mg-mode'); resolves 'system' via prefers-color-scheme;
   toggles `.mode-light` on <html>; dispatches an `mg-mode` window event so
   components (ModeToggle) and canvases re-read tokens. Dark is the default —
   "the platform lives in the dark". */

export type Mode = 'dark' | 'light' | 'system';
export type ResolvedMode = 'dark' | 'light';

const KEY = 'mg-mode';
const EVENT = 'mg-mode';

function read(): Mode {
  try {
    const v = localStorage.getItem(KEY);
    if (v === 'light' || v === 'dark' || v === 'system') return v;
  } catch {
    /* storage unavailable */
  }
  return 'dark';
}

function prefersLight(): boolean {
  return typeof window !== 'undefined' && window.matchMedia('(prefers-color-scheme: light)').matches;
}

export function resolveMode(m: Mode = read()): ResolvedMode {
  return m === 'system' ? (prefersLight() ? 'light' : 'dark') : m;
}

function apply(): void {
  if (typeof document === 'undefined') return;
  document.documentElement.classList.toggle('mode-light', resolveMode() === 'light');
}

export function getMode(): Mode {
  return read();
}

export function setMode(m: Mode): void {
  try {
    localStorage.setItem(KEY, m);
  } catch {
    /* ignore */
  }
  apply();
  if (typeof window !== 'undefined') window.dispatchEvent(new Event(EVENT));
}

/** Subscribe to mode changes (returns an unsubscribe fn). */
export function onModeChange(fn: () => void): () => void {
  if (typeof window === 'undefined') return () => {};
  window.addEventListener(EVENT, fn);
  return () => window.removeEventListener(EVENT, fn);
}

let initialised = false;
/** Apply the persisted mode and wire the OS `prefers-color-scheme` listener. Idempotent. */
export function initMode(): void {
  if (initialised || typeof window === 'undefined') return;
  initialised = true;
  try {
    window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => {
      if (read() === 'system') {
        apply();
        window.dispatchEvent(new Event(EVENT));
      }
    });
  } catch {
    /* older browsers */
  }
  apply();
}
