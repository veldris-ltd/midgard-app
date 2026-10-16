import * as React from 'react';
import {
  Badge,
  Card,
  HumanGate,
  Icon,
  IconButton,
  MarkingChip,
  ProvenanceTag,
  SectionLabel,
  StatBlock,
  Switch,
} from '@midgard/design-system';
import { useAsync, useGateway } from '@midgard/ui-shared';
import { HumanGateRequiredError } from '@midgard/sdk';
import type { HeimdallGateway, HeimdallLayerId, HeimdallPicture, HeimdallTrack } from '@midgard/sdk';
import { ConsolePage } from '../app/ConsolePage';

type LayerState = Record<HeimdallLayerId, boolean>;
type GateState = { rank: number; state: 'pending' | 'authorised' | 'rejected'; auditId?: string } | null;

const CAPABILITY: Record<string, string> = {
  AIRCRAFT: 'IFF · ADS-B · RADAR',
  VESSEL: 'AIS · RADAR',
  'GROUND UNIT': 'OBSERVER NET',
  UNKNOWN: 'RADAR ONLY · UNCORROBORATED',
};

/** Advisory option rank → the human-gated workflow it would open. Rank 3 (hold)
 *  opens no workflow — the operator simply defers. */
const WORKFLOW_FOR_RANK: Record<number, string | undefined> = { 1: 'W-0912', 2: 'W-0913', 3: undefined };

