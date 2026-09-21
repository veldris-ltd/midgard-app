import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  Icon,
  MeterBar,
  SectionLabel,
  StatBlock,
  StatusPill,
  Tag,
  Terminal,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { ForgeGateway, ForgePipeline, ForgePipelineNode } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

/* Flagship: the pipeline builder. A DAG of sources → transforms → dataset →
 * projection, sourced from `listPipelines()`, with an interactive `runPipeline`
 * build that steps through staged phases (MeterBar + StatusPill) and streams a
 * signed build log. Reproduces docs/design/ui_kits/forge/index.html. */

const COL_W = 250;
const ROW_H = 150;
const NODE_W = 190;
const NODE_H = 84;
const PAD = 28;

function Node({ n, selected, onClick }: { n: ForgePipelineNode; selected: boolean; onClick: () => void }) {
  const statusColor = n.status === 'warn' ? 'var(--warn)' : 'var(--secure)';
  return (
    <div
      onClick={onClick}
      style={{
        position: 'absolute',
        left: PAD + n.x * COL_W,
        top: PAD + n.y * ROW_H,
        width: NODE_W,
        height: NODE_H,
        background: selected ? 'var(--surface-raised)' : 'var(--surface-card)',
        border: '1px solid ' + (selected ? 'var(--accent)' : 'var(--border)'),
        boxShadow: selected ? 'var(--shadow-glow)' : 'none',
        borderRadius: 'var(--radius-md)',
        padding: '10px 12px',
        cursor: 'pointer',
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
        transition: 'border-color var(--dur-fast) var(--ease-out), box-shadow var(--dur-fast) var(--ease-out)',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <Icon name={n.icon} size={15} color="var(--accent)" />
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--grey-100)', fontWeight: 'var(--fw-medium)' as React.CSSProperties['fontWeight'] }}>{n.label}</span>
        <span style={{ marginLeft: 'auto', width: 7, height: 7, borderRadius: '50%', background: statusColor }} />
      </div>
      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>{n.meta}</span>
      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--text-faint)', textTransform: 'uppercase' }}>{n.kind}</span>
    </div>
  );
}

