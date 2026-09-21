import * as React from 'react';
import { Button, KeyCustodyChip, ProductMark } from '@midgard/design-system';
import { useGateway } from '@midgard/ui-shared';
import type { ClientGateway } from '@midgard/sdk';

/* ExtensionPopup — the least-capable client: a minimal-privilege broker. It
 * holds NO keys and runs NO bulk crypto; it reads only the user-shared selection
 * and brokers a match to the enclosure, which does the privileged work and
 * audits it. `[FR-C-8]` Ported from docs/design/ui_kits/clients/web.html. */

/** ExtensionPopup — brokered, keyless browser-extension popup shown in the web view. */
export function ExtensionPopup() {
  const g = useGateway<ClientGateway>();
  const [sent, setSent] = React.useState(false);
  const [busy, setBusy] = React.useState(false);

  const broker = async () => {
    setBusy(true);
    // Broker the user-shared selection to the enclosure — the popup itself never
    // reads keys, tabs or form fields; it only forwards what the user shared.
    await g.open('AC-2214');
    setSent(true);
    setBusy(false);
  };

  return (
    <div
      style={{
        position: 'absolute',
        top: 6,
        right: 10,
        width: 300,
        zIndex: 20,
        background: 'var(--surface-inset)',
        border: '1px solid var(--border-strong)',
        borderRadius: 12,
        boxShadow: 'var(--shadow-lg)',
        overflow: 'hidden',
        fontFamily: 'var(--font-sans)',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '9px 12px', borderBottom: '1px solid var(--border)', background: 'var(--bg-page)' }}>
        <ProductMark product="midgard" size="sm" showDescriptor={false} />
        <span style={{ marginLeft: 'auto', fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.1em', color: 'var(--info)' }}>BROKER MODE</span>
      </div>
      <div style={{ padding: 12, display: 'flex', flexDirection: 'column', gap: 9 }}>
        <KeyCustodyChip platform="extension" style={{ fontSize: 8.5 }} />
        <div style={{ background: 'var(--surface-card)', border: '1px solid var(--border)', borderRadius: 8, padding: 10 }}>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 8.5, letterSpacing: '0.1em', color: 'var(--text-faint)', marginBottom: 5 }}>USER-SHARED SELECTION · NOTHING ELSE READ</div>
          <p style={{ margin: 0, fontSize: 11, lineHeight: 1.5, color: 'var(--text-body)', fontStyle: 'italic' }}>
            "…tail AC-2214 diverted to Dublin for an unscheduled check…"
          </p>
        </div>
        {sent ? (
          <div style={{ padding: '8px 10px', background: 'var(--surface-inset)', border: '1px solid var(--secure)', borderRadius: 8 }}>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 8, letterSpacing: '0.1em', color: 'var(--secure)', marginBottom: 4 }}>BROKERED → ENCLOSURE · AUDITED</div>
            <p style={{ margin: 0, fontSize: 11, lineHeight: 1.5, color: 'var(--text-body)' }}>
              Matches <span style={{ fontFamily: 'var(--font-mono)', color: 'var(--accent-hover)' }}>Aircraft/AC-2214</span>: open event ME-88121, human gate pending in Heimdall.
            </p>
          </div>
        ) : (
          <Button variant="primary" size="sm" fullWidth disabled={busy} onClick={() => void broker()}>
            {busy ? 'Brokering…' : 'Match against the fabric'}
          </Button>
        )}
        <div style={{ fontFamily: 'var(--font-mono)', fontSize: 7.5, lineHeight: 1.9, color: 'var(--text-faint)', letterSpacing: '0.06em' }}>
          NO KEYS HELD · NO BULK CRYPTO
          <br />
          NO TABS READ · NO FORM FIELDS READ
          <br />
          PRIVILEGED OPS → ENCLOSURE / COMPANION
        </div>
      </div>
    </div>
  );
}
