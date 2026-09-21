import * as React from 'react';
import {
  Badge,
  BreakGlass,
  Button,
  Card,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  Table,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { AdminBreakGlass, AdminBreakGlassState, AdminGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';
import { ConsoleToolbar } from '../app/ConsoleToolbar';

/* Admin · Break-Glass — emergency elevation: time-boxed, justified, alerted,
 * reviewed (FR-AD-8, AC-AD-8). Request → time-boxed active → post-hoc review.
 * Reproduces docs/design/ui_kits/admin/breakglass.html; uses the `BreakGlass`
 * DS component and `getBreakGlass()` / `requestBreakGlass()`. */

const LIFECYCLE: Array<readonly [string, string]> = [
  ['Request + justification', 'mandatory, written'],
  ['Dual-control open', 'second officer approves'],
  ['Time-boxed grant', 'never wider than the emergency'],
  ['Real-time alerts', 'all security officers paged'],
  ['Auto-expiry', 'hard stop at the box'],
  ['Post-hoc review', 'forced · itself audited'],
];

const ALERTS = [
  { o: 'a.novak', c: 'page + console', ack: true },
  { o: 'j.benjamin', c: 'page + console', ack: true },
  { o: 'security-duty', c: 'ops channel', ack: false },
];

export function BreakGlassScreen() {
  const g = useGateway<AdminGateway>();
  const { data } = useAsync<AdminBreakGlass, AdminGateway>((c) => c.getBreakGlass(), []);
  const [phase, setPhase] = React.useState<AdminBreakGlassState>('review');
  const [session, setSession] = React.useState<AdminBreakGlass | null>(null);

  React.useEffect(() => {
    if (data) {
      setSession(data);
      setPhase(data.state);
    }
  }, [data]);

  // Requesting break-glass opens a time-boxed, dual-controlled active session.
  const request = async () => {
    const res = await g.requestBreakGlass('HSM failover mid-rotation: primary HSM unreachable, rotation window closes 07:00. Need unwrap capability to complete re-wrap on secondary.');
    setSession(res);
    setPhase(res.state);
  };

  const holder = session?.holder ?? 'k.osei';
  const scope = session?.scope ?? 'KMS · unwrap only · enclosure-root-03';
  const remaining = session?.remaining ?? '00:41:12';
  const justification =
    phase !== 'idle'
      ? session?.justification ?? 'HSM failover mid-rotation: primary HSM unreachable, rotation window closes 07:00. Need unwrap capability to complete re-wrap on secondary.'
      : undefined;

  const badgeTone = phase === 'active' ? 'danger' : phase === 'review' ? 'warn' : 'neutral';
  const badgeText = phase === 'active' ? '1 ACTIVE' : phase === 'review' ? '1 REVIEW DUE' : 'SEALED';

  const aside = (
    <>
      <SectionLabel number={7} muted>LIFECYCLE</SectionLabel>
      {LIFECYCLE.map(([t, d], i) => (
        <div key={i} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)' }}>
          <span style={{ fontFamily: 'var(--font-display)', fontWeight: 'var(--fw-black)' as React.CSSProperties['fontWeight'], fontSize: 13, color: 'var(--text-faint)', width: 18, flex: 'none' }}>{String(i + 1).padStart(2, '0')}</span>
          <div>
            <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-heading)', fontWeight: 'var(--fw-semibold)' as React.CSSProperties['fontWeight'] }}>{t}</div>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9.5, color: 'var(--text-faint)', marginTop: 2 }}>{d.toUpperCase()}</div>
          </div>
        </div>
      ))}
      <Card size="sm">
        <div className="mg-label" style={{ color: 'var(--text-faint)' }}>ABUSE GUARDS</div>
        <div style={{ display: 'flex', gap: 14, marginTop: 8 }}>
          <StatBlock value="2/mo" label="RATE LIMIT" size="sm" />
          <StatBlock value="3" label="USES · 12MO" size="sm" />
          <StatBlock value="0" label="OVERDUE REVIEWS" size="sm" />
        </div>
      </Card>
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-AD-8" title="Time-boxed, justified, alerted break-glass with forced post-hoc review" />
        <ProvenanceTag id="AC-AD-8" title="Granted break-glass is time-boxed, alerted in real time, review forced" />
        <ProvenanceTag id="AC-AD-12" title="Break-glass and audit work fully offline" />
      </div>
    </>
  );

  return (
    <ConsolePage
      active="breakglass"
      context="ADMIN / BREAK-GLASS"
      asideWidth={340}
      toolbar={<><Badge tone={badgeTone}>{badgeText}</Badge><ConsoleToolbar /></>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={6}>EMERGENCY</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Break-glass console</h1>
        </div>
        <div style={{ flex: 1 }} />
        <div style={{ display: 'flex', gap: 6 }}>
          {(['active', 'review', 'idle'] as const).map((k) => (
            <Tag key={k} active={phase === k} onClick={() => setPhase(k)}>{k === 'idle' ? 'Sealed' : k === 'active' ? 'Active' : 'Review'}</Tag>
          ))}
        </div>
      </div>

      <BreakGlass state={phase} holder={holder} scope={scope} remaining={remaining} justification={justification} />

      {phase === 'idle' && (
        <Card size="sm">
          <div className="mg-label" style={{ color: 'var(--text-faint)' }}>OPEN AN EMERGENCY ELEVATION · DUAL-CONTROL</div>
          <p style={{ margin: '8px 0 0', fontSize: 'var(--fs-sm)', color: 'var(--text-body)', lineHeight: 1.6, maxWidth: 620 }}>Opening break-glass requires a written justification and a second officer&apos;s approval. The grant is time-boxed and never wider than the emergency requires.</p>
          <div style={{ marginTop: 12 }}>
            <Button variant="primary" size="sm" onClick={request}>Request break-glass · second officer</Button>
          </div>
        </Card>
      )}

      {phase === 'active' && (
        <div style={{ display: 'flex', gap: 12, flexWrap: 'wrap' }}>
          <Card size="sm" style={{ flex: 1, minWidth: 260 }}>
            <div className="mg-label" style={{ color: 'var(--text-faint)' }}>LIVE SESSION CONSTRAINTS</div>
            {([
              ['Scope', 'unwrap only · one key family'],
              ['Excluded', 'no policy edits · no IAM · no export'],
              ['Session recording', 'every call ledgered live'],
              ['Hard stop', '07:00:00 · no extension without re-approval'],
            ] as const).map(([k, v]) => (
              <div key={k} style={{ display: 'flex', gap: 10, padding: '6px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
                <span style={{ color: 'var(--text-faint)', width: 120, flex: 'none' }}>{k.toUpperCase()}</span>
                <span style={{ color: 'var(--text-heading)' }}>{v}</span>
              </div>
            ))}
          </Card>
          <Card size="sm" style={{ flex: 1, minWidth: 260 }}>
            <div className="mg-label" style={{ color: 'var(--text-faint)' }}>ALERTS FIRED · REAL-TIME</div>
            <Table<typeof ALERTS[number]>
              compact
              style={{ marginTop: 8, border: 'none' }}
              columns={[
                { header: 'Officer', key: 'o', mono: true },
                { header: 'Channel', key: 'c', mono: true, width: 110 },
                { header: 'Ack', cell: (r) => <Badge tone={r.ack ? 'secure' : 'warn'}>{r.ack ? 'ACK' : 'PENDING'}</Badge>, width: 90 },
              ]}
              rows={ALERTS}
            />
          </Card>
        </div>
      )}

      {phase === 'review' && (
        <Card size="sm">
          <div className="mg-label" style={{ color: 'var(--warn)' }}>POST-HOC REVIEW · DUE IN 18H · FORCED</div>
          <p style={{ margin: '8px 0 0', fontSize: 'var(--fs-sm)', color: 'var(--text-body)', lineHeight: 1.65, maxWidth: 680 }}>Session transcript: 4 unwrap calls, 0 out-of-scope attempts, closed 12 minutes early. Reviewer must confirm the emergency was real, the scope minimal, and the actions proportionate. The review itself becomes a signed audit entry.</p>
          <div style={{ display: 'flex', gap: 8, marginTop: 12 }}>
            <Button variant="primary" size="sm">Open review · second officer</Button>
            <Button variant="ghost" size="sm">Export transcript</Button>
          </div>
        </Card>
      )}

      <Terminal
        title="LEDGER · BREAK-GLASS CLASS"
        lines={[
          '> admin audit tail --class break-glass',
          '#B-0331  opened   k.osei · dual-control a.novak · box 60m',
          '#B-0331  call     kms.unwrap enclosure-root-03 · in scope',
          '#B-0331  closed   47m52s · auto-review queued',
          '# every entry ML-DSA signed · offline capable',
        ]}
      />
    </ConsolePage>
  );
}
