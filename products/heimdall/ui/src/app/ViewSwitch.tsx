import * as React from 'react';

/* The operating picture's view switch, remembered per browser: 2D (the reference
 * layout, the default), MAP (the tracks on a real map) and 3D (the globe). MAP exists
 * only in a build with the map tiles switch on. [FR-H-1] */

export type PictureView = '2d' | 'map' | '3d';

const KEY = 'mg-heimdall-view';

export const PICTURE_VIEWS: readonly PictureView[] = __HEIMDALL_TILES__ ? ['2d', 'map', '3d'] : ['2d', '3d'];

function stored(): PictureView {
  try {
    const v = localStorage.getItem(KEY);
    return (PICTURE_VIEWS as readonly string[]).includes(v ?? '') ? (v as PictureView) : '2d';
  } catch {
    return '2d';
  }
}

export function usePictureView(): [PictureView, (v: PictureView) => void] {
  const [view, setView] = React.useState<PictureView>(stored);
  const set = React.useCallback((v: PictureView) => {
    setView(v);
    try {
      localStorage.setItem(KEY, v);
    } catch {
      /* storage blocked: the choice lasts for this page only */
    }
  }, []);
  return [view, set];
}

export function ViewSwitch({ view, onChange }: { view: PictureView; onChange: (v: PictureView) => void }) {
  return (
    <div role="group" aria-label="Picture view" className="hg-view-switch">
      {PICTURE_VIEWS.map((v) => (
        <button key={v} type="button" aria-pressed={view === v} onClick={() => onChange(v)}>
          {v.toUpperCase()}
        </button>
      ))}
    </div>
  );
}
