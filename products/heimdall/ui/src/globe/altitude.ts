/* The altitude parser: reads the console's `alt` strings as the globe needs them.
 * [FR-H-1]
 *
 * `FL340` is a flight level, hundreds of feet; `SURFACE`, `GND` and `SFC` are zero;
 * `2500 FT` and `800 M` are plain heights. A trailing ↑ or ↓ is the trend. Anything
 * else is unknown (null), never silently zero: the globe draws an unknown altitude on
 * the surface and says so. */

export const FEET_TO_METRES = 0.3048;

export type AltitudeTrend = 'level' | 'climbing' | 'descending';

export interface Altitude {
  feet: number;
  metres: number;
  trend: AltitudeTrend;
}

const SURFACE = new Set(['SURFACE', 'GND', 'SFC']);

export function parseAltitude(raw: string): Altitude | null {
  let s = raw.trim().toUpperCase();
  let trend: AltitudeTrend = 'level';
  const arrow = /\s*([↑↓])$/u.exec(s);
  if (arrow) {
    trend = arrow[1] === '↑' ? 'climbing' : 'descending';
    s = s.slice(0, arrow.index);
  }
  if (SURFACE.has(s)) return { feet: 0, metres: 0, trend };

  const fl = /^FL\s*(\d{1,3})$/.exec(s);
  if (fl) {
    const feet = Number(fl[1]) * 100;
    return { feet, metres: feet * FEET_TO_METRES, trend };
  }

  const plain = /^(\d+(?:\.\d+)?)\s*(FT|M)$/.exec(s);
  if (plain) {
    const n = Number(plain[1]);
    return plain[2] === 'FT'
      ? { feet: n, metres: n * FEET_TO_METRES, trend }
      : { feet: n / FEET_TO_METRES, metres: n, trend };
  }
  return null;
}
