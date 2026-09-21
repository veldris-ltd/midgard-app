import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  Input,
  ProvenanceTag,
  RiskBadge,
  SectionLabel,
  StatBlock,
} from '@midgard/design-system';
import { useAsync } from '@midgard/ui-shared';
import type { AdminCopilotTurn, AdminGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Admin · Copilot — advisory-only: drafts, explains, simulates, NEVER executes
 * (FR-AD-10). Refusals carry a visible, named code. Reproduces
 * docs/design/ui_kits/admin/copilot.html; thread from `getCopilotThread()`. */

const ASSISTANTS: Array<readonly [string, string, string]> = [
  ['Admin copilot', 'drafts proposed changes · explains state · cites resource ids', 'FR-AD-10'],
  ['Sim explainer', 'turns a policy-sim diff into plain impact language', 'FR-AD-6'],
  ['Audit investigator', 'read-only ledger Q&A · every finding cites [audit:id]', 'FR-AD-9'],
];

const REFUSAL_CODES: Array<readonly [string, string]> = [
  ['CONTROL_BYPASS_REFUSED', 'asked to skip dual-control or audit'],
  ['OUT_OF_SCOPE', 'task exceeds the admin’s role; names the required role'],
];

function AdminTurn({ turn }: { turn: AdminCopilotTurn }) {
  if (turn.role === 'admin') {
    return (
      <div style={{ alignSelf: 'flex-end', maxWidth: 520 }}>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--text-faint)', marginBottom: 5, textAlign: 'right' }}>PLATFORM ADMIN · J.BENJAMIN</div>
        <div style={{ padding: '11px 14px', background: 'var(--accent-tint)', border: '1px solid var(--accent)', borderRadius: 'var(--radius-md)', fontSize: 'var(--fs-sm)', color: 'var(--text-body)' }}>{turn.text}</div>
      </div>
    );
  }
  if (turn.variant === 'refusal') {
    return (
      <div style={{ alignSelf: 'flex-start', maxWidth: 620 }}>
        <div style={{ padding: '12px 15px', background: 'var(--surface-card)', border: '1px solid var(--danger)', borderRadius: 'var(--radius-md)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.08em', color: 'var(--danger)', marginBottom: 5 }}>REFUSAL · {turn.refusalCode}</div>
          <p style={{ margin: 0, fontSize: 'var(--fs-sm)', color: 'var(--text-body)', lineHeight: 1.6 }}>{turn.text}</p>
        </div>
      </div>
    );
  }
  // proposal
  return (
    <div style={{ alignSelf: 'flex-start', maxWidth: 620 }}>
      <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--text-faint)', marginBottom: 5 }}>COPILOT · ADVISORY · CITED</div>
      <div style={{ padding: '12px 15px', background: 'var(--surface-card)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)' }}>
        <p style={{ margin: 0, fontSize: 'var(--fs-sm)', color: 'var(--text-body)', lineHeight: 1.6 }}>{turn.text}</p>
        <div style={{ marginTop: 10, padding: '9px 11px', background: 'var(--surface-inset)', border: '1px solid var(--border)', borderRadius: 'var(--radius-sm)' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-heading)' }}>PROPOSED CHANGE · NOT EXECUTED</span>
            {turn.proposalRisk && <RiskBadge level={turn.proposalRisk} dualControl={false} />}
            {turn.proposalNote && <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--text-faint)' }}>{turn.proposalNote}</span>}
          </div>
          <div style={{ display: 'flex', gap: 8, marginTop: 9 }}>
            <Button variant="primary" size="sm">Simulate → review → commit</Button>
            <Button variant="ghost" size="sm">Discard</Button>
          </div>
        </div>
      </div>
    </div>
  );
}

export function CopilotScreen() {
  const { data } = useAsync<AdminCopilotTurn[], AdminGateway>((c) => c.getCopilotThread(), []);
  const [input, setInput] = React.useState('');
  const thread: AdminCopilotTurn[] = data ?? [];

  const aside = (
    <>
      <SectionLabel number={9} muted>THE THREE ASSISTANTS</SectionLabel>
      {ASSISTANTS.map(([t, d, p]) => (
        <Card key={t} size="sm">
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <Icon name="sparkles" size={14} color="var(--accent)" />
            <span style={{ fontSize: 'var(--fs-sm)', fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'], color: 'var(--text-heading)', flex: 1 }}>{t}</span>
            <ProvenanceTag id={p} />
          </div>
          <p style={{ margin: '6px 0 0', fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', lineHeight: 1.5 }}>{d}</p>
        </Card>
      ))}
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>REFUSAL CODES</div>
        {REFUSAL_CODES.map(([code, d]) => (
          <div key={code} style={{ padding: '6px 0', borderBottom: '1px solid var(--divider)' }}>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--danger)' }}>{code}</div>
            <div style={{ fontSize: 'var(--fs-micro)', fontFamily: 'var(--font-mono)', color: 'var(--text-faint)', marginTop: 2 }}>{d.toUpperCase()}</div>
          </div>
        ))}
      </div>
      <div style={{ marginTop: 'auto', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--text-faint)', lineHeight: 2 }}>
        RUNS ON MIMIR · IN-ENCLOSURE<br />EVALS: EV-AD1-ADVISORY … EV-AD3-PATTERN<br />ALL OUTPUT JSON-CONTRACTED
      </div>
    </>
  );

  return (
    <ConsolePage
      active="copilot"
      context="ADMIN / COPILOT · ADVISORY ONLY"
      asideWidth={330}
      toolbar={<><Badge tone="info">CANNOT EXECUTE</Badge><ConsoleToolbar /></>}
      aside={aside}
      mainStyle={{ minHeight: 0 }}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={8}>COPILOT</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Advisory copilot</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="0" label="ACTIONS EXECUTED BY AI · EVER" size="sm" accent />
        <StatBlock value="184" label="DRAFTS · 30D" size="sm" />
        <StatBlock value="12" label="REFUSALS · 30D" size="sm" />
      </div>

      <div style={{ flex: 1, background: 'var(--bg-sunken)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', padding: 20, display: 'flex', flexDirection: 'column', gap: 16, overflowY: 'auto', minHeight: 240 }}>
        {thread.map((turn, i) => <AdminTurn key={i} turn={turn} />)}
      </div>

      <div style={{ display: 'flex', gap: 10 }}>
        <Input
          fullWidth
          placeholder="Ask about state, draft a change, investigate audit… (advisory only)"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          style={{ flex: 1 }}
        />
        <Button variant="primary" iconRight={<Icon name="arrow-right" size={15} />}>Send</Button>
      </div>
    </ConsolePage>
  );
}