export function PipelineBuilderScreen() {
  const gateway = useGateway<ForgeGateway>();
  const { data, loading } = useAsync<ForgePipeline[], ForgeGateway>((c) => c.listPipelines(), []);
  const pipelines = data ?? [];

  const [pipeIdx, setPipeIdx] = React.useState(0);
  const pipeline = pipelines[pipeIdx];

  const defaultNode = (p?: ForgePipeline) => p?.nodes.find((n) => n.kind === 'dataset')?.id ?? p?.nodes[0]?.id ?? '';
  const [sel, setSel] = React.useState('');
  React.useEffect(() => {
    setSel(defaultNode(pipeline));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pipeIdx, pipeline?.id]);

  const [log, setLog] = React.useState<string[]>(['> forge build — awaiting selection']);
  const [building, setBuilding] = React.useState(false);
  const [progress, setProgress] = React.useState(0);
  const [phase, setPhase] = React.useState<string>('Idle');
  const timers = React.useRef<Array<ReturnType<typeof setTimeout>>>([]);

  React.useEffect(() => {
    if (pipeline) {
      const last = pipeline.versions[0];
      setLog([`> forge build ${pipeline.name} --branch ${pipeline.branch}`, `# last build ${last?.ts ?? '—'} · ${last?.status ?? '—'} · ${pipeline.currentVersion}`]);
      setProgress(0);
      setPhase('Idle');
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pipeline?.id]);

  React.useEffect(() => () => timers.current.forEach(clearTimeout), []);

  const node = pipeline?.nodes.find((n) => n.id === sel);

  const runBuild = async () => {
    if (building || !pipeline) return;
    setBuilding(true);
    setProgress(0);
    setPhase('Resolving');
    setLog((l) => [...l, `> forge build ${pipeline.name} --branch ${pipeline.branch}`]);
    const res = await gateway.runPipeline(pipeline.id);
    timers.current.forEach(clearTimeout);
    timers.current = [];
    res.steps.forEach((step, i) => {
      const t = setTimeout(() => {
        const pad = (step.stage + '        ').slice(0, 9);
        setLog((l) => [...l, `${step.stage === 'done' ? '# ' : ''}${step.stage === 'done' ? step.detail : pad + step.detail}`]);
        setProgress(Math.round(((i + 1) / res.steps.length) * 100));
        setPhase(step.stage === 'done' ? 'Sealed' : step.stage.charAt(0).toUpperCase() + step.stage.slice(1));
        if (i === res.steps.length - 1) setBuilding(false);
      }, 560 * (i + 1));
      timers.current.push(t);
    });
  };

  const edgePath = (from: string, to: string): string => {
    const na = pipeline?.nodes.find((n) => n.id === from);
    const nb = pipeline?.nodes.find((n) => n.id === to);
    if (!na || !nb) return '';
    const x1 = PAD + na.x * COL_W + NODE_W;
    const y1 = PAD + na.y * ROW_H + NODE_H / 2;
    const x2 = PAD + nb.x * COL_W;
    const y2 = PAD + nb.y * ROW_H + NODE_H / 2;
    const mx = (x1 + x2) / 2;
    return `M ${x1} ${y1} C ${mx} ${y1}, ${mx} ${y2}, ${x2} ${y2}`;
  };

  const aside = node ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 9 }}>
        <Icon name={node.icon} size={17} color="var(--accent)" />
        <h2 style={{ flex: 1, fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 15 }}>{node.label}</h2>
        <StatusPill status={node.status} pulse={false} label={node.status === 'warn' ? 'Drift' : 'Healthy'} />
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={pipeline.currentVersion} label="VERSION" size="sm" />
          <StatBlock value={pipeline.expectations} label="EXPECTATIONS" size="sm" />
        </div>
      </Card>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>VERSION HISTORY</div>
        {pipeline.versions.map((v, i) => (
          <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)' }}>
            <Icon name="git-commit" size={13} color="var(--text-faint)" />
            <span style={{ color: 'var(--grey-100)' }}>{v.version}</span>
            <span style={{ color: 'var(--text-faint)' }}>{v.branch}</span>
            <span style={{ marginLeft: 'auto', color: 'var(--text-faint)' }}>{v.ts}</span>
            <Badge tone={v.status === 'OK' ? 'secure' : 'danger'}>{v.status}</Badge>
          </div>
        ))}
      </div>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>ENCRYPTION</div>
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-muted)', lineHeight: 2 }}>
          AT REST&nbsp;&nbsp;AES-256-GCM · ML-KEM-1024<br />TRANSIT&nbsp;&nbsp;X25519 + ML-KEM-768<br />LINEAGE&nbsp;&nbsp;ML-DSA SIGNED
        </div>
      </div>
      <Button variant="secondary" size="sm" style={{ marginTop: 'auto' }} iconLeft={<Icon name="boxes" size={14} />}>Open projection mapping</Button>
    </>
  ) : null;

  return (
    <ConsolePage
      active="pipeline"
      context={`PIPELINES / ${pipeline?.context ?? 'BUILDER'}`}
      asideWidth={320}
      mainStyle={{ padding: 0, gap: 0 }}
      toolbar={
        <>
          <Badge tone="accent">BRANCH · {(pipeline?.branch ?? 'MAIN').toUpperCase()}</Badge>
          <Button variant="secondary" size="sm" iconLeft={<Icon name="git-branch" size={14} />}>Branch</Button>
          <Button variant="primary" size="sm" iconLeft={<Icon name="play" size={14} />} onClick={runBuild} disabled={building || !pipeline}>{building ? 'Building…' : 'Run build'}</Button>
        </>
      }
      aside={aside}
    >
      <div style={{ flex: 1, position: 'relative', background: 'var(--bg-sunken)', minHeight: 0, overflow: 'auto' }} className="mg-grid-bg">
        <svg style={{ position: 'absolute', inset: 0, width: '100%', height: '100%', pointerEvents: 'none' }}>
          {pipeline?.edges.map((e, i) => <path key={i} d={edgePath(e.from, e.to)} fill="none" stroke="var(--ink-500)" strokeWidth="1.5" />)}
        </svg>
        {pipeline?.nodes.map((n) => <Node key={n.id} n={n} selected={sel === n.id} onClick={() => setSel(n.id)} />)}

        {/* Build status strip + pipeline switcher */}
        <div style={{ position: 'absolute', top: 16, left: 16, display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
          <StatusPill status={building ? 'live' : 'healthy'} pulse={building} label={building ? phase : 'Ready'} />
          <MeterBar value={progress} label="BUILD" width={90} />
          {pipelines.map((p, i) => (
            <Tag key={p.id} active={i === pipeIdx} onClick={() => setPipeIdx(i)}>{p.name}</Tag>
          ))}
        </div>

        <div style={{ position: 'absolute', top: 16, right: 16, display: 'flex', gap: 8 }}>
          <Tag active>low-code</Tag>
          <Tag>pro-code</Tag>
        </div>

        {loading && (
          <div style={{ position: 'absolute', bottom: 16, left: 16, fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: 'var(--text-faint)' }}>RESOLVING PIPELINE GRAPH…</div>
        )}
      </div>

      <div style={{ flex: 'none', borderTop: '1px solid var(--border)', display: 'flex', flexWrap: 'wrap' }}>
        <Terminal title={building ? 'BUILD · RUNNING' : 'BUILD LOG'} lines={log} style={{ flex: '1 1 320px', border: 'none', borderRadius: 0, overflowY: 'auto', maxHeight: 200 }} />
        <div style={{ width: 'min(330px, 100%)', flex: '0 1 auto', borderLeft: '1px solid var(--border)', padding: 16, display: 'flex', flexDirection: 'column', gap: 10 }}>
          <SectionLabel number={4} muted>MIMIR SUGGESTION</SectionLabel>
          <p style={{ margin: 0, fontSize: 'var(--fs-xs)', lineHeight: 1.6, color: 'var(--text-body)' }}>{pipeline?.suggestion ?? '—'}</p>
          <div style={{ display: 'flex', gap: 8, marginTop: 'auto' }}>
            <Button variant="outline" size="sm" fullWidth iconLeft={<Icon name="sparkles" size={13} />}>Preview transform</Button>
            <Button variant="ghost" size="sm">Dismiss</Button>
          </div>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--text-faint)' }}>LOCAL INFERENCE · Mistral Small 3.2 · PREVIEW-BEFORE-COMMIT [FR-F-7]</span>
        </div>
      </div>
    </ConsolePage>
  );
}
