import * as React from 'react';
import {
  Button,
  Switch,
  Input,
  MarkingChip,
  SectionLabel,
  SyncStatus,
  TierBadge,
  KeyCustodyChip,
  ProductMark,
} from '@midgard/design-system';
import { useGateway, useAsync } from '@midgard/ui-shared';
import type {
  ClientGateway,
  ClientObject,
  ClientPendingOp,
  ClientKeyCustody,
  ClientTier,
} from '@midgard/sdk';

/* ClientApp — the ONE offline-first client screen, rendered identically inside
 * every device frame. One core, many shells [DD-6]: the picture, edits, sync
 * engine and governance chrome are shared; only the surrounding chrome (bezel,
 * window, browser) differs per platform. Every byte here flows through the
 * `ClientGateway` seam (the `midgard-client-core` mock today). */

/** KeyCustodyChip platform, also selects the platform-auth label. */
export type ClientPlatform = 'ios' | 'android' | 'linux' | 'macos' | 'windows' | 'web';

const PLATFORM_LABEL: Record<ClientPlatform, { shell: string; auth: string }> = {
  ios: { shell: 'iOS · SWIFTUI', auth: 'FACE ID' },
  android: { shell: 'ANDROID · COMPOSE', auth: 'BIOMETRICPROMPT' },
  linux: { shell: 'LINUX MOBILE · GTK', auth: 'PAM · FPRINTD' },
  macos: { shell: 'MACOS · TAURI', auth: 'TOUCH ID' },
  windows: { shell: 'WINDOWS · TAURI', auth: 'WINDOWS HELLO' },
  web: { shell: 'WEB · WASM CORE', auth: 'WEBAUTHN · PASSKEY' },
};

const TIER_MODEL: Record<ClientTier, string> = {
  'on-device': 'GEMMA INT4',
  hybrid: 'Mistral INT4',
  remote: 'Mistral Small 3.2',
};

const MONO_MICRO: React.CSSProperties = {
  fontFamily: 'var(--font-mono)',
  fontSize: 'var(--fs-micro)',
  letterSpacing: '0.08em',
  textTransform: 'uppercase',
  color: 'var(--text-faint)',
};

export interface ClientAppProps {
  platform: ClientPlatform;
  /** Compact density for narrow mobile bezels. */
  density?: 'compact' | 'regular';
}

type SyncChip = 'offline' | 'syncing' | 'synced' | 'merged';

/** ClientApp — the shared client experience: clearance-scoped cached picture,
 *  editable fields that queue conflict-free ops, and an offline-first sync
 *  engine you can watch drain on reconnect. */
