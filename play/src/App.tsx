import * as React from 'react';
import { Icon } from '@midgard/design-system';
import { ModeToggle } from '@midgard/ui-shared';
import { CONSOLES, byId, consoleUrl, type ConsoleId, type ConsoleInfo } from './consoles';
import { TOUR } from './tour';
import { findTarget, trackRing } from './highlight';

/* The playground shell: a launcher, and a workspace with a console in a
   same-origin frame beside a panel that is either the guided tour or the
   console's sections. State lives in the URL hash so any moment can be linked:
     #/                launcher
     #/tour/<n>        tour step n (1-based)
     #/console/<id>    a console, free to explore
   [ADR-PLAY-002] */

type View = { kind: 'home' } | { kind: 'tour'; step: number } | { kind: 'console'; id: ConsoleId };

const STEP_KEY = 'mg-play-step';
const REPO = 'https://github.com/veldris-ltd/midgard-app';

function parse(hash: string): View {
  const tour = /^#\/tour\/(\d+)$/.exec(hash);
  if (tour) {
    const n = Math.min(TOUR.length, Math.max(1, Number(tour[1])));
    return { kind: 'tour', step: n };
  }
  const con = /^#\/console\/([a-z]+)$/.exec(hash);
  if (con && byId(con[1])) return { kind: 'console', id: con[1] as ConsoleId };
  return { kind: 'home' };
}

const href = (v: View) => (v.kind === 'home' ? '#/' : v.kind === 'tour' ? `#/tour/${v.step}` : `#/console/${v.id}`);

function useView(): View {
  const [view, setView] = React.useState<View>(() => parse(location.hash));
  React.useEffect(() => {
    const on = () => setView(parse(location.hash));
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  }, []);
  return view;
}

function savedStep(): number {
  try {
    const n = Number(localStorage.getItem(STEP_KEY));
    return n >= 1 && n <= TOUR.length ? n : 1;
  } catch {
    return 1;
  }
}

export function App() {
  const view = useView();
  React.useEffect(() => {
    if (view.kind === 'tour') {
      try {
        localStorage.setItem(STEP_KEY, String(view.step));
      } catch {
        /* storage unavailable: the tour still works, it just will not resume */
      }
    }
    const what = view.kind === 'home' ? '' : view.kind === 'tour' ? ` · Tour ${view.step} of ${TOUR.length}` : ` · ${byId(view.id)?.name}`;
    document.title = `Midgard playground${what}`;
  }, [view]);

  return (
    <div className="play">
      <TopBar view={view} />
      <p className="play-strip" role="note">
        <span className="dot" aria-hidden="true" />
        Sample data. Runs entirely in your browser. Nothing you do here is sent anywhere.
      </p>
      {view.kind === 'home' ? <Launcher /> : <Workspace view={view} />}
    </div>
  );
}

function TopBar({ view }: { view: View }) {
  const current = view.kind === 'console' ? view.id : view.kind === 'tour' ? TOUR[view.step - 1].console : null;
  return (
    <header className="play-top">
      <a className="play-brand" href="#/" aria-label="Midgard playground, home">
        <span className="tile" aria-hidden="true">M</span>
        <span className="word">MIDGARD</span>
        <span className="ctx">PLAYGROUND</span>
      </a>
      <nav className="play-nav" aria-label="Consoles">
        <a href={`#/tour/${view.kind === 'tour' ? view.step : savedStep()}`} aria-current={view.kind === 'tour' ? 'page' : undefined}>
          Tour
        </a>
        {CONSOLES.map((c) => (
          <a
            key={c.id}
            href={`#/console/${c.id}`}
            className={`theme-${c.theme}`}
            aria-current={view.kind === 'console' && view.id === c.id ? 'page' : undefined}
            data-current={current === c.id ? '' : undefined}
          >
            {c.name}
          </a>
        ))}
      </nav>
      <div className="play-tools">
        <a className="play-src" href={REPO} rel="noopener">
          Source
        </a>
        <ModeToggle />
      </div>
    </header>
  );
}

