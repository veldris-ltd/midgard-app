/* Mimir decision support for the selected item: on request, ranked courses of action
 * from the sovereign model (Mimir, in the enclosure), each citing the picture evidence
 * it rests on, with every citation checked against the live picture (grounded) or
 * flagged (unverified). Strictly advisory: nothing here acts; a human decides, and the
 * request is recorded in Heimdall's audit ledger. [FR-H-4] [NFR-AI-1] */
import * as React from 'react';
import { Card } from '@midgard/design-system';
import type { HeimdallAdvice, HeimdallGateway } from '@midgard/sdk';

const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.08em' };
const btn = (primary: boolean): React.CSSProperties => ({
  all: 'unset', cursor: 'pointer', ...mono, padding: '5px 10px', borderRadius: 'var(--radius-sm)',
  border: `1px solid ${primary ? 'var(--accent)' : 'var(--divider)'}`, color: primary ? 'var(--accent-hover)' : 'var(--text-muted)',
});

type State = { status: 'idle' } | { status: 'thinking'; since: number } | { status: 'done'; advice: HeimdallAdvice } | { status: 'failed'; reason: string };

/** `subject`: the entity key; `alertId`: its open alert, if any (advice is then about
 *  the alert). `pick` selects a cited entity by key. */
export function AdviceCard({ gateway, subject, alertId, pick }: { gateway: HeimdallGateway; subject: string; alertId?: string; pick: (key: string) => void }) {
  const [state, setState] = React.useState<State>({ status: 'idle' });
  const [question, setQuestion] = React.useState('');
  const [, tick] = React.useState(0);
  const current = React.useRef(subject);
  current.current = subject;
  React.useEffect(() => { setState({ status: 'idle' }); setQuestion(''); }, [subject]);
  React.useEffect(() => {
    if (state.status !== 'thinking') return;
    const t = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(t);
  }, [state.status]);
  if (!gateway.advise) return null;

  const ask = () => {
    if (state.status === 'thinking') return;
    const asked = subject;
    setState({ status: 'thinking', since: Date.now() });
    gateway.advise!(alertId ? { alert: alertId } : { entity: subject }, question).then(
      (advice) => { if (current.current === asked) setState({ status: 'done', advice }); },
      (err) => { if (current.current === asked) setState({ status: 'failed', reason: err instanceof Error ? err.message : String(err) }); },
    );
  };

  return (
    <Card size="sm">
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
        <span className="mg-label" style={{ color: 'var(--text-faint)' }}>MIMIR · DECISION SUPPORT</span>
        <span style={{ marginLeft: 'auto', ...mono, color: 'var(--text-faint)' }}>{alertId ? `ON ALERT ${alertId}` : 'ON THIS ITEM'}</span>
      </div>
      <div style={{ display: 'flex', gap: 6 }}>
        <input value={question} onChange={(e) => setQuestion(e.target.value)} placeholder="Optional question for Mimir…" maxLength={400}
          onKeyDown={(e) => { if (e.key === 'Enter') ask(); }} disabled={state.status === 'thinking'}
          style={{ flex: 1, minWidth: 0, ...mono, fontSize: 10, padding: '5px 8px', background: 'var(--bg-sunken)', color: 'var(--text-body)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)' }} />
        <button type="button" onClick={ask} disabled={state.status === 'thinking'} style={{ ...btn(true), opacity: state.status === 'thinking' ? 0.6 : 1 }}>
          {state.status === 'done' ? 'ASK AGAIN' : 'ASK MIMIR'}
        </button>
      </div>
      {state.status === 'idle' && (
        <div style={{ ...mono, color: 'var(--text-faint)', marginTop: 8, lineHeight: 1.6 }}>
          RANKED COURSES OF ACTION FROM THE ENCLOSURE'S OWN MODEL, EACH CITING THE EVIDENCE IT RESTS ON. ADVISORY ONLY.
        </div>
      )}
      {state.status === 'thinking' && (
        <div style={{ ...mono, color: 'var(--accent-hover)', marginTop: 8, lineHeight: 1.6 }}>
          MIMIR IS THINKING · {Math.round((Date.now() - state.since) / 1000)} S · ABOUT A MINUTE ON THIS LAPTOP'S CPU
        </div>
      )}
      {state.status === 'failed' && (
        <div style={{ ...mono, color: 'var(--warn)', marginTop: 8, lineHeight: 1.6, overflowWrap: 'anywhere' }}>
          NO ADVICE · {state.reason.toUpperCase()}
        </div>
      )}
      {state.status === 'done' && (
        <div style={{ marginTop: 8, display: 'grid', gap: 8 }}>
          {state.advice.options.length === 0 && (
            <div style={{ ...mono, color: 'var(--text-muted)', lineHeight: 1.6 }}>
              NO OPTION COULD BE SUPPORTED FROM THE PICTURE{state.advice.unsupported ? ` · ${state.advice.unsupported.toUpperCase()}` : ''}
            </div>
          )}
          {state.advice.options.map((o) => (
            <div key={o.rank} style={{ borderLeft: `3px solid ${o.grounded ? 'var(--secure)' : 'var(--warn)'}`, paddingLeft: 8, minWidth: 0, overflow: 'hidden' }}>
              <div style={{ display: 'flex', gap: 6, alignItems: 'baseline', flexWrap: 'wrap' }}>
                <span style={{ ...mono, fontWeight: 700, color: 'var(--accent-hover)' }}>{o.rank}</span>
                <span style={{ flex: '1 1 140px', minWidth: 0, fontSize: 'var(--fs-sm, 12px)', fontWeight: 600, color: 'var(--text-body)', overflowWrap: 'anywhere' }}>{o.title}</span>
                <span style={{ marginLeft: 'auto', ...mono, color: o.grounded ? 'var(--secure)' : 'var(--warn)', whiteSpace: 'nowrap' }}>
                  {o.grounded ? '✓ GROUNDED' : '⚠ UNVERIFIED'}
                </span>
              </div>
              <div style={{ fontSize: 11.5, color: 'var(--text-muted)', marginTop: 3, lineHeight: 1.45, overflowWrap: 'anywhere' }}>{o.summary}</div>
              {o.expectedEffect && <div style={{ ...mono, color: 'var(--text-faint)', marginTop: 4, lineHeight: 1.5, overflowWrap: 'anywhere' }}>EFFECT · {o.expectedEffect.toUpperCase()}</div>}
              {o.risks.length > 0 && <div style={{ ...mono, color: 'var(--warn)', marginTop: 2, lineHeight: 1.5, overflowWrap: 'anywhere' }}>RISKS · {o.risks.join(' · ').toUpperCase()}</div>}
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 4, minWidth: 0 }}>
                {o.citations.map((c) => {
                  const [kind, id] = c.includes(':') ? [c.slice(0, c.indexOf(':')), c.slice(c.indexOf(':') + 1)] : ['', c];
                  const bad = o.unresolved.includes(c);
                  return (
                    <button key={c} type="button" title={bad ? 'Not found in the picture' : `Show ${id}`} disabled={kind !== 'entity' && kind !== 'track'}
                      onClick={() => pick(id)}
                      style={{ all: 'unset', cursor: kind === 'entity' || kind === 'track' ? 'pointer' : 'default', ...mono, fontSize: 8.5, padding: '1px 5px', borderRadius: 2,
                        border: `1px solid ${bad ? 'var(--warn)' : 'var(--divider)'}`, color: bad ? 'var(--warn)' : 'var(--text-muted)', maxWidth: '100%', boxSizing: 'border-box', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {c}
                    </button>
                  );
                })}
              </div>
            </div>
          ))}
          <div style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', lineHeight: 1.6 }}>
            ADVISORY ONLY · A HUMAN DECIDES · {state.advice.considered} ITEMS CONSIDERED · {Math.round(state.advice.ms / 1000)} S · SOVEREIGN MODEL IN THE ENCLOSURE · RECORDED IN THE AUDIT LEDGER
          </div>
        </div>
      )}
    </Card>
  );
}
