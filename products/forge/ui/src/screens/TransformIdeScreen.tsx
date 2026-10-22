import * as React from 'react';
import {
  Badge,
  Button,
  Card,
  HumanGate,
  Icon,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import type { ForgeGateway, ForgeTransform, ForgeTransformDiff } from '@midgard/sdk';
import { ForgeHumanGateRequiredError } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

type GateState = { state: 'pending' | 'authorised' | 'rejected'; auditId?: string } | null;

const HUNK_COLOR: Record<ForgeTransformDiff['hunks'][number]['op'], string> = {
  '+': 'var(--secure)',
  '-': 'var(--danger)',
  '~': 'var(--warn)',
};

/* Transform IDE: pro-code transforms over the same dataset contracts as the
 * visual builder. The key flow is preview-before-commit — `previewTransform`
 * renders a sandboxed diff, and the commit is a human-gated `applyTransform`.
 * Reproduces forge/ide.html. */
export function TransformIdeScreen() {
  const gateway = useGateway<ForgeGateway>();
  const { data, loading } = useAsync<ForgeTransform, ForgeGateway>((c) => c.getTransform(), []);

  const [fileIdx, setFileIdx] = React.useState(0);
  const [diff, setDiff] = React.useState<ForgeTransformDiff | null>(null);
  const [gate, setGate] = React.useState<GateState>(null);
  const [busy, setBusy] = React.useState(false);

  const t = data;
  const live = gateway.live === true;
  const files = t?.files ?? [];
  // Live: every job's definition is browsable; open the one the gateway chose first.
  React.useEffect(() => {
    if (t?.codeByFile) setFileIdx(Math.max(0, files.indexOf(t.activeFile)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t?.activeFile]);
  const code = (t?.codeByFile && t.codeByFile[files[fileIdx]]) ?? t?.code ?? [];

  const preview = async () => {
    if (!t || busy) return;
    setBusy(true);
    try {
      const res = await gateway.previewTransform(t.code.join('\n'));
      setDiff(res.diff);
      setGate({ state: 'pending' });
    } finally {
      setBusy(false);
    }
  };

  const authorise = async () => {
    if (!t || busy) return;
    setBusy(true);
    try {
      const res = await gateway.applyTransform(t.code.join('\n'), 'HG-forge-ide');
      setGate({ state: 'authorised', auditId: res.auditId });
    } catch (e) {
      if (e instanceof ForgeHumanGateRequiredError) setGate({ state: 'pending' });
    } finally {
      setBusy(false);
    }
  };

  const aside = t ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
        <Icon name="code" size={17} color="var(--accent)" />
        <h2 style={{ fontSize: 15, flex: 1 }}>{live ? 'Declared, typed, governed' : 'Same contracts as the builder'}</h2>
      </div>
      <p style={{ margin: 0, fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', lineHeight: 1.6 }}>
        {live
          ? 'Each Forge job is declared in the jobs file: where it reads from, its typed columns, and how rows become Midgard objects and links, with their marking and purposes. The worker checks every row against these types before anything is synced, and the gateway checks every sync against the ontology.'
          : 'Pro-code transforms compile to the same typed execution plan as low-code graphs; a dataset can mix both in one pipeline. Type errors surface before run, not after.'}
      </p>
      <div>
        <div className="mg-label" style={{ color: 'var(--text-faint)', marginBottom: 8 }}>{live ? `CONTRACT · ${t.id.toUpperCase()}` : 'CONTRACT CHECK'}</div>
        {t.contractChecks.map(([k, v]) => (
          <div key={k} style={{ display: 'flex', gap: 10, padding: '7px 0', borderBottom: '1px solid var(--divider)', fontSize: 'var(--fs-xs)' }}>
            <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--text-faint)', width: 100, flex: 'none' }}>{k.toUpperCase()}</span>
            <span style={{ color: 'var(--text-body)' }}>{v}</span>
          </div>
        ))}
      </div>
      {diff && (
        <Card size="sm" style={{ borderColor: 'var(--secure)' }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--secure)', marginBottom: 6 }}>PREVIEW · SANDBOXED</div>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: 'var(--text-body)', lineHeight: 1.7 }}>
            rows in {diff.rowsIn.toLocaleString()} → out {diff.rowsOut.toLocaleString()}<br />
            expectations {diff.expectations} ok · {diff.durationLabel}<br />
            nothing committed
          </div>
          <div style={{ marginTop: 10, display: 'flex', flexDirection: 'column', gap: 4 }}>
            {diff.hunks.map((h, i) => (
              <div key={i} style={{ fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-micro)', color: HUNK_COLOR[h.op], display: 'flex', gap: 6 }}>
                <span style={{ flex: 'none' }}>{h.op}</span>
                <span>{h.text}</span>
              </div>
            ))}
          </div>
        </Card>
      )}
      {gate && (
        <HumanGate
          title={`Commit ${t.nextVersion} · fuse_positions`}
          detail="STATE MUTATION · PREVIEW APPROVED · HUMAN GATE"
          state={gate.state}
          auditId={gate.auditId}
          onAuthorise={busy ? undefined : authorise}
          onReject={() => setGate({ state: 'rejected' })}
        />
      )}
      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-F-1" title="Low-code builder and pro-code IDE over identical dataset contracts" />
        <ProvenanceTag id="FR-F-3" title="Versioned with lineage; reproducible rebuilds" />
      </div>
    </>
  ) : null;

  return (
    <ConsolePage
      active="ide"
      context={live ? 'DATA OPS / JOB DEFINITIONS' : 'DATA OPS / TRANSFORM IDE · DEV/FUSE-FIX'}
      toolbar={
        live ? (
          <Badge tone="accent">READ-ONLY · DECLARED IN JOBS.JSON</Badge>
        ) : (
          <>
            <Badge tone="accent">BRANCH · {(t?.branch ?? 'DEV').toUpperCase()}</Badge>
            <Button variant="secondary" size="sm" onClick={preview} disabled={busy || !t} iconLeft={<Icon name="sparkles" size={14} />}>Preview run</Button>
            <Button variant="primary" size="sm" onClick={preview} disabled={busy || !t} iconLeft={<Icon name="git-commit" size={14} />}>Commit {t?.nextVersion ?? ''}</Button>
          </>
        )
      }
      aside={aside}
    >
      <div style={{ display: 'flex', alignItems: 'flex-end', gap: '12px 24px', flexWrap: 'wrap' }}>
        <div>
          <SectionLabel number={7}>PRO-CODE</SectionLabel>
          <h1 style={{ fontSize: 'var(--fs-h3)', marginTop: 6 }}>{live ? 'Job definitions' : 'Transform IDE'}</h1>
        </div>
        <div style={{ flex: 1 }} />
        <StatBlock value={String(t?.filesCount ?? '—')} label={live ? 'JOBS' : 'FILES ON BRANCH'} size="sm" />
        {!live && <StatBlock value={t?.expectations ?? '—'} label="EXPECTATIONS" size="sm" accent />}
        {!live && <StatBlock value={t?.nextVersion ?? '—'} label="NEXT VERSION" size="sm" />}
      </div>

      <div style={{ flex: 1, display: 'flex', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', overflow: 'hidden', minHeight: 320 }}>
        <div style={{ width: 200, flex: 'none', background: 'var(--bg-sunken)', borderRight: '1px solid var(--border)', padding: 10 }}>
          <div className="mg-label" style={{ color: 'var(--text-faint)', padding: '2px 6px 8px' }}>TRANSFORMS</div>
          {files.map((f, i) => (
            <button
              key={f}
              type="button"
              onClick={() => setFileIdx(i)}
              style={{ all: 'unset', boxSizing: 'border-box', display: 'flex', alignItems: 'center', gap: 8, width: '100%', padding: '7px 8px', borderRadius: 6, cursor: 'pointer', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', color: i === fileIdx ? 'var(--text-heading)' : 'var(--text-muted)', background: i === fileIdx ? 'var(--accent-tint)' : 'transparent', boxShadow: i === fileIdx ? 'inset 2px 0 0 var(--accent)' : 'none' }}
            >
              <Icon name="file-text" size={13} />{f}
            </button>
          ))}
        </div>
        <div style={{ flex: 1, background: 'var(--bg-sunken)', padding: '14px 18px', fontFamily: 'var(--font-mono)', fontSize: 'var(--fs-xs)', lineHeight: 1.9, color: 'var(--grey-300)', overflow: 'auto' }}>
          {code.map((l, i) => (
            <div key={i} style={{ display: 'flex', gap: 14 }}>
              <span style={{ color: 'var(--text-faint)', width: 18, textAlign: 'right', flex: 'none', userSelect: 'none' }}>{i + 1}</span>
              <span style={{ whiteSpace: 'pre', color: l.trimStart().startsWith('#') || l.includes('# bounds') ? 'var(--text-faint)' : l.startsWith('@') ? 'var(--accent-hover)' : 'inherit' }}>{l}</span>
            </div>
          ))}
          {loading && <div style={{ color: 'var(--text-faint)' }}>LOADING TRANSFORM…</div>}
        </div>
      </div>
    </ConsolePage>
  );
}
