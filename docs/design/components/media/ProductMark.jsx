import React from 'react';

/** ProductMark, typographic product identity (no logos exist; plain type only).
    Initial tile in the product tint + Oxanium wordmark + mono descriptor. */
export function ProductMark({ product = 'midgard', showDescriptor = true, size = 'md', style, ...rest }) {
  const products = {
    midgard:  { name: 'MIDGARD', desc: 'CORE PLATFORM', c: 'var(--midgard-300)', tint: 'var(--midgard-tint)' },
    forge:    { name: 'FORGE', desc: 'DATA OPERATIONS', c: 'var(--forge-300)', tint: 'var(--forge-tint)' },
    mimir:    { name: 'MIMIR', desc: 'LOCAL AGENTIC AI', c: 'var(--mimir-300)', tint: 'var(--mimir-tint)' },
    bifrost:  { name: 'BIFROST', desc: 'CONTINUOUS DELIVERY', c: 'var(--bifrost-300)', tint: 'var(--bifrost-tint)' },
    heimdall: { name: 'HEIMDALL', desc: 'OPERATIONAL FUSION', c: 'var(--heimdall-300)', tint: 'var(--heimdall-tint)' },
    admin:    { name: 'ADMIN', desc: 'SUITE CONTROL PANEL', c: 'var(--grey-100)', tint: 'rgba(127,127,127,0.16)' },
  };
  const p = products[product] || products.midgard;
  const dims = { sm: { tile: 26, fs: 13, word: 13, desc: 8 }, md: { tile: 34, fs: 17, word: 15, desc: 9 }, lg: { tile: 46, fs: 24, word: 20, desc: 10 } };
  const d = dims[size] || dims.md;
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: Math.round(d.tile * 0.28), ...style }} {...rest}>
      <span style={{
        width: d.tile, height: d.tile, flex: 'none',
        borderRadius: Math.round(d.tile * 0.26),
        border: '1px solid var(--border-strong)',
        background: p.tint, color: p.c,
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center',
        fontFamily: 'var(--font-display)', fontWeight: 'var(--fw-black)', fontSize: d.fs,
      }}>{p.name[0]}</span>
      <span style={{ display: 'inline-flex', flexDirection: 'column', gap: 1 }}>
        <span style={{ fontFamily: 'var(--font-display)', fontWeight: 'var(--fw-bold)', fontSize: d.word, letterSpacing: '0.07em', color: 'var(--text-heading)', lineHeight: 1 }}>{p.name}</span>
        {showDescriptor && (
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: d.desc, letterSpacing: '0.13em', color: 'var(--text-faint)', lineHeight: 1.3 }}>{p.desc}</span>
        )}
      </span>
    </span>
  );
}
