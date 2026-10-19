/* A camera's analysis results over time: vehicles, people and moving vehicles charted
 * across the results kept (the last day), with incidents marked, and WATCH, which asks
 * for the camera to be re-analysed every few minutes for a few hours so the trend builds
 * up. Nothing is analysed unless someone asks or watches. [FR-P3-5] */
import * as React from 'react';
import type { HeimdallCameraTrend, HeimdallGateway } from '@midgard/sdk';

const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em' };
const btn: React.CSSProperties = {
  all: 'unset', cursor: 'pointer', ...mono, padding: '3px 7px', borderRadius: 'var(--radius-sm)',
  border: '1px solid var(--divider)', color: 'var(--text-muted)',
};
const SERIES = [
  { key: 'vehicles', label: 'VEHICLES', colour: '#ffb020' },
  { key: 'moving', label: 'MOVING', colour: '#2fd16b' },
  { key: 'people', label: 'PEOPLE', colour: '#3fe0ff' },
] as const;
const hm = (s: number) => new Date(s * 1000).toISOString().slice(11, 16);

/** `refresh` changes after each analysis asked for here, to read the trend again. */
export function CameraTrend({ gateway, cameraId, refresh }: { gateway: HeimdallGateway; cameraId: string; refresh: number }) {
  const [trend, setTrend] = React.useState<HeimdallCameraTrend | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [tick, setTick] = React.useState(0);
  const watching = !!trend?.watch && trend.watch.until * 1000 > Date.now();

  React.useEffect(() => { setTrend(null); setError(null); }, [cameraId]);
  React.useEffect(() => {
    if (!gateway.cameraTrend) return;
    let live = true;
    gateway.cameraTrend(cameraId).then(
      (t) => { if (live) { setTrend(t); setError(null); } },
      (err) => { if (live) setError(err instanceof Error ? err.message : String(err)); },
    );
    return () => { live = false; };
  }, [gateway, cameraId, refresh, tick]);
  // While watched, read again every minute to pick up the new results.
  React.useEffect(() => {
    if (!watching) return;
    const timer = window.setInterval(() => setTick((n) => n + 1), 60_000);
    return () => window.clearInterval(timer);
  }, [watching]);

  if (!gateway.cameraTrend) return null;
  const watch = (hours: number) => {
    if (!gateway.watchCamera || busy) return;
    setBusy(true);
    gateway.watchCamera(cameraId, hours).then(
      (w) => { setTrend((t) => ({ points: t?.points ?? [], watch: w })); setError(null); setTick((n) => n + 1); },
      (err) => setError(err instanceof Error ? err.message : String(err)),
    ).finally(() => setBusy(false));
  };

  const pts = trend?.points ?? [];
  const now = Date.now() / 1000;
  const t1 = Math.max(now, pts.length ? pts[pts.length - 1].at : now);
  const t0 = Math.min(pts.length ? pts[0].at : t1 - 3600, t1 - 3600);
  const top = Math.max(4, ...pts.flatMap((p) => SERIES.map((s) => p[s.key] ?? 0)));
  const W = 300, H = 64, X = (t: number) => ((t - t0) / (t1 - t0)) * W, Y = (v: number) => H - 4 - (v / top) * (H - 12);
  const incident = (p: HeimdallCameraTrend['points'][number]) => !!(p.blocking || p.wrongWay || p.queue);
  const last = pts[pts.length - 1];

  return (
    <div style={{ marginTop: 10, paddingTop: 8, borderTop: '1px solid var(--divider)' }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6, flexWrap: 'wrap' }}>
        <span className="mg-label" style={{ color: 'var(--text-faint)' }}>TREND · {pts.length} RESULT{pts.length === 1 ? '' : 'S'}</span>
        <span style={{ marginLeft: 'auto', display: 'flex', gap: 6, alignItems: 'center' }}>
          {gateway.watchCamera && (watching
            ? <>
                <span style={{ ...mono, color: 'var(--accent-hover)' }}>WATCHING TO {hm(trend!.watch!.until)}Z</span>
                <button type="button" onClick={() => watch(0)} disabled={busy} style={btn}>STOP</button>
              </>
            : <>
                <span style={{ ...mono, color: 'var(--text-faint)' }}>WATCH</span>
                {[1, 4, 12].map((h) => <button key={h} type="button" onClick={() => watch(h)} disabled={busy} style={btn}>{h} H</button>)}
              </>)}
        </span>
      </div>
      {error && <div style={{ ...mono, color: 'var(--warn)', marginBottom: 4, overflowWrap: 'anywhere' }}>{error.toUpperCase()}</div>}
      {pts.length === 0
        ? <div style={{ ...mono, color: 'var(--text-faint)', lineHeight: 1.6 }}>NO RESULTS IN THE LAST DAY · ANALYSE, OR WATCH TO RE-ANALYSE EVERY {Math.round((trend?.watch?.everyS ?? 300) / 60)} MIN</div>
        : (
          <>
            <svg viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" style={{ width: '100%', height: 64, display: 'block', background: 'rgba(0,0,0,0.18)', borderRadius: 'var(--radius-sm)' }}
              role="img" aria-label={`Camera results from ${hm(t0)} to ${hm(t1)} UTC`}>
              {[0.5, 1].map((f) => <line key={f} x1={0} x2={W} y1={Y(top * f)} y2={Y(top * f)} stroke="var(--divider)" strokeWidth={0.5} vectorEffect="non-scaling-stroke" />)}
              {pts.filter(incident).map((p) => <line key={`i${p.at}`} x1={X(p.at)} x2={X(p.at)} y1={0} y2={H} stroke="#ff3b30" strokeOpacity={0.55} strokeWidth={1} vectorEffect="non-scaling-stroke" />)}
              {SERIES.map((s) => (
                <g key={s.key}>
                  {pts.length > 1 && <polyline fill="none" stroke={s.colour} strokeWidth={1.4} vectorEffect="non-scaling-stroke"
                    points={pts.map((p) => `${X(p.at).toFixed(1)},${Y(p[s.key] ?? 0).toFixed(1)}`).join(' ')} />}
                  {pts.map((p) => <circle key={p.at} cx={X(p.at)} cy={Y(p[s.key] ?? 0)} r={1.6} fill={s.colour} />)}
                </g>
              ))}
            </svg>
            <div style={{ display: 'flex', justifyContent: 'space-between', ...mono, fontSize: 8, color: 'var(--text-faint)', marginTop: 2 }}>
              <span>{hm(t0)}Z</span><span>PEAK {top}</span><span>{hm(t1)}Z</span>
            </div>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '2px 10px', ...mono, marginTop: 4, color: 'var(--text-body)' }}>
              {SERIES.map((s) => (
                <span key={s.key}><span style={{ color: s.colour }}>■</span> {s.label} {last?.[s.key] ?? 0}
                  <span style={{ color: 'var(--text-faint)' }}> (MAX {Math.max(...pts.map((p) => p[s.key] ?? 0))})</span></span>
              ))}
              {pts.some(incident) && <span><span style={{ color: '#ff3b30' }}>|</span> INCIDENT OR QUEUE {pts.filter(incident).length}</span>}
            </div>
          </>
        )}
    </div>
  );
}
