/* The globe's colours come from the design tokens, read as the browser computes them,
 * so the clean style follows the theme and the light or dark mode. [FR-H-1] */

export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

const num = (s: string, scale: number) => (s.endsWith('%') ? parseFloat(s) / 100 : parseFloat(s) / scale);

/** A computed CSS colour (`rgb()`, `rgba()` or `color(srgb …)`) as 0–1 channels; null otherwise. */
export function parseCssColour(css: string): Rgba | null {
  const s = css.trim();
  let m = /^rgba?\(\s*([^)]*)\)$/i.exec(s);
  if (m) {
    const parts = m[1].split(/[\s,/]+/).filter(Boolean);
    if (parts.length < 3) return null;
    const [r, g, b] = parts.slice(0, 3).map((p) => num(p, 255));
    const a = parts[3] !== undefined ? num(parts[3], 1) : 1;
    return [r, g, b, a].every(Number.isFinite) ? { r, g, b, a } : null;
  }
  m = /^color\(\s*srgb\s+([^)]*)\)$/i.exec(s);
  if (m) {
    const parts = m[1].split(/[\s/]+/).filter(Boolean);
    if (parts.length < 3) return null;
    const [r, g, b] = parts.slice(0, 3).map((p) => num(p, 1));
    const a = parts[3] !== undefined ? num(parts[3], 1) : 1;
    return [r, g, b, a].every(Number.isFinite) ? { r, g, b, a } : null;
  }
  return null;
}

/** The token each part of the clean style uses. */
export const GLOBE_TOKENS = {
  sea: '--bg-sunken',
  land: '--surface-raised',
  coast: '--border-strong',
  border: '--border',
  graticule: '--divider',
  atmosphere: '--text-muted',
  accent: '--accent',
  warn: '--warn',
} as const;

export type GlobeColours = Record<keyof typeof GLOBE_TOKENS, Rgba>;

const FALLBACK: GlobeColours = {
  sea: { r: 0.03, g: 0.035, b: 0.04, a: 1 },
  land: { r: 0.16, g: 0.17, b: 0.19, a: 1 },
  coast: { r: 0.3, g: 0.32, b: 0.35, a: 1 },
  border: { r: 0.24, g: 0.25, b: 0.28, a: 1 },
  graticule: { r: 1, g: 1, b: 1, a: 0.07 },
  atmosphere: { r: 0.57, g: 0.6, b: 0.64, a: 1 },
  accent: { r: 0.85, g: 0.65, b: 0.2, a: 1 },
  warn: { r: 0.94, g: 0.66, b: 0.14, a: 1 },
};

/** Reads the tokens through a probe element inside `scope`, so var() chains resolve. */
export function readGlobeColours(scope: HTMLElement): GlobeColours {
  const probe = document.createElement('span');
  probe.style.display = 'none';
  scope.appendChild(probe);
  const out = { ...FALLBACK };
  try {
    for (const key of Object.keys(GLOBE_TOKENS) as (keyof GlobeColours)[]) {
      probe.style.color = `var(${GLOBE_TOKENS[key]})`;
      out[key] = parseCssColour(getComputedStyle(probe).color) ?? FALLBACK[key];
    }
  } finally {
    probe.remove();
  }
  return out;
}

export function cssRgba(c: Rgba): string {
  const to = (v: number) => Math.round(Math.min(1, Math.max(0, v)) * 255);
  return `rgba(${to(c.r)}, ${to(c.g)}, ${to(c.b)}, ${c.a})`;
}
