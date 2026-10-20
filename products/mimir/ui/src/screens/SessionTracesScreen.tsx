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
  // The recent sessions (live: every call the plane has served since it started, each
  // joined to its signed ledger entry), and the selected one's trace. [FR-MI-3] [FR-MI-8]
  const { data: sessions } = useAsync<MimirSession[], MimirGateway>((c) => c.listSessions(), []);
  const [selId, setSelId] = React.useState<string | null>(null);
  const id = selId ?? sessions?.[0]?.id ?? 'S-8812';
  const { data: session } = useAsync<MimirSession, MimirGateway>((c) => c.getSessionTrace(id), [id]);
  const steps: MimirTraceStep[] = session?.steps ?? [];
  const memory = session?.memory ?? [];
  const live = session?.live;

  const [sel, setSel] = React.useState(6);
  const s = steps[sel] ?? steps[steps.length - 1];
  const hm = (at: number) => new Date(at * 1000).toISOString().slice(11, 19) + 'Z';

  return (
    <ConsolePage
      active="sessions"
      context={`MIMIR / SESSION ${session?.id ?? id} · ${(session?.agent ?? '').toUpperCase()}`}
      asideWidth={340}
      toolbar={
        <>
          <Badge tone="accent">{live ? `${(live.model ?? 'model').replace(/^llama-cpp\//, '').toUpperCase()} · LOCAL` : 'Mistral Small 3.2 · LOCAL'}</Badge>
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
            {live && memory.length === 0 && <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>NONE · A DIRECT INFERENCE CALL KEEPS NO AGENT MEMORY</div>}
            {memory.map((m) => (
              <div key={m.key} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)' }}>
                <span style={{ color: 'var(--grey-100)', flex: 1, overflow: 'hidden', textOverflow: 'ellipsis' }}>{m.key}</span>
                <span style={{ color: 'var(--text-faint)' }}>{m.kind}</span>
                <span style={{ color: 'var(--text-faint)' }}>{m.age}</span>
              </div>
            ))}
          </div>
          <Terminal title="SESSION LEDGER" lines={live ? [
            `caller   ${live.caller} · purpose ${live.purpose}`,
            ...(live.context ? [`request  ${live.context}`] : []),
            `outcome  ${live.outcome} · ${live.promptTokens} in · ${live.tokens} out`,
            live.ledger ? `entry    #${live.ledger.seq} ${live.ledger.action}` : 'entry    older than the newest reported',
            live.ledger ? `hash     ${live.ledger.entryHash.slice(0, 32)}…` : 'hash     —',
            live.ledger ? `sig      ${live.ledger.sigAlg} · verified chain` : 'sig      ML-DSA',
          ] : [
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
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Session trace · {session?.id ?? id}</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(steps.length)} label="STEPS" size="sm" />
        <StatBlock value={session?.total ?? '—'} label={`TOTAL · FIRST TOKEN ${session?.firstToken ?? '—'}`.toUpperCase()} size="sm" />
        {live
          ? <StatBlock value={live.ledger ? `#${live.ledger.seq}` : '—'} label="SIGNED LEDGER ENTRY" size="sm" accent />
          : <StatBlock value="1" label="HUMAN GATE PENDING" size="sm" accent />}
      </div>
      {sessions && sessions.length > 0 && sessions[0].live && (
        <div style={{ border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', maxHeight: 190, overflowY: 'auto' }}>
          {sessions.map((x) => (
            <div key={x.id} onClick={() => { setSelId(x.id); setSel(6); }}
              style={{ display: 'flex', gap: 12, alignItems: 'center', padding: '7px 12px', cursor: 'pointer', borderBottom: '1px solid var(--divider)',
                background: x.id === (session?.id ?? id) ? 'var(--surface-raised)' : 'transparent', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
              <span style={{ color: 'var(--accent-hover)', width: 64, flex: 'none' }}>{x.id}</span>
              <span style={{ color: 'var(--text-heading)', flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{x.live?.context ?? `${x.agent} · ${x.live?.purpose}`}</span>
              <span style={{ color: x.live?.outcome === 'served' ? 'var(--secure)' : 'var(--warn)', width: 56 }}>{x.live?.outcome?.toUpperCase()}</span>
              <span style={{ color: 'var(--text-muted)', width: 86, textAlign: 'right' }}>{x.live?.tokens ?? 0} TOKENS</span>
              <span style={{ color: 'var(--text-muted)', width: 50, textAlign: 'right' }}>{x.total}</span>
              <span style={{ color: 'var(--text-faint)', width: 74, textAlign: 'right' }}>{x.live ? hm(x.live.at) : ''}</span>
            </div>
          ))}
        </div>
      )}

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
