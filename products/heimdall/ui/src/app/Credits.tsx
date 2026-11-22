import * as React from 'react';

import type { Credit } from './dataSources.ts';

/* The compact ⓘ CREDITS control on the globe and the map: every source the view is
 * drawing from, with its licence and attribution. Plain text, no links: the console
 * makes no request beyond its own files and the map tiles. [FR-H-1] */
export function Credits({ credits }: { credits: readonly Credit[] }) {
  const [open, setOpen] = React.useState(false);
  const id = React.useId();
  return (
    <div className="hg-credits">
      <button type="button" aria-expanded={open} aria-controls={id} onClick={() => setOpen((v) => !v)}>
        <span aria-hidden="true">ⓘ</span> CREDITS
      </button>
      <div id={id} hidden={!open} className="hg-credits-list">
        <ul aria-label="Data sources in use">
          {credits.map((c) => (
            <li key={c.id}>
              <strong>{c.name}</strong>
              <span className="licence">{c.licence}</span>
              <span className="attribution">{c.attribution}</span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}
