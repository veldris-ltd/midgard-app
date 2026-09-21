import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  IconButton,
  Input,
  ProvenanceTag,
  SectionLabel,
  Select,
  StatBlock,
  StatusPill,
} from '@midgard/design-system';
import { useAsync, useMedia } from '@midgard/ui-shared';
import type { MimirAgent, MimirGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

/* Flagship — the OAG (ontology-grounded) agent studio. Agents are sourced from
 * the gateway; the chat is a canned, grounded exchange demonstrating live object
 * citations and a human-gated state mutation. `[FR-MI-2, NFR-AI-1, DD-5]` */

type ChatMsg = { who: 'user' | 'ai'; text: string; cites?: string[]; action?: string };

const CANNED: { cites: string[]; text: string; action: string } = {
  cites: ['Aircraft/AC-2214', 'MaintenanceEvent/ME-88121', 'fn check_slack()'],
  text: 'AC-2214 shows a recurring hydraulic pressure drift across the last 3 legs. Object history links it to open event ME-88121. Recommended action: Schedule maintenance at FAC-DUB within 48h, schedule slack allows it without route impact.',
  action: 'Schedule maintenance · AC-2214 → FAC-DUB',
};

function Msg({ who, children, cites, action, onGate }: { who: 'user' | 'ai'; children: React.ReactNode; cites?: string[]; action?: string | null; onGate?: () => void }) {
  const isUser = who === 'user';
  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: isUser ? 'flex-end' : 'flex-start', gap: 6 }}>
      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.14em', color: 'var(--text-faint)' }}>{isUser ? 'OPERATOR' : 'MIMIR · Mistral Small 3.2 · LOCAL'}</span>
      <div style={{
        maxWidth: 560, padding: '12px 15px', fontSize: 'var(--fs-sm)', lineHeight: 1.6,
        background: isUser ? 'var(--accent-tint)' : 'var(--surface-card)',
        border: '1px solid ' + (isUser ? 'var(--accent)' : 'var(--border)'),
        borderRadius: 'var(--radius-md)', color: 'var(--text-body)',
      }}>
        {children}
        {cites && (
          <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 10 }}>
            {cites.map((c, i) => (
              <span key={i} style={{ display: 'inline-flex', alignItems: 'center', gap: 5, fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--accent-hover)', background: 'var(--surface-inset)', border: '1px solid var(--border)', borderRadius: 4, padding: '2px 7px' }}>
                <Icon name="boxes" size={11} />{c}
              </span>
            ))}
          </div>
        )}
        {action && (
          <div style={{ marginTop: 12, padding: '10px 12px', background: 'var(--surface-inset)', border: '1px solid var(--warn)', borderRadius: 'var(--radius-sm)', display: 'flex', alignItems: 'center', gap: 10 }}>
            <Icon name="zap" size={15} color="var(--warn)" />
            <div style={{ flex: 1 }}>
              <div style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--grey-100)' }}>{action}</div>
              <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--warn)', marginTop: 2 }}>STATE MUTATION · HUMAN GATE REQUIRED</div>
            </div>
            <Button variant="primary" size="sm" onClick={onGate}>Authorise</Button>
            <Button variant="ghost" size="sm">Reject</Button>
          </div>
        )}
      </div>
    </div>
  );
}

const SELF_LOOPS: [string, string][] = [
  ['self-learning', 'governed feedback → retrieval'],
  ['self-healing', 'reroute · retry · rollback'],
  ['self-defending', 'injection filter · tool scope'],
];