export function OperatingPictureScreen() {
  const gateway = useGateway<HeimdallGateway>();
  const { data: picture } = useAsync<HeimdallPicture, HeimdallGateway>((c) => c.getPicture(), []);

  const layerDefs = picture?.layers ?? [];
  const tracks: HeimdallTrack[] = picture?.tracks ?? [];

  const [layers, setLayers] = React.useState<LayerState | null>(null);
  const [sel, setSel] = React.useState('E-0871');
  const [chosen, setChosen] = React.useState(1);
  const [hover, setHover] = React.useState<string | null>(null);
  const [gate, setGate] = React.useState<GateState>(null);
  const [busy, setBusy] = React.useState(false);

  // Seed the toggle state once the picture's layers resolve.
  React.useEffect(() => {
    if (layerDefs.length && !layers) {
      setLayers(layerDefs.reduce((acc, l) => ({ ...acc, [l.id]: l.active }), {} as LayerState));
    }
  }, [layerDefs, layers]);

  const layerOn = (id: HeimdallLayerId) => layers?.[id] !== false;
  const setLayer = (id: HeimdallLayerId, v: boolean) => setLayers((l) => ({ ...(l ?? ({} as LayerState)), [id]: v }));

  const e = tracks.find((t) => t.id === sel);
  const visible = tracks.filter((t) => layerOn(t.layer));
  const options = e?.options ?? [];

  const authorise = async () => {
    const wfId = WORKFLOW_FOR_RANK[chosen];
    if (!wfId) return;
    setBusy(true);
    try {
      const res = await gateway.authoriseWorkflow(wfId, `HG-${Date.now().toString(36)}`);
      setGate({ rank: chosen, state: 'authorised', auditId: res.auditId });
    } catch (err) {
      if (err instanceof HumanGateRequiredError) setGate({ rank: chosen, state: 'pending' });
    } finally {
      setBusy(false);
    }
  };

  const aside = e ? (
    <>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
        <Icon name="crosshair" size={17} color={e.track === 'anomalous' ? 'var(--warn)' : 'var(--accent)'} />
        <h2 style={{ fontSize: 'var(--fs-h4)', minWidth: 0 }}>{e.id}</h2>
        <Badge tone={e.track === 'anomalous' ? 'warn' : 'neutral'}>{e.cls}</Badge>
        <span style={{ marginLeft: 'auto' }}><MarkingChip level="confidential" compartment="OPS-N" /></span>
      </div>
      <Card size="sm">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          <StatBlock value={Math.round(e.conf * 100) + '%'} label="CLASSIFICATION CONF." size="sm" accent={e.conf < 0.7} />
          <StatBlock value={e.track === 'anomalous' ? 'ANOM' : 'STABLE'} label="TRACK STATE" size="sm" />
        </div>
      </Card>

      {options.length > 0 && (
        <div>
          <SectionLabel number={5}>RANKED OPTIONS · ADVISORY</SectionLabel>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, marginTop: 10 }}>
            {options.map((o) => (
              <Card
                key={o.rank}
                size="sm"
                interactive
                onClick={() => { setChosen(o.rank); setGate(null); }}
                style={{ borderColor: chosen === o.rank ? 'var(--accent)' : undefined }}
              >
                <div style={{ display: 'flex', gap: 9, alignItems: 'baseline' }}>
                  <span style={{ fontFamily: 'var(--font-display)', fontWeight: 800, fontSize: 15, color: chosen === o.rank ? 'var(--accent-hover)' : 'var(--text-faint)' }}>
                    {String(o.rank).padStart(2, '0')}
                  </span>
                  <div style={{ flex: 1 }}>
                    <div style={{ fontSize: 'var(--fs-sm)', color: 'var(--grey-100)', fontWeight: 500 }}>{o.title}</div>
                    <div style={{ fontSize: 'var(--fs-xs)', color: 'var(--text-muted)', marginTop: 3, lineHeight: 1.5 }}>{o.why}</div>
                    <div style={{ display: 'flex', gap: 5, marginTop: 6, flexWrap: 'wrap' }}>
                      {o.cites.map((c, i) => (
                        <span key={i} style={{ fontFamily: 'var(--font-mono)', fontSize: 9, color: 'var(--accent-hover)', background: 'var(--surface-inset)', border: '1px solid var(--border)', borderRadius: 4, padding: '1px 6px' }}>{c}</span>
                      ))}
                    </div>
                  </div>
                </div>
              </Card>
            ))}
          </div>
        </div>
      )}

      {options.length > 0 && (
        WORKFLOW_FOR_RANK[chosen] ? (
          <HumanGate
            title={`Authorise option ${String(chosen).padStart(2, '0')} · ${e.id}`}
            detail="HUMAN AUTHORISATION REQUIRED · AI IS ADVISORY"
            state={gate?.state ?? 'pending'}
            auditId={gate?.auditId}
            onAuthorise={busy ? undefined : authorise}
            onReject={() => setGate({ rank: chosen, state: 'rejected' })}
          />
        ) : (
          <div style={{ padding: '12px 14px', background: 'var(--surface-inset)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--text-faint)' }}>
            HOLD · NO ACTION · PASSIVE FUSION CONTINUES
          </div>
        )
      )}

      <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', marginTop: 'auto' }}>
        <ProvenanceTag id="FR-H-4" title="AI-assisted, explainable, cited ranked options, advisory" />
        <ProvenanceTag id="FR-H-5" title="Human-gated action workflows with full audit lineage" />
        <ProvenanceTag id="NFR-AI-1" title="Every agentic action traceable, reversible or human-gated" />
      </div>
    </>
  ) : null;

  const focus = e;
  const focusRng = focus ? Math.round(Math.hypot(focus.x - 50, focus.y - 50) * 1.3) : 0;

  return (
    <ConsolePage
      active="cop"
      context="COMMON OPERATING PICTURE / SECTOR NORTH"
      asideWidth={360}
      mainStyle={{ padding: 0, gap: 0, position: 'relative' }}
      toolbar={
        <>
          <IconButton label="Alerts" active>
            <Icon name="bell" size={17} />
          </IconButton>
          <IconButton label="Settings">
            <Icon name="settings" size={17} />
          </IconButton>
        </>
      }
      navFooter={
        <div style={{ padding: '10px 10px 4px', display: 'flex', flexDirection: 'column', gap: 9 }}>
          <div className="mg-label" style={{ color: 'var(--text-faint)' }}>LAYERS</div>
          {layerDefs.map((l) => (
            <Switch key={l.id} checked={layerOn(l.id)} onChange={(v) => setLayer(l.id, v)} label={l.label} />
          ))}
        </div>
      }
      aside={aside}
    >
      <div className="mg-grid-bg" style={{ position: 'absolute', inset: 0, pointerEvents: 'none' }} />
      <div style={{ position: 'absolute', inset: 0, background: 'radial-gradient(ellipse at 55% 45%, var(--accent-tint), transparent 60%)' }} />

      {/* range rings + scale + north arrow */}
      <div style={{ position: 'absolute', inset: 0, overflow: 'hidden', pointerEvents: 'none' }}>
        {([[160, '20 NM'], [320, '40 NM'], [480, '60 NM']] as [number, string][]).map(([rr, lb], i) => (
          <React.Fragment key={i}>
            <div style={{ position: 'absolute', left: '50%', top: '50%', width: rr * 2, height: rr * 2, marginLeft: -rr, marginTop: -rr, border: '1px solid var(--divider)', borderRadius: '50%' }} />
            <span style={{ position: 'absolute', left: '50%', top: `calc(50% - ${rr}px)`, transform: 'translate(-50%, -50%)', fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.12em', color: 'var(--text-faint)', background: 'var(--bg-sunken)', padding: '0 5px' }}>{lb}</span>
          </React.Fragment>
        ))}
        <div style={{ position: 'absolute', top: 12, left: 14, display: 'flex', flexDirection: 'column', gap: 6 }}>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, letterSpacing: '0.14em', color: 'var(--text-muted)' }}>N ↑</span>
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span style={{ width: 80, height: 0, borderTop: '2px solid var(--text-faint)', position: 'relative' }}>
              <span style={{ position: 'absolute', left: 0, top: -4, width: 2, height: 6, background: 'var(--text-faint)' }} />
              <span style={{ position: 'absolute', right: 0, top: -4, width: 2, height: 6, background: 'var(--text-faint)' }} />
            </span>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.12em', color: 'var(--text-faint)' }}>10 NM</span>
          </span>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.12em', color: 'var(--text-faint)' }}>VECTOR · 1 MIN PROJECTION</span>
        </div>
      </div>

      {layerOn('feeds') && (
        <>
          <div style={{ position: 'absolute', left: '12%', top: '8%', width: '55%', height: '62%', border: '1px dashed var(--accent-glow)', borderRadius: '50%', pointerEvents: 'none' }} />
          <span style={{ position: 'absolute', left: '39%', top: '8%', transform: 'translate(-50%, -50%)', fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.12em', color: 'var(--accent-hover)', background: 'var(--bg-sunken)', padding: '1px 6px', pointerEvents: 'none' }}>NORTH-2 · RADAR · 4 HZ · R 60 NM</span>
        </>
      )}

      {visible.map((en) => {
        const anomalous = en.track === 'anomalous';
        const selected = en.id === sel;
        const color = anomalous ? 'var(--warn)' : 'var(--accent)';
        const rad = (en.hdg * Math.PI) / 180;
        const vlen = Math.min(46, 12 + en.spd / 14);
        return (
          <button
            key={en.id}
            onClick={() => { setSel(en.id); setGate(null); }}
            onMouseEnter={() => setHover(en.id)}
            onMouseLeave={() => setHover(null)}
            onFocus={() => setHover(en.id)}
            onBlur={() => setHover(null)}
            aria-label={`${en.id} · ${en.cls} · heading ${en.hdg} degrees`}
            style={{ all: 'unset', position: 'absolute', left: en.x + '%', top: en.y + '%', transform: 'translate(-50%,-50%)', display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 5, cursor: 'pointer' }}
          >
            <span style={{ position: 'relative', width: 14, height: 14 }}>
              {[1, 2, 3].map((k) => (
                <span
                  key={k}
                  style={{
                    position: 'absolute', left: '50%', top: '50%', width: 4 - k * 0.7, height: 4 - k * 0.7, borderRadius: '50%',
                    background: color, opacity: 0.45 - k * 0.11,
                    transform: `translate(-50%,-50%) translate(${-Math.sin(rad) * k * 10}px, ${Math.cos(rad) * k * 10}px)`,
                  }}
                />
              ))}
              <span
                style={{
                  position: 'absolute', left: '50%', bottom: '50%', width: 2, height: vlen,
                  background: `linear-gradient(to top, ${color}, transparent)`,
                  transformOrigin: 'bottom center', transform: `translateX(-50%) rotate(${en.hdg}deg)`,
                  opacity: 0.9, pointerEvents: 'none',
                }}
              />
              <span
                style={{
                  position: 'absolute', inset: 0, borderRadius: en.layer === 'maritime' ? 2 : '50%',
                  background: color,
                  boxShadow: selected ? `0 0 0 5px ${anomalous ? 'var(--warn-tint)' : 'var(--accent-tint)'}, 0 0 18px -2px ${anomalous ? 'var(--warn)' : 'var(--accent-glow)'}` : 'none',
                  outline: selected ? `1px solid ${color}` : 'none', outlineOffset: 3,
                }}
              />
            </span>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.08em', color: selected ? 'var(--grey-100)' : 'var(--text-faint)', whiteSpace: 'nowrap' }}>
              {en.id}{anomalous ? ' ⚠' : ''} · {en.hdg}°
            </span>
          </button>
        );
      })}

      {(() => {
        const t = tracks.find((x) => x.id === hover);
        if (!t || !layerOn(t.layer)) return null;
        const rng = Math.round(Math.hypot(t.x - 50, t.y - 50) * 1.3);
        const flipY = t.y < 36;
        const rows: [string, string][] = [
          ['HDG / SPD', String(t.hdg).padStart(3, '0') + '° · ' + t.spd + ' KT'],
          ['ALT', t.alt],
          ['RNG', rng + ' NM FROM SITE'],
          ['CAP', CAPABILITY[t.cls] ?? '—'],
          ['TRACK', t.track.toUpperCase() + ' · ' + t.feeds + ' FEEDS'],
        ];
        return (
          <div
            role="tooltip"
            style={{
              position: 'absolute', left: `clamp(110px, ${t.x}%, calc(100% - 110px))`, top: t.y + '%',
              transform: `translate(-50%, ${flipY ? '30px' : 'calc(-100% - 26px)'})`,
              zIndex: 5, pointerEvents: 'none', minWidth: 200,
              background: 'var(--surface-card)', border: '1px solid ' + (t.track === 'anomalous' ? 'var(--warn)' : 'var(--border-strong)'),
              borderRadius: 'var(--radius-sm)', padding: '9px 11px', boxShadow: 'var(--shadow-md)',
            }}
          >
            <div style={{ display: 'flex', gap: 8, alignItems: 'baseline', marginBottom: 6 }}>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, fontWeight: 600, color: 'var(--text-heading)' }}>{t.id}</span>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.1em', color: 'var(--text-faint)' }}>{t.cls}</span>
              <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 10, color: t.conf < 0.7 ? 'var(--warn)' : 'var(--secure)' }}>{Math.round(t.conf * 100)}%</span>
            </div>
            {rows.map(([k, v]) => (
              <div key={k} style={{ display: 'flex', gap: 10, padding: '2px 0', fontFamily: 'var(--font-mono)', fontSize: 9 }}>
                <span style={{ color: 'var(--text-faint)', width: 62, flex: 'none', letterSpacing: '0.08em' }}>{k}</span>
                <span style={{ color: 'var(--text-body)' }}>{v}</span>
              </div>
            ))}
            {t.id === sel && <div style={{ marginTop: 5, fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.14em', color: 'var(--accent-hover)' }}>FOCUSED · OPTIONS IN INSPECTOR →</div>}
          </div>
        );
      })()}

      {focus && (
        <div style={{ position: 'absolute', top: 12, right: 14, pointerEvents: 'none', textAlign: 'right', fontFamily: 'var(--font-mono)', display: 'flex', flexDirection: 'column', gap: 3 }}>
          <span style={{ fontSize: 8, letterSpacing: '0.14em', color: 'var(--accent-hover)' }}>FOCUS · {focus.id}</span>
          <span style={{ fontSize: 9, color: 'var(--text-muted)' }}>{String(focus.hdg).padStart(3, '0')}° · {focus.spd} KT · {focus.alt}</span>
          <span style={{ fontSize: 9, color: 'var(--text-faint)' }}>{focusRng} NM · {focus.feeds} FEEDS · {focus.track.toUpperCase()}</span>
        </div>
      )}

      <div style={{ position: 'absolute', left: 12, right: 12, bottom: 10, display: 'flex', alignItems: 'flex-end', gap: '8px 18px', flexWrap: 'wrap', zIndex: 4, background: 'color-mix(in srgb, var(--bg-sunken) 80%, transparent)', backdropFilter: 'var(--blur-panel)', WebkitBackdropFilter: 'var(--blur-panel)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)', padding: '8px 12px', width: 'fit-content', maxWidth: 'calc(100% - 24px)' }}>
        {/* Counted from the picture the gateway serves, not fixed figures. [FR-H-1] [FR-H-2] */}
        <StatBlock value={(picture?.entities.length ?? 0).toLocaleString()} label="TRACKED ENTITIES" size="sm" />
        <StatBlock value={visible.length.toLocaleString()} label="ON CANVAS" size="sm" />
        <StatBlock value={String(tracks.filter((t) => t.track === 'anomalous').length)} label="ANOMALIES" size="sm" accent />
        <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.14em', color: 'var(--text-faint)', whiteSpace: 'nowrap' }}>REFERENCE LAYOUT · POSITIONS NOT TO MAP SCALE</span>
      </div>
    </ConsolePage>
  );
}
