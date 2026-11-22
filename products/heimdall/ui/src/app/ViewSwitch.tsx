import * as React from 'react';

/* The operating picture's 2D/3D switch, remembered per browser. 2D is the default;
 * the globe is opt-in. [FR-H-1] */

export type PictureView = '2d' | '3d';

const KEY = 'mg-heimdall-view';

function stored(): PictureView {
  try {
    return localStorage.getItem(KEY) === '3d' ? '3d' : '2d';
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
      {(['2d', '3d'] as const).map((v) => (
        <button key={v} type="button" aria-pressed={view === v} onClick={() => onChange(v)}>
          {v.toUpperCase()}
        </button>
      ))}
    </div>
  );
}