export function AgentStudioScreen() {
  const { data: agents } = useAsync<MimirAgent[], MimirGateway>((c) => c.listAgents(), []);
  const rows: MimirAgent[] = agents ?? [];

  const [agentId, setAgentId] = React.useState('dispatch');
  const [msgs, setMsgs] = React.useState<ChatMsg[]>([
    { who: 'user', text: 'Any aircraft trending toward a maintenance risk this week?' },
    { who: 'ai', text: CANNED.text, cites: CANNED.cites, action: CANNED.action },
  ]);
  const [input, setInput] = React.useState('');
  const [thinking, setThinking] = React.useState(false);
  const [gated, setGated] = React.useState(false);
  const endRef = React.useRef<HTMLDivElement>(null);
  const isNarrow = useMedia('(max-width: 1100px)');

  const a = rows.find((x) => x.id === agentId) ?? rows[0] ?? null;

  const send = () => {
    if (!input.trim() || thinking) return;
    setMsgs((m) => [...m, { who: 'user', text: input }]);
    setInput('');
    setThinking(true);
    setTimeout(() => {
      setMsgs((m) => [...m, { who: 'ai', text: 'Grounded answer from the object fabric (OAG): retrieval scoped to your role, markings, and declared purpose. Every citation is a live object reference.', cites: ['ObjectSet/aircraft·risk>0.6', 'Policy/purpose:maintenance'] }]);
      setThinking(false);
    }, 1200);
  };
  React.useEffect(() => { if (endRef.current) endRef.current.scrollTop = endRef.current.scrollHeight; }, [msgs, thinking]);

  const aside = (
    <>
      <SectionLabel number={4} muted>EVALS, {(a?.name ?? '—').toUpperCase()}</SectionLabel>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 14 }}>
        <StatBlock value={(a?.evals ?? 0) + '%'} label="EVAL PASS" size="sm" accent />
        <StatBlock value={a?.firstToken ?? '—'} label="FIRST TOKEN" size="sm" />
        <StatBlock value={String(a?.runsToday ?? 0)} label="RUNS TODAY" size="sm" />
        <StatBlock value={String(a?.humanGates ?? 0)} label="HUMAN GATES" size="sm" />
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>GUARDRAILS</div>
        {([['Prompt-injection filter', 'secure'], ['Tool scoping', 'secure'], ['Exfiltration block', 'secure'], ['Adversarial eval drift', 'warn']] as [string, string][]).map(([g, s], i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)' }}>
            <span style={{ width: 7, height: 7, borderRadius: '50%', background: s === 'secure' ? 'var(--secure)' : 'var(--warn)', flex: 'none' }} />{g}
          </div>
        ))}
      </div>
      <div style={{ marginTop: 'auto', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--text-faint)', lineHeight: 2 }}>
        MODEL PLANE · IN-ENCLOSURE<br />WEIGHTS ML-DSA VERIFIED<br />PROMPTS + MEMORY PQ-SEALED
      </div>
    </>
  );

  return (
    <ConsolePage
      active="agents"
      context="MIMIR / AGENT STUDIO"
      asideWidth={300}
      mainStyle={{ padding: 0, gap: 0, flexDirection: 'row', flexWrap: 'wrap' }}
      toolbar={
        <>
          <Select size="sm" options={[{ value: 'mistral', label: 'Mistral Small 3.2' }, { value: 'nemotron', label: 'Nemotron' }, { value: 'gemma', label: 'Gemma' }]} value="mistral" onChange={() => {}} />
          <IconButton label="Settings"><Icon name="settings" size={17} /></IconButton>
        </>
      }
      aside={aside}
    >
      <div style={{ width: isNarrow ? '100%' : 270, flex: 'none', borderRight: isNarrow ? 'none' : '1px solid var(--border)', borderBottom: isNarrow ? '1px solid var(--border)' : 'none', padding: 14, display: 'flex', flexDirection: isNarrow ? 'row' : 'column', gap: 8, overflowY: 'auto', overflowX: 'auto', alignItems: isNarrow ? 'stretch' : undefined }}>
        <div className="mg-label" style={{ color: 'var(--text-faint)', padding: '2px 4px 6px', display: isNarrow ? 'none' : 'block' }}>AGENTS</div>
        {rows.map((ag) => (
          <Card key={ag.id} size="sm" interactive onClick={() => setAgentId(ag.id)} accentRule={agentId === ag.id} style={{ borderColor: agentId === ag.id ? 'var(--accent)' : undefined, minWidth: isNarrow ? 200 : undefined }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <Icon name="bot" size={15} color="var(--accent)" />
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)' }}>{ag.name}</span>
              <span style={{ marginLeft: 'auto' }}><StatusPill status={ag.status} pulse={false} label={ag.evals + '%'} /></span>
            </div>
            <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', marginTop: 5 }}>{ag.desc}</div>
          </Card>
        ))}
        <div style={{ marginTop: isNarrow ? 0 : 'auto', display: isNarrow ? 'none' : 'block' }}>
          <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>SELF-* LOOPS</div>
          {SELF_LOOPS.map(([k, v], i) => (
            <div key={i} style={{ padding: '6px 0', borderBottom: '1px solid var(--divider)' }}>
              <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--accent-hover)', letterSpacing: '0.06em' }}>{k}</div>
              <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--text-faint)', marginTop: 1 }}>{v}</div>
            </div>
          ))}
        </div>
      </div>

      <div style={{ flex: '1 1 380px', minWidth: 0, display: 'flex', flexDirection: 'column', minHeight: 420 }}>
        <div style={{ padding: '12px 22px', borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
          <SectionLabel number={3}>OAG SESSION</SectionLabel>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)' }}>{a?.name ?? '—'}</span>
          <Badge tone="accent">ONTOLOGY-GROUNDED</Badge>
          {gated && <Badge tone="secure">ACTION AUTHORISED · AUDIT #A-77120</Badge>}
          <div style={{ flex: 1 }} />
          <ProvenanceTag id="FR-MI-2" title="Ground generation in the object fabric with citations" />
          <ProvenanceTag id="NFR-AI-1" title="Every agentic action traceable, evaluated, reversible or human-gated" />
        </div>
        <div ref={endRef} style={{ flex: 1, overflowY: 'auto', padding: '20px 22px', display: 'flex', flexDirection: 'column', gap: 18 }} className="mg-grid-bg">
          {msgs.map((m, i) => (
            <Msg key={i} who={m.who} cites={m.cites} action={!gated && m.action ? m.action : null} onGate={() => setGated(true)}>{m.text}</Msg>
          ))}
          {thinking && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--text-faint)' }}>mimir is retrieving from the fabric…</span>}
        </div>
        <div style={{ padding: '14px 22px', borderTop: '1px solid var(--border)', display: 'flex', gap: 10 }}>
          <Input fullWidth placeholder="Ask across the object fabric… (grounded, cited, governed)" value={input} onChange={(e) => setInput(e.target.value)} onKeyDown={(e) => e.key === 'Enter' && send()} style={{ flex: 1 }} />
          <Button variant="primary" onClick={send} iconRight={<Icon name="arrow-right" size={15} />}>Send</Button>
        </div>
      </div>
    </ConsolePage>
  );
}
