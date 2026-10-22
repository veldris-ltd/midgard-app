/* Console-wide live status for every Heimdall screen: the header seals and nav badges
 * worked out from the picture (not fixed figures), the open alerts behind the bell,
 * small toasts, and the wall-display switch. [FR-H-1] [FR-H-5] [FR-P3-7]
 *
 * Header values:
 *   FEEDS     the distinct sources that contributed an entity to the current picture.
 *   PQ-AUTH ✓ shown only when every entity has at least one source and none of them is a
 *             feed whose source signature failed (listFeeds `auth: false`); Heimdall
 *             admits an observation only after its ML-DSA signature verifies.
 *   SYNC      "SYNC · LOCAL": there is no multi-site replication behind this console yet.
 *   Kp · G    NOAA SWPC space weather, coloured by storm level.
 * Read every 15 s (the live client fetches the picture at most that often anyway). */
import * as React from 'react';
import { useGateway, type Seal } from '@midgard/ui-shared';
import { HeimdallAlertsUnavailableError } from '@midgard/sdk';
import type { HeimdallAlert, HeimdallGateway, HeimdallSpaceWeather } from '@midgard/sdk';
import { alertText } from './alertMarks';

const POLL_MS = 15_000;

export interface Toast {
  id: number;
  text: string;
  tone: 'danger' | 'warn' | 'info';
}

export interface HeimdallStatus {
  /** Distinct contributing sources, or null before the first read. */
  feeds: number | null;
  allSigned: boolean;
  entities: number | null;
  spaceWeather: HeimdallSpaceWeather | null;
  /** Open alerts, newest first (null: not read yet). */
  alerts: HeimdallAlert[] | null;
  /** Why alerts cannot be read, when they cannot. */
  alertsError: string | null;
  refreshAlerts: () => void;
  ackAlert: (id: string) => Promise<HeimdallAlert | null>;
  /** The alerts drawer, opened from any screen's bell. */
  alertsOpen: boolean;
  setAlertsOpen: (open: boolean) => void;
  toasts: Toast[];
  toast: (text: string, tone?: Toast['tone']) => void;
  wall: boolean;
  setWall: (on: boolean) => void;
}

const Ctx = React.createContext<HeimdallStatus | null>(null);

export function useStatus(): HeimdallStatus {
  const c = React.useContext(Ctx);
  if (!c) throw new Error('useStatus must be used within <HeimdallStatusProvider>');
  return c;
}

/** `?wall=1` in the page query or in the hash route's query. */
function wallFromUrl(): boolean {
  try {
    const q = new URLSearchParams(window.location.search);
    const h = new URLSearchParams(window.location.hash.split('?')[1] ?? '');
    return q.get('wall') === '1' || h.get('wall') === '1';
  } catch {
    return false;
  }
}

export function HeimdallStatusProvider({ children }: { children: React.ReactNode }) {
  const gateway = useGateway<HeimdallGateway>();
  const [summary, setSummary] = React.useState<{ feeds: number; allSigned: boolean; entities: number; sw: HeimdallSpaceWeather | null } | null>(null);
  const [alerts, setAlerts] = React.useState<HeimdallAlert[] | null>(null);
  const [alertsError, setAlertsError] = React.useState<string | null>(null);
  const [alertsOpen, setAlertsOpen] = React.useState(false);
  const [toasts, setToasts] = React.useState<Toast[]>([]);
  const [wall, setWall] = React.useState(wallFromUrl);
  const seenCritical = React.useRef<Set<string> | null>(null);

  const toast = React.useCallback((text: string, tone: Toast['tone'] = 'info') => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t.slice(-3), { id, text, tone }]);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 7000);
  }, []);

  // The picture summary behind the header seals and nav badges.
  React.useEffect(() => {
    let live = true;
    const read = async () => {
      try {
        const [p, feeds] = await Promise.all([gateway.getPicture(), gateway.listFeeds().catch(() => [])]);
        if (!live) return;
        const geo = p.geo ?? p.tracks;
        const sources = new Set<string>();
        let unsourced = 0;
        for (const t of geo) {
          const s = t.detail?.sources;
          if (!s) continue;
          if (!s.length) unsourced++;
          for (const x of s) sources.add(x);
        }
        const failed = new Set(feeds.filter((f) => !f.auth).map((f) => f.name));
        const allSigned = sources.size > 0 && unsourced === 0 && ![...sources].some((s) => failed.has(s));
        setSummary({ feeds: sources.size, allSigned, entities: p.entities.length, sw: p.spaceWeather ?? null });
      } catch {
        /* keep the last summary; the screen shows the picture's own error */
      }
    };
    void read();
    const timer = window.setInterval(read, POLL_MS);
    return () => { live = false; window.clearInterval(timer); };
  }, [gateway]);

  // Open alerts, with a toast for each new critical one.
  const [nonce, setNonce] = React.useState(0);
  const refreshAlerts = React.useCallback(() => setNonce((n) => n + 1), []);
  React.useEffect(() => {
    let live = true;
    const read = async () => {
      try {
        const list = await gateway.getAlerts('open');
        if (!live) return;
        const crit = list.filter((a) => a.severity === 'critical' && !a.acknowledgedBy);
        if (seenCritical.current) {
          for (const a of crit) if (!seenCritical.current.has(a.id)) toast(`CRITICAL · ${alertText(a.label)} · ${a.entity}`, 'danger');
        }
        seenCritical.current = new Set([...(seenCritical.current ?? []), ...crit.map((a) => a.id)]);
        setAlerts(list);
        setAlertsError(null);
      } catch (err) {
        if (!live) return;
        setAlertsError(err instanceof HeimdallAlertsUnavailableError ? 'alerts unavailable' : err instanceof Error ? err.message : String(err));
      }
    };
    void read();
    const timer = window.setInterval(read, POLL_MS);
    return () => { live = false; window.clearInterval(timer); };
  }, [gateway, toast, nonce]);

  const ackAlert = React.useCallback(async (id: string) => {
    try {
      const a = await gateway.ackAlert(id);
      setAlerts((list) => list?.map((x) => (x.id === id ? a : x)) ?? list);
      return a;
    } catch (err) {
      toast(`ACKNOWLEDGE FAILED · ${err instanceof Error ? err.message : String(err)}`, 'warn');
      return null;
    }
  }, [gateway, toast]);

  const value: HeimdallStatus = {
    feeds: summary?.feeds ?? null,
    allSigned: summary?.allSigned ?? false,
    entities: summary?.entities ?? null,
    spaceWeather: summary?.sw ?? null,
    alerts,
    alertsError,
    refreshAlerts,
    ackAlert,
    alertsOpen,
    setAlertsOpen,
    toasts,
    toast,
    wall,
    setWall,
  };
  return (
    <Ctx.Provider value={value}>
      {children}
      <Toasts toasts={toasts} dismiss={(id) => setToasts((t) => t.filter((x) => x.id !== id))} />
    </Ctx.Provider>
  );
}

