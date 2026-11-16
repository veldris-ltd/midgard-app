// The demo fixtures' marking rules, shared by extract-fixtures.mjs and its test.
// [AC-C-13] [FR-C-1]
//
// Pure functions. A refusal throws a MarkingError; the extractor turns it into a
// fatal error naming the row.

export const LEVELS = ['open', 'internal', 'restricted', 'confidential'];
export const rank = (level) => LEVELS.indexOf(level);
export const covers = (a, b) => rank(a.level) >= rank(b.level) && b.compartments.every((k) => a.compartments.includes(k));
export const show = (m) => (m.compartments.length ? `${m.level} [${m.compartments.join(', ')}]` : m.level);

export class MarkingError extends Error {}

// Words for a level above the four the demo knows. Any of them, as a token or a
// two-token phrase, refuses the row: the demo ships nothing above confidential.
// Text is NFKC-folded (full-width letters), upper-cased and split at every run of
// characters other than A-Z and 0-9, so TOP_SECRET, "T.S." and "S.E.C.R.E.T" are
// read as their words.
const ABOVE_TOP = ['S', 'SECRET', 'TS', 'TOP SECRET', 'TOP-SECRET', 'COSMIC', 'NATO SECRET', 'OFFICIAL-SENSITIVE'];
const tokens = (s) => String(s).normalize('NFKC').toUpperCase().split(/[^A-Z0-9]+/).filter(Boolean);
const ABOVE = new Set(ABOVE_TOP.map((w) => tokens(w).join(' ')));
export function aboveTop(s) {
  const t = tokens(s);
  return t.some((w, i) => ABOVE.has(w) || (i + 1 < t.length && ABOVE.has(`${w} ${t[i + 1]}`)));
}
function refuseAboveTop(s) {
  if (aboveTop(s)) throw new MarkingError(`level above confidential cannot ship in the demo: ${JSON.stringify(s)}`);
}

// A display marking string in a common form ("C // OPS-N", "RESTRICTED"): a level
// word or letter, then "//"-separated compartments. null when it starts with no
// level word; refused when it does but does not read whole ("C/OPS-N").
const WORDS = {
  C: 'confidential', CONFIDENTIAL: 'confidential', R: 'restricted', RESTRICTED: 'restricted',
  I: 'internal', INTERNAL: 'internal', O: 'open', U: 'open', OPEN: 'open', UNCLASSIFIED: 'open',
};
const word = (w) => (Object.hasOwn(WORDS, w) ? WORDS[w] : undefined);
const COMPARTMENT = /^[A-Za-z0-9][A-Za-z0-9_-]*$/;
export function readMarkingString(s) {
  refuseAboveTop(s);
  if (!word(tokens(s)[0] ?? '')) return null;
  const [head, ...rest] = s.normalize('NFKC').split('//').map((t) => t.trim());
  const level = word(head.toUpperCase());
  if (!level || !rest.every((k) => COMPARTMENT.test(k)))
    throw new MarkingError(`unreadable marking ${JSON.stringify(s)}: write a level, then "//"-separated compartments`);
  return { level, compartments: [...new Set(rest)].sort() };
}

// The marking a row carries itself: null when it has none, { unknown } when it has one
// this extractor does not recognise. A `markings` list yields its highest level and the
// union of its compartments.
export function itemMarking(x) {
  const comps = (v) => {
    if (v === undefined || v === null) return []; // the contract's `compartment: string | null`
    if (typeof v === 'string' && v !== '') return [v];
    if (Array.isArray(v) && v.every((k) => typeof k === 'string' && k !== '')) return v;
    return null;
  };
  if (x.marking !== undefined) {
    if (typeof x.marking === 'string') refuseAboveTop(x.marking);
    if (!LEVELS.includes(x.marking)) return { unknown: x.marking };
    const cs = [...(comps(x.compartment) ?? [null]), ...(comps(x.compartments) ?? [null])];
    if (cs.includes(null)) return { unknown: { compartment: x.compartment, compartments: x.compartments } };
    return { level: x.marking, compartments: [...new Set(cs)].sort() };
  }
  if (x.markings !== undefined) {
    const ms = x.markings;
    if (typeof ms === 'string') refuseAboveTop(ms);
    if (!Array.isArray(ms) || ms.length === 0) return { unknown: ms };
    let level = null; const cs = new Set();
    for (const m of ms) {
      if (m && typeof m.level === 'string') refuseAboveTop(m.level);
      if (!m || !LEVELS.includes(m.level)) return { unknown: ms };
      const c = comps(m.compartments);
      if (c === null) return { unknown: ms };
      c.forEach((k) => cs.add(k));
      if (level === null || rank(m.level) > rank(level)) level = m.level;
    }
    return { level, compartments: [...cs].sort() };
  }
  return null;
}

// The highest level named anywhere inside a row (`marking`/`level` values only); a
// level above confidential in any classification-like field inside it is refused.
const CLASSIFICATION_KEYS = new Set(['marking', 'markings', 'level', 'classification', 'caveat', 'class']);
export function innerFloor(v) {
  let top = null;
  const walk = (x) => {
    if (Array.isArray(x)) { x.forEach(walk); return; }
    if (!x || typeof x !== 'object') return;
    for (const [k, y] of Object.entries(x)) {
      if (CLASSIFICATION_KEYS.has(k) && typeof y === 'string') refuseAboveTop(y);
      if ((k === 'marking' || k === 'level') && LEVELS.includes(y) && (top === null || rank(y) > rank(top))) top = y;
      walk(y);
    }
  };
  walk(v);
  return top;
}
