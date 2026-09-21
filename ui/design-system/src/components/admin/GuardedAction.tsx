import * as React from 'react';

/** Dual-control approval card (FR-AD-5): risk bars, mandatory justification, requester/approver with live separation-of-duties check, time-boxed single-use token on approval. */
export interface GuardedActionProps extends React.HTMLAttributes<HTMLDivElement> {
  action: string;
  subsystem?: string;
  requester?: string;
  approver?: string;
  risk?: 1 | 2 | 3 | 4 | 5;
  state?: 'awaiting' | 'approved' | 'executed' | 'denied';
  justification?: string;
  /** Shown when state='denied', e.g. "DENIED · SELF-APPROVAL (AC-AD-4)" */
  deniedReason?: string;
  /** Token TTL label on approval. Default "10m" */
  tokenTtl?: string;
  onApprove?: () => void;
  onDeny?: () => void;
  style?: React.CSSProperties;
}

type GuardedActionState = NonNullable<GuardedActionProps['state']>;

/** GuardedAction — the dual-control approval card (FR-AD-5).
    States: awaiting (second approver pending), approved (time-boxed token issued),
    denied (e.g. separation-of-duties: requester == approver), executed. */
export function GuardedAction({
  action, subsystem, requester, approver,
  risk = 4, state = 'awaiting', justification,
  deniedReason, tokenTtl = '10m',
  onApprove, onDeny, style, ...rest
}: GuardedActionProps) {
  const palette: Record<GuardedActionState, { c: string; label: string }> = {
    awaiting: { c: 'var(--warn)', label: 'AWAITING SECOND APPROVER' },
    approved: { c: 'var(--secure)', label: 'APPROVED · TOKEN ' + tokenTtl + ' · SINGLE-USE' },
    executed: { c: 'var(--secure)', label: 'EXECUTED · AUDITED' },
    denied:   { c: 'var(--danger)', label: deniedReason || 'DENIED' },
  };
  const p = palette[state] || palette.awaiting;
  const riskColor = risk >= 4 ? 'var(--danger)' : risk === 3 ? 'var(--warn)' : 'var(--secure)';
  return (
    <div style={{
      background: 'var(--surface-card)', border: '1px solid ' + p.c,
      borderRadius: 'var(--radius-md)', padding: '13px 15px',
      display: 'flex', flexDirection: 'column', gap: 9,
      ...style,
    }} {...rest}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <span style={{ fontFamily: 'var(--font-sans)', fontSize: 'var(--fs-sm)', fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'], color: 'var(--text-heading)', flex: 1, minWidth: 160 }}>{action}</span>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)', letterSpacing: '0.08em' }}>{subsystem}</span>
        <span style={{ display: 'inline-flex', gap: 2, alignItems: 'flex-end' }}>
          {[1, 2, 3, 4, 5].map(i => <span key={i} style={{ width: 3, height: 3 + i * 1.6, borderRadius: 1, background: i <= risk ? riskColor : 'var(--surface-raised)' }} />)}
        </span>
      </div>
      {justification && (
        <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55, background: 'var(--surface-inset)', border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)', padding: '7px 10px' }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--text-faint)', display: 'block', marginBottom: 3 }}>JUSTIFICATION · MANDATORY</span>
          {justification}
        </div>
      )}
      <div style={{ display: 'flex', gap: 14, flexWrap: 'wrap', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)' }}>
        <span><span style={{ color: 'var(--text-faint)' }}>REQUESTER </span><span style={{ color: 'var(--text-body)' }}>{requester}</span></span>
        <span><span style={{ color: 'var(--text-faint)' }}>APPROVER </span><span style={{ color: approver ? 'var(--text-body)' : 'var(--text-faint)' }}>{approver || 'unassigned'}</span></span>
        <span style={{ color: requester && approver && requester === approver ? 'var(--danger)' : 'var(--secure)' }}>
          {requester && approver && requester === approver ? 'SEPARATION VIOLATED' : 'SEPARATION ✓'}
        </span>
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', letterSpacing: '0.1em', color: p.c, fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'] }}>{p.label}</span>
        <span style={{ flex: 1 }} />
        {state === 'awaiting' && onApprove && (
          <>
            <button type="button" onClick={onDeny} style={{ all: 'unset', cursor: 'pointer', padding: '6px 13px', borderRadius: 'var(--radius-sm)', fontFamily: 'var(--font-sans)', fontSize: 'var(--fs-xs)', fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'], color: 'var(--text-body)', border: '1px solid var(--border)' }}>Deny</button>
            <button type="button" onClick={onApprove} style={{ all: 'unset', cursor: 'pointer', padding: '6px 13px', borderRadius: 'var(--radius-sm)', fontFamily: 'var(--font-sans)', fontSize: 'var(--fs-xs)', fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'], color: 'var(--text-on-accent)', background: 'var(--accent)' }}>Approve · step-up auth</button>
          </>
        )}
      </div>
    </div>
  );
}