function Toasts({ toasts, dismiss }: { toasts: Toast[]; dismiss: (id: number) => void }) {
  if (!toasts.length) return null;
  const colour = { danger: 'var(--danger)', warn: 'var(--warn)', info: 'var(--accent-hover)' } as const;
  return (
    <div role="status" aria-live="polite" style={{ position: 'fixed', left: '50%', bottom: 22, transform: 'translateX(-50%)', zIndex: 200, display: 'flex', flexDirection: 'column', gap: 6, alignItems: 'center', pointerEvents: 'none' }}>
      {toasts.map((t) => (
        <button key={t.id} type="button" onClick={() => dismiss(t.id)}
          style={{ all: 'unset', pointerEvents: 'auto', cursor: 'pointer', padding: '8px 14px', fontFamily: 'var(--font-mono)', fontSize: 10, letterSpacing: '0.1em', color: 'var(--text-heading)', background: 'var(--surface-card)', border: `1px solid ${colour[t.tone]}`, borderLeft: `3px solid ${colour[t.tone]}`, borderRadius: 'var(--radius-sm)', boxShadow: 'var(--shadow-md)', maxWidth: 'min(90vw, 520px)' }}>
          {t.text}
        </button>
      ))}
    </div>
  );
}

/** The space weather seal: "Kp 3 · G0", green when quiet, amber from Kp 4 or G1–G2,
 *  red from G3. */
export function spaceWeatherSeal(sw: HeimdallSpaceWeather): Seal {
  const kp = sw.kp;
  const status = sw.gScale >= 3 ? 'danger' : sw.gScale >= 1 || (kp ?? 0) >= 4 ? 'warn' : 'secure';
  return [status, `Kp ${kp === null ? '—' : Math.round(kp)} · G${sw.gScale}`, false];
}

/** The live header seals for Heimdall. */
export function liveSeals(s: HeimdallStatus): Seal[] {
  const out: Seal[] = [];
  out.push(s.feeds === null ? ['offline', 'FEEDS · READING', false] : ['online', `${s.feeds} FEED${s.feeds === 1 ? '' : 'S'} · FUSED`, s.feeds > 0]);
  if (s.feeds !== null) out.push(s.allSigned ? ['secure', 'FEEDS PQ-AUTH ✓', false] : ['warn', 'FEEDS PQ-AUTH ?', false]);
  out.push(['offline', 'SYNC · LOCAL', false]);
  if (s.spaceWeather) out.push(spaceWeatherSeal(s.spaceWeather));
  return out;
}

/** The live nav badges: entity and feed counts; the workflow count is dropped (those
 *  workflows are still simulated). */
export function liveBadges(s: HeimdallStatus): Record<string, string | null> {
  return {
    entities: s.entities === null ? null : s.entities.toLocaleString(),
    feeds: s.feeds === null ? null : String(s.feeds),
    workflows: null,
  };
}

/** Open (not cleared) alerts, and how many are critical and not yet acknowledged. */
export function alertCounts(alerts: HeimdallAlert[] | null): { open: number; critical: number } {
  const open = (alerts ?? []).filter((a) => a.clearedAt === null);
  return { open: open.length, critical: open.filter((a) => a.severity === 'critical' && !a.acknowledgedBy).length };
}
