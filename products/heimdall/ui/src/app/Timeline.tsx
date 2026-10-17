/* The replay timeline under the operating picture, shared by the 2D and 3D views.
 * [FR-P3-7] [NFR-P3-3]
 *
 * LIVE (the default) shows the current picture. Scrubbing, or pressing play, enters
 * REPLAY: positions then come from recorded history at the replay time, which plays at
 * 1x to 300x and returns to LIVE when it catches up with now. Replay is labelled in the
 * warning colour so it is never mistaken for the live picture. */
import * as React from 'react';
import type { HistoryState } from './useHistory';

export const WINDOWS: { secs: number; label: string }[] = [
  { secs: 15 * 60, label: '15M' },
  { secs: 3600, label: '1H' },
  { secs: 6 * 3600, label: '6H' },
  { secs: 24 * 3600, label: '24H' },
];
export const SPEEDS = [1, 10, 60, 300];
/** Trail lengths in minutes; 0 is the whole window. */
export const TRAILS = [2, 10, 30, 0];

export interface ReplayClock {
  live: boolean;
  /** The time shown, unix s (now, when live). */
  t: number;
  playing: boolean;
  speed: number;
  goLive: () => void;
  scrub: (t: number) => void;
  setPlaying: (p: boolean) => void;
  setSpeed: (s: number) => void;
}

/** The replay clock. While playing it advances every `tickMs` (wall time × speed). */
export function useReplayClock(windowSecs: number, tickMs: number): ReplayClock {
  const [live, setLive] = React.useState(true);
  const [t, setT] = React.useState(() => Date.now() / 1000);
  const [playing, setPlayingState] = React.useState(false);
  const [speed, setSpeed] = React.useState(60);

  const goLive = React.useCallback(() => {
    setLive(true);
    setPlayingState(false);
    setT(Date.now() / 1000);
  }, []);
  const scrub = React.useCallback((to: number) => {
    const now = Date.now() / 1000;
    if (to >= now - 1) return goLive();
    setLive(false);
    setT(Math.max(now - windowSecs, to));
  }, [goLive, windowSecs]);
  const setPlaying = React.useCallback((p: boolean) => {
    // Play from LIVE replays the window from its start.
    if (p && live) {
      setLive(false);
      setT(Date.now() / 1000 - windowSecs);
    }
    setPlayingState(p);
  }, [live, windowSecs]);

  // Keep the replay time inside a window that has just been narrowed.
  React.useEffect(() => {
    if (!live) setT((x) => Math.max(Date.now() / 1000 - windowSecs, x));
  }, [windowSecs, live]);

  React.useEffect(() => {
    if (live || !playing) return;
    let last = performance.now();
    const timer = window.setInterval(() => {
      const nowMs = performance.now();
      const dt = ((nowMs - last) / 1000) * speed;
      last = nowMs;
      setT((x) => {
        const next = x + dt;
        if (next >= Date.now() / 1000) {
          window.setTimeout(goLive, 0);
          return Date.now() / 1000;
        }
        return next;
      });
    }, tickMs);
    return () => window.clearInterval(timer);
  }, [live, playing, speed, tickMs, goLive]);

  // While live, the clock reads now.
  const shown = live ? Date.now() / 1000 : t;
  return { live, t: shown, playing, speed, goLive, scrub, setPlaying, setSpeed };
}

const utc = (unix: number, secs = true) => new Date(unix * 1000).toISOString().slice(11, secs ? 19 : 16);
const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.1em' };

function Chip({ on, onClick, children, label, tone, disabled }: { on?: boolean; onClick: () => void; children: React.ReactNode; label?: string; tone?: string; disabled?: boolean }) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={on}
      disabled={disabled}
      onClick={onClick}
      style={{
        all: 'unset', boxSizing: 'border-box', cursor: disabled ? 'default' : 'pointer', minWidth: 44, height: 44, padding: '0 10px',
        display: 'inline-flex', alignItems: 'center', justifyContent: 'center', gap: 6, ...mono,
        color: disabled ? 'var(--text-faint)' : on ? (tone ?? 'var(--accent-hover)') : 'var(--text-muted)',
        border: `1px solid ${on ? (tone ?? 'var(--accent)') : 'var(--border)'}`,
        background: on ? 'var(--surface-raised)' : 'transparent', borderRadius: 'var(--radius-sm)',
        opacity: disabled ? 0.5 : 1, touchAction: 'manipulation',
      }}
    >
      {children}
    </button>
  );
}

