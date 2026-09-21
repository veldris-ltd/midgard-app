import * as React from 'react';

/** Mandatory authorisation card for state-mutating AI proposals (NFR-AI-1). Amber pending; green authorised with audit id; neutral rejected. */
export interface HumanGateProps extends React.HTMLAttributes<HTMLDivElement> {
  /** The proposed action, e.g. "Schedule maintenance · AC-2214 → FAC-DUB" */
  title?: string;
  /** Override the default HUMAN GATE REQUIRED line */
  detail?: string;
  state?: 'pending' | 'authorised' | 'rejected';
  /** Audit id shown after resolution, e.g. "A-77120" */
  auditId?: string;
  onAuthorise?: () => void;
  onReject?: () => void;
  style?: React.CSSProperties;
}

/** HumanGate — the mandatory authorisation card for state-mutating AI proposals.
    Amber pending → green authorised (with audit id) or neutral rejected. */
export function HumanGate({
  title,
  detail,
  state = 'pending',
  auditId,
  onAuthorise,
  onReject,
  style,
  ...rest
}: HumanGateProps) {
  const border = state === 'authorised' ? 'var(--secure)' : state === 'rejected' ? 'var(--border-strong)' : 'var(--warn)';
  return (
    <div style={{
      padding: '12px 14px', background: 'var(--surface-inset)',
      border: '1px solid ' + border, borderRadius: 'var(--radius-md)',
      ...style,
    }} {...rest}>
      {state === 'authorised' ? (
        <div style={{ display: 'flex', alignItems: 'center', gap: 9 }}>
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="var(--secure)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="m9 12 2 2 4-4"/></svg>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.08em', color: 'var(--secure)' }}>
            AUTHORISED · SIGNED{auditId ? ' · AUDIT #' + auditId : ''}
          </span>
        </div>
      ) : state === 'rejected' ? (
        <div style={{ display: 'flex', alignItems: 'center', gap: 9 }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.08em', color: 'var(--text-faint)' }}>
            REJECTED · RECORDED{auditId ? ' · AUDIT #' + auditId : ''}
          </span>
        </div>
      ) : (
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="var(--warn)" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M4 14a1 1 0 0 1-.78-1.63l9.9-10.2a.5.5 0 0 1 .86.46l-1.92 6.02A1 1 0 0 0 13 10h7a1 1 0 0 1 .78 1.63l-9.9 10.2a.5.5 0 0 1-.86-.46l1.92-6.02A1 1 0 0 0 11 14z"/></svg>
          <div style={{ flex: 1, minWidth: 180 }}>
            {title && <div style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-heading)' }}>{title}</div>}
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--warn)', marginTop: title ? 2 : 0 }}>
              {detail || 'STATE MUTATION · HUMAN GATE REQUIRED'}
            </div>
          </div>
          <span style={{ display: 'inline-flex', gap: 8 }}>
            <button type="button" onClick={onAuthorise} style={{
              all: 'unset', boxSizing: 'border-box', cursor: 'pointer', padding: '7px 14px',
              background: 'var(--accent)', color: '#fff', borderRadius: 'var(--radius-sm)',
              fontFamily: 'var(--font-sans)', fontSize: 'var(--fs-xs)', fontWeight: 600,
            }}>Authorise</button>
            <button type="button" onClick={onReject} style={{
              all: 'unset', boxSizing: 'border-box', cursor: 'pointer', padding: '7px 10px',
              color: 'var(--text-muted)', borderRadius: 'var(--radius-sm)',
              fontFamily: 'var(--font-sans)', fontSize: 'var(--fs-xs)', fontWeight: 600,
            }}>Reject</button>
          </span>
        </div>
      )}
    </div>
  );
}
