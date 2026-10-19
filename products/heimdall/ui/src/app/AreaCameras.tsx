/* Analyse the cameras in view, one at a time, on request: the nearest to the centre of
 * the view first, at most MAX_RUN a run. Each analysis is an audited action at the
 * gateway and reaches Heimdall, so the cameras' colours and counts on the map follow at
 * the next picture read; the card lists the totals and the busiest cameras meanwhile.
 * [FR-P3-5] [FR-H-1] */
import * as React from 'react';
import { Card } from '@midgard/design-system';
import type { HeimdallCameraAnalysis, HeimdallGateway, HeimdallTrack } from '@midgard/sdk';
import { CAMERA_LEVEL_COLOURS, cameraLevel } from './trackStyle';

const MAX_RUN = 25;
const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em' };

const btn = (on: boolean): React.CSSProperties => ({
  all: 'unset', cursor: 'pointer', ...mono, padding: '4px 8px', borderRadius: 'var(--radius-sm)',
  border: `1px solid ${on ? 'var(--accent)' : 'var(--divider)'}`, color: on ? 'var(--accent-hover)' : 'var(--text-muted)',
});

interface Run {
  running: boolean;
  total: number;
  done: number;
  failed: number;
  lastError: string | null;
  results: Map<string, { track: HeimdallTrack; ai: HeimdallCameraAnalysis }>;
}

/** `cameras`: the camera tracks in view, nearest the centre first. */
export function AreaCamerasCard({ gateway, cameras, select }: { gateway: HeimdallGateway; cameras: HeimdallTrack[]; select: (id: string) => void }) {
  const [run, setRun] = React.useState<Run | null>(null);
  const stop = React.useRef(false);
  // Stop a run when the card goes (the view moved to a selection, or away).
  React.useEffect(() => () => { stop.current = true; }, []);
  if (!gateway.analyseCamera) return null;
  const analyse = gateway.analyseCamera.bind(gateway);

  const start = async () => {
    const list = cameras.slice(0, MAX_RUN).filter((t) => t.detail?.camera?.id);
    stop.current = false;
    const results = new Map<string, { track: HeimdallTrack; ai: HeimdallCameraAnalysis }>();
    setRun({ running: true, total: list.length, done: 0, failed: 0, lastError: null, results });
    for (const t of list) {
      if (stop.current) break;
      try {
        const ai = await analyse(t.detail!.camera!.id);
        results.set(t.id, { track: t, ai });
        setRun((r) => r && { ...r, done: r.done + 1, results: new Map(results) });
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setRun((r) => r && { ...r, done: r.done + 1, failed: r.failed + 1, lastError: msg });
        if (/session expired/i.test(msg)) break;
      }
    }
    setRun((r) => r && { ...r, running: false });
  };

  const analysed = cameras.filter((t) => cameraLevel(t.detail?.camera?.analysis)).length;
  const res = run ? [...run.results.values()] : [];
  const sum = (f: (a: HeimdallCameraAnalysis) => number | undefined) => res.reduce((n, r) => n + (f(r.ai) ?? 0), 0);
  const levels = { light: 0, moderate: 0, heavy: 0 };
  for (const r of res) { const l = cameraLevel(r.ai); if (l) levels[l]++; }
  const busiest = res.slice().sort((a, b) => (b.ai.vehicles ?? 0) - (a.ai.vehicles ?? 0)).slice(0, 6);

  return (
    <Card size="sm">
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6 }}>
        <span className="mg-label" style={{ color: 'var(--text-faint)' }}>AREA CAMERAS</span>
        <span style={{ marginLeft: 'auto', display: 'flex', gap: 6 }}>
          {run?.running
            ? <button type="button" onClick={() => { stop.current = true; }} style={btn(true)}>STOP</button>
            : <button type="button" onClick={start} disabled={cameras.length === 0} style={{ ...btn(false), opacity: cameras.length ? 1 : 0.5 }}>
                ANALYSE {Math.min(cameras.length, MAX_RUN)} IN VIEW
              </button>}
        </span>
      </div>
      <div style={{ ...mono, color: 'var(--text-muted)', lineHeight: 1.6 }}>
        {cameras.length === 0 ? 'NO CAMERAS IN VIEW · ZOOM TO AN AREA WITH CAMERAS' : `${cameras.length} IN VIEW · ${analysed} WITH A RECENT ANALYSIS${cameras.length > MAX_RUN ? ` · NEAREST ${MAX_RUN} TO THE CENTRE A RUN` : ''}`}
      </div>
      {run && (
        <>
          <div style={{ height: 3, background: 'var(--divider)', borderRadius: 2, margin: '6px 0' }}>
            <div style={{ height: 3, width: `${run.total ? (100 * run.done) / run.total : 0}%`, background: 'var(--accent)', borderRadius: 2 }} />
          </div>
          <div style={{ ...mono, color: 'var(--text-body)', lineHeight: 1.7 }}>
            {run.running ? 'ANALYSING' : 'DONE'} {run.done}/{run.total}{run.failed ? ` · ${run.failed} FAILED` : ''}
            {res.length > 0 && <> · {sum((a) => a.vehicles)} VEHICLES · {sum((a) => a.people)} PEOPLE · {res.filter((r) => r.ai.queue).length} QUEUES</>}
          </div>
          {res.length > 0 && (
            <div style={{ display: 'flex', gap: 10, ...mono, marginTop: 2 }}>
              {(['heavy', 'moderate', 'light'] as const).map((l) => (
                <span key={l} style={{ color: CAMERA_LEVEL_COLOURS[l] }}>■ {l.toUpperCase()} {levels[l]}</span>
              ))}
            </div>
          )}
          {run.lastError && <div style={{ ...mono, color: 'var(--warn)', marginTop: 4, overflowWrap: 'anywhere' }}>LAST FAILURE · {run.lastError.toUpperCase()}</div>}
          {busiest.length > 0 && (
            <div style={{ marginTop: 6, display: 'grid', gap: 2 }}>
              {busiest.map(({ track, ai }) => {
                const l = cameraLevel(ai);
                return (
                  <button key={track.id} type="button" onClick={() => select(track.id)}
                    style={{ all: 'unset', cursor: 'pointer', display: 'flex', gap: 8, ...mono, fontSize: 9.5, padding: '2px 0', color: 'var(--text-body)' }}>
                    <span style={{ color: l ? CAMERA_LEVEL_COLOURS[l] : 'var(--text-faint)' }}>■</span>
                    <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{(track.detail?.name || track.id).toUpperCase()}</span>
                    <span>{ai.vehicles ?? 0}V{ai.people ? ` ${ai.people}P` : ''}{ai.queue ? ' Q' : ''}</span>
                  </button>
                );
              })}
            </div>
          )}
        </>
      )}
      <div style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', marginTop: 6, lineHeight: 1.6 }}>
        ONE CAMERA AT A TIME · EACH IS AN AUDITED ACTION · MAP COLOURS: GREEN LIGHT, AMBER MODERATE, RED HEAVY
      </div>
    </Card>
  );
}
