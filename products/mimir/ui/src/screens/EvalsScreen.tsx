import * as React from 'react';
import {
  Button,
  Card,
  Icon,
  MeterBar,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  StatusPill,
  Table,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { MimirEval, MimirEvalRun, MimirGateway } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

/* Evals — evaluation suites with a 90% regression gate. The KEY FLOW: select a
 * suite and Run it; `runEval` returns per-case pass/fail plus a score, shown
 * live in the inspector with a MeterBar and a gate verdict. `[FR-MI-3, NFR-AI-1]` */

function PassBar({ v, warn }: { v: number; warn: boolean }) {
  return (
    <span style={{ display: 'inline-flex', alignItems: 'center', gap: 8 }}>
      <span style={{ width: 70, height: 5, background: 'var(--surface-inset)', borderRadius: 3, overflow: 'hidden', display: 'inline-block' }}>
        <span style={{ display: 'block', height: '100%', width: v + '%', background: warn ? 'var(--warn)' : 'var(--secure)', borderRadius: 3 }} />
      </span>
      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>{v}%</span>
    </span>
  );
}

const KINDS = ['all kinds', 'capability', 'grounding', 'adversarial', 'calibration'];

export function EvalsScreen() {
  const g = useGateway<MimirGateway>();
  const { data: suites } = useAsync<MimirEval[], MimirGateway>((c) => c.listEvals(), []);
  const rows: MimirEval[] = suites ?? [];

  const [sel, setSel] = React.useState(3);
  const [kind, setKind] = React.useState(0);
  const [run, setRun] = React.useState<MimirEvalRun | null>(null);
  const [running, setRunning] = React.useState(false);

  const s = rows[sel];

  // Clear a stale run when the selected suite changes.
  React.useEffect(() => { setRun(null); }, [sel]);

  const runSuite = async () => {
    if (!s || running) return;
    setRunning(true);
    setRun(null);
    try {
      setRun(await g.runEval(s.id));
    } finally {
      setRunning(false);
    }
  };

  const aside = s ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="gauge" size={17} color="var(--accent)" />
        <h2 style={{ fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 14, flex: 1 }}>{s.id}</h2>
        <StatusPill status={s.status} pulse={false} label={s.statusLabel} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={s.pass + '%'} label="PASS · GATE 90%" size="sm" accent={s.pass < 90} />
          <StatBlock value={s.delta} label="7-DAY DELTA" size="sm" />
        </div>
      </Card>

      <Button variant="primary" size="sm" fullWidth onClick={runSuite} iconLeft={<Icon name="play" size={14} />}>
        {running ? 'Running…' : `Run ${s.id}`}
      </Button>

      {running && (
        <div style={{ padding: '10px 2px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>EVALUATING CASES…</div>
      )}

      {run && (
        <Card size="sm" style={{ borderColor: run.gateClear ? 'var(--secure)' : 'var(--warn)' }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 10 }}>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: run.gateClear ? 'var(--secure)' : 'var(--warn)' }}>{run.gateClear ? 'GATE · CLEAR' : 'GATE · BLOCKING'}</span>
            <span style={{ flex: 1 }} />
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-faint)' }}>{run.passed}/{run.total}</span>
          </div>
          <MeterBar value={run.score} width={150} warnAt={90} dangerAt={85} invert format={(v) => v + '%'} style={{ marginBottom: 12 }} />
          <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 6 }}>CASES</div>
          {run.cases.map((c, i) => (
            <div key={i} style={{ display: 'flex', alignItems: 'flex-start', gap: 8, padding: '5px 0', borderBottom: '1px solid var(--divider)' }}>
              <Icon name={c.passed ? 'check' : 'x'} size={13} color={c.passed ? 'var(--secure)' : 'var(--danger)'} style={{ marginTop: 2 }} />
              <div style={{ flex: 1 }}>
                <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.45 }}>{c.name}</div>
                <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--text-faint)', marginTop: 1 }}>{c.detail}</div>
              </div>
            </div>
          ))}
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--text-faint)', marginTop: 8 }}>AUDIT {run.auditId} · ML-DSA ✓</div>
        </Card>
      )}

      {!run && !running && (
        <div>
          <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>FAILING CASES · TOP 3</div>
          {['multi-leg schedule with crew constraint', 'conflicting maintenance windows', 'risk score on config-only diffs'].map((l, i) => (
            <div key={i} style={{ display: 'flex', gap: 8, padding: '6px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.5 }}>
              <Icon name="x" size={13} color="var(--danger)" style={{ marginTop: 2 }} />{l}
            </div>
          ))}
        </div>
      )}

      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-MI-3" title="Agent runtime with tools, memory, planning, and per-agent Evals" />
        <ProvenanceTag id="NFR-AI-1" title="Traceable, evaluated, reversible or human-gated" />
      </div>
    </>
  ) : null;

  const visible = kind === 0 ? rows : rows.filter((r) => r.kind === KINDS[kind]);

  return (
    <ConsolePage
      active="evals"
      context="MIMIR / EVALS"
      toolbar={<Button variant="primary" size="sm" onClick={runSuite} iconLeft={<Icon name="play" size={14} />}>Run suite</Button>}
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={1}>EVALS</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>Evaluation suites</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value="18" label="SUITES" size="sm" />
        <StatBlock value="93.4%" label="FLEET PASS RATE" size="sm" accent />
        <StatBlock value="2" label="GATES BLOCKING" size="sm" />
      </div>

      <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' }}>
        {KINDS.map((k, i) => (
          <Tag key={k} active={kind === i} onClick={() => setKind(i)}>{k}</Tag>
        ))}
      </div>

      <div style={{ overflowX: 'auto' }}>
        <Table<MimirEval>
          columns={[
            { header: 'Suite', cell: (r) => <span style={{ display: 'flex', alignItems: 'center', gap: 9 }}><Icon name="gauge" size={14} color="var(--accent)" />{r.id}</span> },
            { header: 'Agent', key: 'agent', mono: true, width: 130 },
            { header: 'Kind', key: 'kind', mono: true, width: 110 },
            { header: 'Cases', key: 'cases', mono: true, align: 'right', width: 70 },
            { header: 'Pass rate', cell: (r) => <PassBar v={r.pass} warn={r.pass < 90 || r.status === 'warn'} />, width: 150 },
            { header: 'Δ 7d', key: 'delta', mono: true, align: 'right', width: 70 },
            { header: 'State', cell: (r) => <StatusPill status={r.status} pulse={false} label={r.statusLabel} />, width: 130 },
          ]}
          rows={visible}
          selectedIndex={visible === rows ? sel : undefined}
          onRowClick={(i) => { const target = visible[i]; setSel(rows.indexOf(target)); }}
          style={{ minWidth: 760 }}
        />
      </div>

      <Terminal title="LAST RUN · 14:20" lines={[
        '> mimir evals run --all --model mistral-small-3.2',
        'dispatch-core        412/412 · 96% · 4m02s',
        'dispatch-adversarial 128/128 · 92% · 2m11s ⚠ drift −0.8',
        'risk-calibration      96/96  · 88% · 1m38s ⚠ below gate',
        '# 2 gates blocking · policy updates queued',
      ]} />
    </ConsolePage>
  );
}
