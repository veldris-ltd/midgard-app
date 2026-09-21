import * as React from 'react';

/** AI inference tier badge (FR-C-6): on-device (green), remote enclosure (blue), hybrid (accent), disabled (grey). Every AI answer on a client carries one. */
export interface TierBadgeProps extends React.HTMLAttributes<HTMLSpanElement> {
  tier?: 'on-device' | 'remote' | 'hybrid' | 'disabled';
  /** Model name, e.g. "GEMMA INT4" */
  model?: string;
  /** Explicit data_leaves_device readout */
  leaves?: boolean | null;
  style?: React.CSSProperties;
}

type Tier = NonNullable<TierBadgeProps['tier']>;

/** TierBadge — which AI tier served a result: on-device, remote (enclosure), hybrid, or disabled. */
export function TierBadge({ tier = 'on-device', model, leaves = null, style, ...rest }: TierBadgeProps) {
  const map: Record<Tier, { c: string; label: string; d: string | null }> = {
    'on-device': { c: 'var(--secure)', label: 'ON-DEVICE', d: leaves === null ? 'data stays local' : null },
    remote:      { c: 'var(--info)', label: 'REMOTE · ENCLOSURE', d: 'PQ-mTLS' },
    hybrid:      { c: 'var(--accent-hover)', label: 'HYBRID', d: 'local draft · enclosure verify' },
    disabled:    { c: 'var(--grey-500)', label: 'AI DISABLED', d: 'capability absent · graceful' },
  };
  const t = map[tier] || map['on-device'];
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', gap: 7,
      padding: '3px 9px',
      fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.07em',
      background: 'var(--surface-inset)', border: '1px solid var(--border)',
      borderRadius: 'var(--radius-xs)', whiteSpace: 'nowrap',
      ...style,
    }} {...rest}>
      <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke={t.c} strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" style={{ flex: 'none' }}>
        {tier === 'remote' ? <><rect x="2" y="3" width="20" height="8" rx="2"/><rect x="2" y="13" width="20" height="8" rx="2"/></> : tier === 'disabled' ? <><rect x="4" y="4" width="16" height="16" rx="2"/><path d="m4 4 16 16"/></> : <><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="9" y="9" width="6" height="6"/></>}
      </svg>
      <span style={{ color: t.c, fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'] }}>{t.label}</span>
      {model && <span style={{ color: 'var(--grey-300)' }}>{model}</span>}
      {(t.d || leaves !== null) && <span style={{ color: 'var(--text-faint)' }}>{leaves !== null ? (leaves ? 'data leaves device · policy-permitted' : 'data stays local') : t.d}</span>}
    </span>
  );
}
