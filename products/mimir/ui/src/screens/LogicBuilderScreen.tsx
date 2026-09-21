import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { MimirLogicFn, MimirLogicStep, MimirGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

/* Logic — the no-code agent builder. Steps are sourced from the gateway's Logic
 * function; a mandatory human gate sits between any state-mutating proposal and
 * execution and cannot be deleted. `[FR-MI-4]` */

const KIND_COLOR: Record<string, string> = { trigger: 'var(--info)', gate: 'var(--warn)', audit: 'var(--secure)' };

function stepColor(kind: MimirLogicStep['kind']): string {
  if (kind === 'gate') return 'var(--warn)';
  if (kind === 'audit') return 'var(--secure)';
  return 'var(--accent)';
}

export function LogicBuilderScreen() {
  const { data: logic } = useAsync<MimirLogicFn, MimirGateway>((c) => c.getLogic('dispatch-01'), []);
  const steps: MimirLogicStep[] = logic?.steps ?? [];
  const versions = logic?.versions ?? [];

  const [sel, setSel] = React.useState(4);
  const s = steps[sel];

  return (
    <ConsolePage
      active="logic"
      context="MIMIR / LOGIC · DISPATCH-01"
      toolbar={
        <>
          <Button variant="secondary" size="sm">Test run</Button>
          <Button variant="primary" size="sm" iconLeft={<Icon name="check" size={14} />}>Publish v7</Button>
        </>
      }
      aside={s ? (
        <>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <Icon name={s.icon} size={17} color={KIND_COLOR[s.kind] || 'var(--accent)'} />
            <h2 style={{ fontSize: 15, flex: 1 }}>Step {s.n} · {s.title}</h2>
          </div>
          <Card size="sm">
            <p style={{ margin: 0, fontSize: 'var(--fs-sm)', color: 'var(--text-body)', lineHeight: 1.6 }}>{s.body}</p>
          </Card>
          {s.kind === 'gate' && (
            <Card size="sm" style={{ borderColor: 'var(--warn)' }}>
              <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--warn)', marginBottom: 6 }}>MANDATORY STEP</div>
              <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.55 }}>Logic containing a state-mutating action cannot publish without a human gate between proposal and execution. This step cannot be deleted.</p>
            </Card>
          )}
          <div>
            <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>VERSIONS</div>
            {versions.map((v, i) => (
              <div key={v.label} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
                <span style={{ color: i === 0 ? 'var(--accent-hover)' : 'var(--grey-100)', width: 76, flex: 'none' }}>{v.label}</span>
                <span style={{ color: 'var(--text-faint)' }}>{v.meta}</span>
              </div>
            ))}
          </div>
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
            <ProvenanceTag id="FR-MI-4" title="No-code (Logic) and pro-code builders for AI functions and agents" />
          </div>
        </>
      ) : null}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={6}>LOGIC</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>dispatch-01 · workflow v7</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="7" label="STEPS" size="sm" />
        <StatBlock value="1" label="HUMAN GATES" size="sm" accent />
        <StatBlock value="96%" label="EVALS · V6 LIVE" size="sm" />
      </div>

      <div className="mg-grid-bg" style={{ flex: 1, background: 'var(--bg-sunken)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', padding: 24, overflowY: 'auto' }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 0, maxWidth: 560 }}>
          {steps.map((st, i) => (
            <React.Fragment key={st.n}>
              <div onClick={() => setSel(i)} style={{
                display: 'flex', alignItems: 'center', gap: 12, padding: '12px 16px', cursor: 'pointer',
                background: i === sel ? 'var(--surface-raised)' : 'var(--surface-card)',
                border: '1px solid ' + (i === sel ? 'var(--accent)' : st.kind === 'gate' ? 'var(--warn)' : 'var(--border)'),
                borderRadius: 'var(--radius-md)',
                boxShadow: i === sel ? 'var(--shadow-glow)' : 'none',
              }}>
                <span style={{ fontFamily: 'var(--font-display)', fontWeight: 800, fontSize: 15, color: i === sel ? 'var(--accent-hover)' : 'var(--text-faint)', width: 22 }}>{String(st.n).padStart(2, '0')}</span>
                <Icon name={st.icon} size={16} color={stepColor(st.kind)} />
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 'var(--fs-sm)', fontWeight: 600, color: 'var(--text-heading)' }}>{st.title}</div>
                  <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', marginTop: 2 }}>{st.body}</div>
                </div>
                {st.kind === 'gate' && <Badge tone="warn">MANDATORY</Badge>}
              </div>
              {i < steps.length - 1 && <div style={{ width: 2, height: 14, background: 'var(--border-strong)', marginLeft: 43 }} />}
            </React.Fragment>
          ))}
        </div>
      </div>
    </ConsolePage>
  );
}
