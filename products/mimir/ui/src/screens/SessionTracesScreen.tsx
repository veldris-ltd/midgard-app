import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  Terminal,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { MimirSession, MimirTraceStep, MimirGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

/* Sessions — plan–act–observe traces with per-step evals, guardrail screens, and
 * PQ-encrypted memory records. `[FR-MI-3, NFR-PQ-2]` */

const KIND_COLOR: Record<string, string> = {
  plan: 'var(--info)', act: 'var(--accent)', observe: 'var(--grey-400)',
  guard: 'var(--secure)', eval: 'var(--secure)', gate: 'var(--warn)',
};

export function SessionTracesScreen() {
  const { data: session } = useAsync<MimirSession, MimirGateway>((c) => c.getSessionTrace('S-8812'), []);
  const steps: MimirTraceStep[] = session?.steps ?? [];
  const memory = session?.memory ?? [];

  const [sel, setSel] = React.useState(6);
  const s = steps[sel] ?? steps[steps.length - 1];

  return (
    <ConsolePage
      active="sessions"
      context="MIMIR / SESSION S-8812 · DISPATCH-01"
      asideWidth={340}
      toolbar={
        <>
          <Badge tone="accent">GLM 5.2 · LOCAL</Badge>
          <Button variant="secondary" size="sm" iconLeft={<Icon name="download" size={14} />}>Export trace</Button>
        </>
      }
      aside={s ? (
        <>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
            <Icon name="terminal" size={17} color={KIND_COLOR[s.kind]} />
            <h2 style={{ fontSize: 15, flex: 1 }}>{s.label}</h2>
            {s.ok === true && <Badge tone="secure">OK</Badge>}
            {s.ok === null && <Badge tone="warn">PENDING</Badge>}
          </div>
          <Card size="sm">
            <p style={{ margin: 0, fontSize: 'var(--fs-sm)', color: 'var(--text-body)', lineHeight: 1.6 }}>{s.detail}</p>
            {s.ms != null && <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-faint)', marginTop: 8 }}>{s.ms}MS</div>}
          </Card>
          <div>
            <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>MEMORY RECORDS · PQ-ENCRYPTED</div>
            {memory.map((m) => (
              <div key={m.key} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)' }}>
                <span style={{ color: 'var(--grey-100)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{m.key}</span>
                <span style={{ color: 'var(--text-faint)' }}>{m.kind}</span>
                <span style={{ color: 'var(--text-faint)' }}>{m.age}</span>
              </div>
            ))}
          </div>
          <Terminal title="SESSION LEDGER" lines={[
            'agent    dispatch-01 · policy v6',
            'steps    7 · tools 2 · guards 1',
            'evals    per-step · all passed',
            'sig      ML-DSA · session lineage',
          ]} style={{ marginTop: 'auto' }} />
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap' }}>
            <ProvenanceTag id="FR-MI-3" title="Agent runtime with tools, memory, planning, and per-agent Evals" />
            <ProvenanceTag id="NFR-PQ-2" title="Agent memory PQ-encrypted at rest" />
          </div>
        </>
      ) : null}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={7}>SESSIONS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Session trace · S-8812</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="7" label="STEPS" size="sm" />
        <StatBlock value={session?.total ?? '—'} label="TOTAL · FIRST TOKEN 420MS" size="sm" />
        <StatBlock value="1" label="HUMAN GATE PENDING" size="sm" accent />
      </div>

      <div className="mg-grid-bg" style={{ flex: 1, background: 'var(--bg-sunken)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', padding: 22, overflowY: 'auto' }}>
        <div style={{ display: 'flex', flexDirection: 'column', maxWidth: 640 }}>
          {steps.map((st, i) => (
            <React.Fragment key={i}>
              <div onClick={() => setSel(i)} style={{
                display: 'flex', alignItems: 'center', gap: 12, padding: '10px 14px', cursor: 'pointer',
                background: i === sel ? 'var(--surface-raised)' : 'var(--surface-card)',
                border: '1px solid ' + (i === sel ? 'var(--accent)' : st.kind === 'gate' ? 'var(--warn)' : 'var(--border)'),
                borderRadius: 'var(--radius-md)',
              }}>
                <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: KIND_COLOR[st.kind], width: 58, flex: 'none', textTransform: 'uppercase' }}>{st.kind}</span>
                <div style={{ flex: 1 }}>
                  <div style={{ fontSize: 'var(--fs-sm)', fontWeight: 600, color: 'var(--text-heading)' }}>{st.label}</div>
                  <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', marginTop: 2 }}>{st.detail}</div>
                </div>
                {st.ms != null && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-faint)' }}>{st.ms}ms</span>}
                {st.kind === 'gate' && <Badge tone="warn">HUMAN GATE</Badge>}
              </div>
              {i < steps.length - 1 && <div style={{ width: 2, height: 10, background: 'var(--border-strong)', marginLeft: 40 }} />}
            </React.Fragment>
          ))}
        </div>
      </div>
    </ConsolePage>
  );
}
