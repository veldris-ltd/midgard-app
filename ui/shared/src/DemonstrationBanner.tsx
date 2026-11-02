import * as React from 'react';

/* DemonstrationBanner — the notice a console shows while it runs on built-in sample
 * data. A console in mock mode renders it above everything else, so nobody mistakes
 * the seeded fixtures for an enclosure's data or the absent sign-in for a control.
 * [VLD-ANB-MIDGARD-0086] [AC-25.2] */

export const DEMONSTRATION_NOTICE =
  'Demonstration data. This console runs on built-in sample data and is not connected to an enclosure.';

export function DemonstrationBanner(): React.ReactElement {
  return (
    <div
      role="status"
      style={{
        position: 'sticky',
        top: 0,
        zIndex: 1000,
        padding: '6px 16px',
        textAlign: 'center',
        fontFamily: 'var(--font-sans, system-ui, sans-serif)',
        fontSize: 12,
        fontWeight: 600,
        color: 'var(--ink-950, #050506)',
        background: 'var(--warn, #f0a824)',
      }}
    >
      {DEMONSTRATION_NOTICE}
    </div>
  );
}