function Launcher() {
  return (
    <main id="main" className="play-home">
      <section className="hero">
        <p className="eyebrow">MIDGARD · OPEN SOURCE · APACHE-2.0</p>
        <h1>
          Try the sovereign enclosure<span className="dot">.</span>
        </h1>
        <p className="lede">
          All seven Midgard consoles, built from the public source and running on sample data in your browser. Take the guided tour, or open any console and explore on your own.
        </p>
        <div className="cta">
          <a className="btn primary" href={`#/tour/${savedStep()}`}>
            {savedStep() > 1 ? `Resume the tour at step ${savedStep()}` : 'Start the guided tour'}
            <Icon name="arrow-right" size={16} aria-hidden="true" />
          </a>
          <a className="btn" href={REPO} rel="noopener">
            Read the source
          </a>
        </div>
      </section>

      <section aria-labelledby="consoles-h">
        <h2 id="consoles-h" className="eyebrow">
          THE CONSOLES
        </h2>
        <ul className="cards">
          {CONSOLES.map((c) => (
            <li key={c.id} className={`card theme-${c.theme}`}>
              <a href={`#/console/${c.id}`}>
                <span className="card-role">{c.role.toUpperCase()}</span>
                <span className="card-name">{c.name}</span>
                <span className="card-sum">{c.summary}</span>
                <span className="card-go">
                  Open {c.name} <Icon name="arrow-right" size={14} aria-hidden="true" />
                </span>
              </a>
            </li>
          ))}
        </ul>
      </section>

      <section className="facts" aria-labelledby="facts-h">
        <h2 id="facts-h" className="eyebrow">
          WHAT IS REAL HERE
        </h2>
        <div className="fact-grid">
          <div>
            <h3>Real</h3>
            <p>
              The console code: every screen, control and design-system component is the code in the repository at commit{' '}
              <a href={`${REPO}/commit/${__PLAY_COMMIT__}`} rel="noopener">
                <code>{__PLAY_COMMIT__}</code>
              </a>
              .
            </p>
          </div>
          <div>
            <h3>Sample</h3>
            <p>The data. Each console runs on an in-browser gateway seeded with sample objects, feeds, releases and audit records. There is no server behind this page.</p>
          </div>
          <div>
            <h3>Next step</h3>
            <p>
              Run it yourself: <code>pnpm install</code> then <code>pnpm play</code> serves this playground offline, and <code>just try</code> runs the Midgard console against the real Rust edge.
            </p>
          </div>
        </div>
      </section>
    </main>
  );
}