function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <span role="group" aria-label={label} style={{ display: 'inline-flex', alignItems: 'center', gap: 4 }}>
      <span style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', marginRight: 2 }}>{label}</span>
      {children}
    </span>
  );
}

export function Timeline({ clock, history, windowSecs, setWindow, trailMins, setTrailMins }: {
  clock: ReplayClock;
  history: HistoryState;
  windowSecs: number;
  setWindow: (s: number) => void;
  trailMins: number;
  setTrailMins: (m: number) => void;
}) {
  const now = Date.now() / 1000;
  const from = now - windowSecs;
  const unavailable = history.status === 'unavailable';
  const ticks = [0, 0.25, 0.5, 0.75, 1].map((f) => from + f * windowSecs);
  return (
    <div
      role="region"
      aria-label="Replay timeline"
      style={{
        display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '6px 14px', padding: '6px 10px',
        background: 'color-mix(in srgb, var(--bg-sunken) 88%, transparent)', backdropFilter: 'var(--blur-panel)', WebkitBackdropFilter: 'var(--blur-panel)',
        border: `1px solid ${clock.live ? 'var(--divider)' : 'var(--warn)'}`, borderRadius: 'var(--radius-sm)',
      }}
    >
      <Chip on={clock.live} onClick={clock.goLive} label="Return to live" tone="var(--secure)">
        <span style={{ width: 7, height: 7, borderRadius: '50%', background: clock.live ? 'var(--secure)' : 'var(--text-faint)' }} />LIVE
      </Chip>
      <Chip onClick={() => clock.setPlaying(!clock.playing)} label={clock.playing ? 'Pause replay' : 'Play replay'} on={clock.playing} disabled={unavailable}>
        {clock.playing ? '❚❚' : '▶'}
      </Chip>
      <span aria-live="polite" style={{ ...mono, fontSize: 13, fontWeight: 600, minWidth: 190, color: clock.live ? 'var(--secure)' : 'var(--warn)' }}>
        {clock.live ? 'LIVE' : 'REPLAY'} {utc(clock.t)} UTC
      </span>

      <span style={{ flex: '1 1 260px', minWidth: 220, display: 'flex', flexDirection: 'column', gap: 0 }}>
        <input
          type="range"
          aria-label="Replay time"
          min={Math.floor(from)}
          max={Math.ceil(now)}
          step={1}
          value={Math.round(clock.t)}
          disabled={unavailable}
          onChange={(e) => clock.scrub(Number(e.target.value))}
          style={{ width: '100%', height: 28, margin: 0, accentColor: clock.live ? 'var(--secure)' : 'var(--warn)', touchAction: 'none', cursor: unavailable ? 'default' : 'pointer' }}
        />
        <span style={{ display: 'flex', justifyContent: 'space-between', ...mono, fontSize: 8, color: 'var(--text-faint)' }}>
          {ticks.map((x, i) => <span key={i}>{i === ticks.length - 1 ? 'NOW' : utc(x, false)}</span>)}
        </span>
      </span>

      <Group label="WINDOW">
        {WINDOWS.map((w) => <Chip key={w.secs} on={windowSecs === w.secs} onClick={() => setWindow(w.secs)} label={`Window ${w.label}`}>{w.label}</Chip>)}
      </Group>
      {!clock.live && (
        <Group label="SPEED">
          {SPEEDS.map((s) => <Chip key={s} on={clock.speed === s} onClick={() => clock.setSpeed(s)} label={`Speed ${s}x`}>{s}×</Chip>)}
        </Group>
      )}
      <Group label="TRAIL">
        {TRAILS.map((m) => <Chip key={m} on={trailMins === m} onClick={() => setTrailMins(m)} label={m ? `Trail ${m} minutes` : 'Trail over the whole window'}>{m ? `${m}M` : 'ALL'}</Chip>)}
      </Group>
      <span title={history.message} style={{ ...mono, fontSize: 8, color: unavailable ? 'var(--warn)' : 'var(--text-faint)' }}>
        {unavailable
          ? 'HISTORY UNAVAILABLE · TRAILS FROM THIS SESSION'
          : history.status === 'loading'
            ? 'READING HISTORY…'
            : `HISTORY · ${history.store.size.toLocaleString()} TRACKS${history.truncated ? ' · TRUNCATED' : ''}`}
      </span>
    </div>
  );
}