export function ClientApp({ platform, density = 'regular' }: ClientAppProps) {
  const g = useGateway<ClientGateway>();
  const compact = density === 'compact';

  const objects = useAsync<ClientObject[], ClientGateway>((c) => c.visibleObjects());
  const custody = useAsync<ClientKeyCustody, ClientGateway>((c) => c.keyCustody());

  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const [drafts, setDrafts] = React.useState<Record<string, string>>({});
  const [pending, setPending] = React.useState<ClientPendingOp[]>([]);
  const [lastSync, setLastSync] = React.useState<string>('—');
  const [offline, setOffline] = React.useState(false);
  const [tier, setTier] = React.useState<{ tier: ClientTier; reason: string }>({
    tier: 'hybrid',
    reason: '',
  });
  const [chip, setChip] = React.useState<SyncChip>('synced');
  const [busy, setBusy] = React.useState(false);

  const list = objects.data ?? [];
  const selected = list.find((o) => o.id === selectedId) ?? list[0] ?? null;

  // Default the selection once the cached picture resolves.
  React.useEffect(() => {
    if (selectedId === null && list.length) setSelectedId(list[0].id);
  }, [list, selectedId]);

  // Pull sync + inference posture from the core.
  const refreshPosture = React.useCallback(async () => {
    const [ops, state, t] = await Promise.all([g.pendingOps(), g.syncState(), g.inferenceTier()]);
    setPending(ops);
    setLastSync(state.lastSync);
    setTier(t);
    setChip(state.phase === 'offline' ? 'offline' : ops.length > 0 ? 'syncing' : 'synced');
  }, [g]);

  React.useEffect(() => {
    void refreshPosture();
  }, [refreshPosture]);

  const fieldKey = (id: string, field: string) => `${id}::${field}`;
  const pendingSet = new Set(pending.map((op) => fieldKey(op.objectId, op.field)));

  const commit = async (id: string, field: string, committed: string) => {
    const key = fieldKey(id, field);
    const next = drafts[key];
    if (next === undefined || next === committed) return;
    setBusy(true);
    await g.editField(id, field, next);
    setDrafts((d) => {
      const { [key]: _drop, ...rest } = d;
      return rest;
    });
    objects.reload();
    await refreshPosture();
    setChip(offline ? 'offline' : 'syncing');
    setBusy(false);
  };

  const onSync = async () => {
    setBusy(true);
    if (!offline) setChip('syncing');
    const res = await g.sync();
    if (res.drained > 0 || res.state.phase === 'synced') {
      setChip('merged');
      window.setTimeout(() => setChip('synced'), 1600);
    } else {
      setChip('offline');
    }
    await refreshPosture();
    setLastSync(res.state.lastSync);
    setBusy(false);
  };

  const onToggleOffline = async (next: boolean) => {
    setOffline(next);
    g.setOffline(next);
    await refreshPosture();
    setChip(next ? 'offline' : pending.length > 0 ? 'syncing' : 'synced');
  };

  const pl = PLATFORM_LABEL[platform];
  const pad = compact ? 10 : 14;
  const gap = compact ? 8 : 10;

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        height: '100%',
        background: 'var(--bg-page)',
        color: 'var(--text-body)',
        fontFamily: 'var(--font-sans)',
      }}
    >
      {/* in-frame classification strip (server-side marking enforcement) */}
      <div
        style={{
          height: 20,
          flex: 'none',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          background: 'var(--marking-confidential)',
          color: '#fff',
          fontFamily: 'var(--font-mono)',
          fontSize: 8.5,
          letterSpacing: '0.16em',
          fontWeight: 600,
        }}
      >
        CONFIDENTIAL · RECIPIENT EYES ONLY
      </div>

      {/* header */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: `${pad}px ${pad}px 6px` }}>
        <ProductMark product="midgard" size="sm" showDescriptor={false} />
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.12em', color: 'var(--text-faint)' }}>{pl.shell}</span>
      </div>

      {/* scrollable content */}
      <div style={{ flex: 1, minHeight: 0, overflowY: 'auto', padding: `0 ${pad}px ${pad}px`, display: 'flex', flexDirection: 'column', gap }}>
        {/* posture: key custody + sync + inference tier */}
        <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', alignItems: 'center' }}>
          <SyncStatus state={chip} queued={pending.length} style={{ fontSize: 9 }} />
          <KeyCustodyChip platform={platform} style={{ fontSize: 8.5 }} />
          <TierBadge
            tier={tier.tier}
            model={TIER_MODEL[tier.tier]}
            leaves={tier.tier === 'on-device' ? false : null}
            style={{ fontSize: 8.5 }}
          />
        </div>

        {/* sealed local store readout */}
        {custody.data && (
          <div style={{ ...MONO_MICRO, fontSize: 8.5, letterSpacing: '0.06em', textTransform: 'none', lineHeight: 1.7, color: 'var(--text-faint)' }}>
            LOCAL STORE · {custody.data.sealed ? 'SEALED' : 'UNSEALED'} · {custody.data.cipher} under {custody.data.dataKey} · {pl.auth} ✓
          </div>
        )}

        {/* cached picture — clearance-scoped object list */}
        <div>
          <SectionLabel number={1} style={{ marginBottom: 6 }}>CACHED FABRIC · POLICY-SCOPED</SectionLabel>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {objects.loading && !list.length && <span style={MONO_MICRO}>LOADING PICTURE…</span>}
            {list.map((o) => {
              const active = selected?.id === o.id;
              const objHasPending = pending.some((op) => op.objectId === o.id);
              return (
                <button
                  key={o.id}
                  type="button"
                  onClick={() => setSelectedId(o.id)}
                  style={{
                    all: 'unset',
                    cursor: 'pointer',
                    display: 'flex',
                    alignItems: 'center',
                    gap: 8,
                    padding: '8px 10px',
                    borderRadius: 'var(--radius-sm)',
                    background: active ? 'var(--accent-tint)' : 'var(--surface-card)',
                    border: '1px solid ' + (active ? 'var(--accent)' : 'var(--border)'),
                  }}
                >
                  <span style={{ display: 'flex', flexDirection: 'column', gap: 2, minWidth: 0, flex: 1 }}>
                    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-heading)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{o.title}</span>
                    <span style={{ ...MONO_MICRO, fontSize: 8 }}>{o.type}</span>
                  </span>
                  {objHasPending && (
                    <span style={{ fontFamily: 'var(--font-mono)', fontSize: 7.5, letterSpacing: '0.08em', color: 'var(--warn)', flex: 'none' }}>· PENDING</span>
                  )}
                  <MarkingChip level={o.marking} style={{ fontSize: 8, flex: 'none' }} />
                </button>
              );
            })}
          </div>
        </div>

        {/* object detail — editable fields queue conflict-free ops */}
        {selected && (
          <div style={{ background: 'var(--surface-card)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', padding: pad }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 10, color: 'var(--text-heading)' }}>{selected.title}</span>
              <span style={{ marginLeft: 'auto' }}>
                <MarkingChip level={selected.marking} style={{ fontSize: 8 }} />
              </span>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              {Object.entries(selected.fields).map(([field, committed]) => {
                const key = fieldKey(selected.id, field);
                const value = drafts[key] ?? committed;
                const queued = pendingSet.has(key);
                return (
                  <div key={field} style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                    <span style={{ ...MONO_MICRO, fontSize: 8, display: 'flex', alignItems: 'center', gap: 6 }}>
                      {field.replace(/_/g, ' ')}
                      {queued && <span style={{ color: 'var(--warn)' }}>· QUEUED</span>}
                    </span>
                    <Input
                      mono
                      size="sm"
                      fullWidth
                      value={value}
                      onChange={(e) => setDrafts((d) => ({ ...d, [key]: e.target.value }))}
                      onBlur={() => void commit(selected.id, field, committed)}
                      onKeyDown={(e) => {
                        if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
                      }}
                    />
                  </div>
                );
              })}
            </div>
            <p style={{ margin: '8px 0 0', ...MONO_MICRO, fontSize: 8, textTransform: 'none', letterSpacing: '0.04em', lineHeight: 1.6, color: 'var(--text-faint)' }}>
              Edits write to the sealed local store and queue a conflict-free op. Nothing leaves the device until sync.
            </p>
          </div>
        )}

        {/* sync engine + connectivity */}
        <div style={{ background: 'var(--surface-card)', border: '1px solid var(--border)', borderRadius: 'var(--radius-md)', padding: pad, display: 'flex', flexDirection: 'column', gap }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <SectionLabel number={2}>SYNC ENGINE · CRDT</SectionLabel>
            <span style={{ marginLeft: 'auto', ...MONO_MICRO, fontSize: 8 }}>LAST MERGE {lastSync}</span>
          </div>
          <div style={{ display: 'flex', gap: 14, flexWrap: 'wrap' }}>
            <span style={{ ...MONO_MICRO, fontSize: 8 }}>
              <span style={{ color: pending.length ? 'var(--warn)' : 'var(--secure)', fontSize: 13, fontFamily: 'var(--font-display)' }}>{pending.length}</span> OPS QUEUED
            </span>
            <span style={{ ...MONO_MICRO, fontSize: 8 }}>
              <span style={{ color: 'var(--secure)', fontSize: 13, fontFamily: 'var(--font-display)' }}>0</span> CONFLICTS · EVER
            </span>
          </div>
          {pending.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 3, maxHeight: 96, overflowY: 'auto' }}>
              {pending.map((op) => (
                <div key={op.id} style={{ ...MONO_MICRO, fontSize: 8, textTransform: 'none', letterSpacing: '0.03em', color: 'var(--grey-300)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                  <span style={{ color: 'var(--warn)' }}>+</span> {op.id} · {op.objectId}.{op.field} = {op.value} · {op.ts}
                </div>
              ))}
            </div>
          )}
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, flexWrap: 'wrap' }}>
            <Button variant="primary" size="sm" disabled={busy || offline || pending.length === 0} onClick={() => void onSync()}>
              {offline ? 'Sync blocked · offline' : `Sync · drain ${pending.length}`}
            </Button>
            <Switch checked={offline} onChange={(v) => void onToggleOffline(v)} label={offline ? 'OFFLINE' : 'ONLINE'} style={{ marginLeft: 'auto' }} />
          </div>
          <div style={{ ...MONO_MICRO, fontSize: 8, textTransform: 'none', letterSpacing: '0.04em', lineHeight: 1.6, color: 'var(--text-faint)' }}>
            {offline
              ? 'Offline · edits keep queuing locally. Sync fails closed until PQ-mTLS re-negotiates.'
              : 'Transport PQ-mTLS · X25519+ML-KEM-768 · pinned. Reconnect drains the queue with a conflict-free merge.'}
          </div>
        </div>
      </div>
    </div>
  );
}