function Workspace({ view }: { view: Exclude<View, { kind: 'home' }> }) {
  const step = view.kind === 'tour' ? TOUR[view.step - 1] : null;
  const info = byId(step ? step.console : (view as { id: ConsoleId }).id) as ConsoleInfo;
  const route = step ? step.route : '/';
  const frame = React.useRef<HTMLIFrameElement>(null);
  const ring = React.useRef<HTMLDivElement>(null);
  const [loaded, setLoaded] = React.useState<string | null>(null);
  const [missing, setMissing] = React.useState(false);
  const src = consoleUrl(info.id, route);

  // Load a different console by replacing the frame's document; within the same
  // console, move its hash router so its state and scroll survive.
  React.useEffect(() => {
    const f = frame.current;
    if (!f) return;
    const w = f.contentWindow;
    const sameConsole = loaded === info.id && w && w.location.pathname.endsWith(`/consoles/${info.id}/index.html`);
    if (sameConsole) {
      if (w.location.hash !== `#${route}`) w.location.hash = route;
    } else {
      f.src = src;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [info.id, route]);

  // Keep the colour mode in step both ways: the frame and the shell share the
  // `mg-mode` key, and each hears the other's change as a storage event.
  const onLoad = React.useCallback(() => {
    const w = frame.current?.contentWindow;
    if (!w) return;
    setLoaded(info.id);
    w.addEventListener('storage', (e) => {
      if (e.key !== 'mg-mode') return;
      const light = e.newValue === 'light' || (e.newValue === 'system' && w.matchMedia('(prefers-color-scheme: light)').matches);
      w.document.documentElement.classList.toggle('mode-light', light);
      w.dispatchEvent(new Event('mg-mode'));
    });
  }, [info.id]);

  // Ring the step's target once it is on screen; give the console up to 3 s to render it.
  React.useEffect(() => {
    setMissing(false);
    const f = frame.current;
    const r = ring.current;
    if (!step?.target || !f || !r || loaded !== info.id) return;
    let stop = () => {};
    let tries = 0;
    const id = window.setInterval(() => {
      const doc = f.contentDocument;
      const el = doc && f.contentWindow?.location.hash === `#${route}` ? findTarget(doc, step.target!) : null;
      if (el) {
        window.clearInterval(id);
        stop = trackRing(f, el, r);
      } else if (++tries > 30) {
        window.clearInterval(id);
        setMissing(true);
      }
    }, 100);
    return () => {
      window.clearInterval(id);
      stop();
    };
  }, [step, loaded, info.id, route]);

  return (
    <main id="main" className={`play-work theme-${info.theme}`}>
      <div className="frame-wrap">
        <iframe ref={frame} title={`${info.name} console, ${info.role}, on sample data`} onLoad={onLoad} />
        <div ref={ring} className="ring" hidden aria-hidden="true" />
      </div>
      {view.kind === 'tour' ? <TourPanel n={view.step} missing={missing} /> : <ConsolePanel info={info} frame={frame} />}
    </main>
  );
}

function TourPanel({ n, missing }: { n: number; missing: boolean }) {
  const step = TOUR[n - 1];
  const info = byId(step.console) as ConsoleInfo;
  const prev = n > 1 ? href({ kind: 'tour', step: n - 1 }) : null;
  const next = n < TOUR.length ? href({ kind: 'tour', step: n + 1 }) : null;
  const heading = React.useRef<HTMLHeadingElement>(null);

  React.useEffect(() => {
    heading.current?.focus({ preventScroll: true });
  }, [n]);

  const onKey = (e: React.KeyboardEvent) => {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    if (e.key === 'ArrowRight' && next) location.hash = next;
    if (e.key === 'ArrowLeft' && prev) location.hash = prev;
  };

  return (
    <aside className="panel" aria-labelledby="step-h" onKeyDown={onKey}>
      <p className="eyebrow" aria-live="polite">
        STEP {n} OF {TOUR.length} · {info.name.toUpperCase()}
      </p>
      <div className="progress" aria-hidden="true">
        <span style={{ width: `${(n / TOUR.length) * 100}%` }} />
      </div>
      <h2 id="step-h" ref={heading} tabIndex={-1}>
        {step.title}
      </h2>
      {step.body.map((p, i) => (
        <p key={i}>{p}</p>
      ))}
      {step.try && (
        <div className="try">
          <span className="eyebrow">TRY IT</span>
          <p>{step.try}</p>
        </div>
      )}
      {step.target && missing && <p className="faint">The highlighted control is not on screen right now; the step still applies.</p>}
      <div className="step-nav">
        {prev ? (
          <a className="btn" href={prev}>
            <Icon name="arrow-right" size={14} className="flip" aria-hidden="true" /> Back
          </a>
        ) : (
          <a className="btn" href="#/">
            Home
          </a>
        )}
        {next ? (
          <a className="btn primary" href={next}>
            Next <Icon name="arrow-right" size={14} aria-hidden="true" />
          </a>
        ) : (
          <a className="btn primary" href={REPO} rel="noopener">
            Get the source <Icon name="arrow-right" size={14} aria-hidden="true" />
          </a>
        )}
      </div>
      <p className="faint keys">← → keys move between steps</p>
      <ol className="steps" aria-label="All steps">
        {TOUR.map((s, i) => (
          <li key={i}>
            <a href={href({ kind: 'tour', step: i + 1 })} aria-current={i + 1 === n ? 'step' : undefined}>
              <span className="num">{String(i + 1).padStart(2, '0')}</span> {s.title}
            </a>
          </li>
        ))}
      </ol>
    </aside>
  );
}

function ConsolePanel({ info, frame }: { info: ConsoleInfo; frame: React.RefObject<HTMLIFrameElement> }) {
  const first = TOUR.findIndex((s) => s.console === info.id) + 1;
  const go = (route: string) => {
    const w = frame.current?.contentWindow;
    if (w) w.location.hash = route;
  };
  return (
    <aside className="panel" aria-labelledby="con-h">
      <p className="eyebrow">{info.role.toUpperCase()}</p>
      <h2 id="con-h">{info.name}</h2>
      <p>{info.summary}</p>
      <h3 className="eyebrow">SECTIONS</h3>
      <ul className="sections">
        {info.sections.map((s) => (
          <li key={s.route}>
            <button type="button" onClick={() => go(s.route)}>
              {s.label}
            </button>
          </li>
        ))}
      </ul>
      {first > 0 && (
        <a className="btn" href={href({ kind: 'tour', step: first })}>
          Tour {info.name} from step {first} <Icon name="arrow-right" size={14} aria-hidden="true" />
        </a>
      )}
    </aside>
  );
}
