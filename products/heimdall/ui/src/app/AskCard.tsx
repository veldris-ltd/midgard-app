/* Mimir: a question about the picture in the area on screen, answered by the
 * enclosure's own model (the Mimir core agent, A.1) from the items Heimdall gives it as
 * evidence, every one cited and checked. Strictly advisory: actions it proposes are
 * shown, never executed, and the question is recorded in Heimdall's audit ledger.
 * [FR-MI-3] [FR-H-4] [NFR-AI-1] */
import * as React from 'react';
import { Card } from '@midgard/design-system';
import type { HeimdallAnswer, HeimdallGateway } from '@midgard/sdk';
import { findPrepared, loadShippedGuide, preparedKey, putPrepared, shippedGuideNow, shippedKey, type ShippedGuide } from './preparedAnswers';

const mono: React.CSSProperties = { fontFamily: 'var(--font-mono)', fontSize: 9, letterSpacing: '0.08em' };
/** The built-in example chips, when the app ships no prepared answers. */
const EXAMPLES = ['What has changed in the Gulf watch?', 'Which areas have power cuts?', 'Are any storm overflows discharging near here?', 'What are the most serious alerts in this area?'];

type Example = { question: string; region: string | null };

/** The chips: the shipped guide's questions (each one prepared, so one click answers at
 *  once), else the built-in examples; empty while the shipped file is still loading. */
function examplesFrom(guide: ShippedGuide | null | undefined): Example[] {
  if (guide === undefined) return [];
  if (guide && guide.questions.length) return guide.questions;
  return EXAMPLES.map((question) => ({ question, region: null }));
}

type State = { status: 'idle' } | { status: 'thinking'; since: number; question: string } | { status: 'done'; question: string; answer: HeimdallAnswer } | { status: 'failed'; question: string; reason: string };

/** `focus`: the view circle to ask about (else the regions shown); `areaLabel` says
 *  which; `pick` selects a cited item by key. */
export function AskCard({ gateway, focus, areaLabel, pick }: {
  gateway: HeimdallGateway;
  focus: { lat: number; lon: number; km: number } | null;
  areaLabel: string;
  pick: (key: string) => void;
}) {
  const [state, setState] = React.useState<State>({ status: 'idle' });
  const [question, setQuestion] = React.useState('');
  const [, tick] = React.useState(0);
  // The latest request: a result is applied only if no later question has superseded
  // it and the card is still shown.
  const latest = React.useRef(0);
  React.useEffect(() => () => { latest.current += 1; }, []);
  const [examples, setExamples] = React.useState<Example[]>(() => examplesFrom(shippedGuideNow()));
  React.useEffect(() => {
    let on = true;
    void loadShippedGuide().then((g) => { if (on) setExamples(examplesFrom(g)); });
    return () => { on = false; };
  }, []);
  React.useEffect(() => {
    if (state.status !== 'thinking') return;
    const t = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(t);
  }, [state.status]);
  if (!gateway.ask) return null;

  const ask = (q: string) => {
    const text = q.trim();
    // While Mimir is thinking only a chip can ask (the box and ASK are disabled): it
    // replaces the request in flight, whose answer is then not shown.
    if (text.length < 3) return;
    const request = ++latest.current;
    const live = () => latest.current === request;
    // A shipped answer to this question (any area), or the same question about the same
    // area answered by Mimir in the last two hours: show it at once (its footer keeps the
    // original call's figures). The shipped answers are a local file, loaded before
    // deciding, so a shipped answer never flashes "thinking"; a click meanwhile
    // supersedes this one.
    const key = preparedKey('ask', areaLabel, text);
    const where = focus ? { focus } : undefined;
    void findPrepared<HeimdallAnswer>(shippedKey('ask', '*', text), key).then((prepared) => {
      if (!live()) return;
      if (prepared) {
        setState({ status: 'done', question: text, answer: prepared.value });
        return;
      }
      setState({ status: 'thinking', since: Date.now(), question: text });
      gateway.ask!(text, where).then(
        (answer) => {
          if (answer.answer) putPrepared(key, answer);
          if (live()) setState({ status: 'done', question: text, answer });
        },
        (err) => { if (live()) setState({ status: 'failed', question: text, reason: err instanceof Error ? err.message : String(err) }); },
      );
    });
  };

  return (
    <Card size="sm">
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 8 }}>
        <span className="mg-label" style={{ color: 'var(--accent-hover)' }}>MIMIR</span>
        <span style={{ marginLeft: 'auto', ...mono, color: 'var(--text-faint)', textAlign: 'right' }}>{areaLabel}</span>
      </div>
      <div style={{ display: 'flex', gap: 6 }}>
        <input value={question} onChange={(e) => setQuestion(e.target.value)} placeholder="Ask about what's on the map…" maxLength={400}
          onKeyDown={(e) => { if (e.key === 'Enter') ask(question); }} disabled={state.status === 'thinking'} aria-label="Question for Mimir"
          style={{ flex: 1, minWidth: 0, ...mono, fontSize: 10.5, padding: '6px 8px', background: 'var(--bg-sunken)', color: 'var(--text-body)', border: '1px solid var(--divider)', borderRadius: 'var(--radius-sm)' }} />
        <button type="button" onClick={() => ask(question)} disabled={state.status === 'thinking' || question.trim().length < 3}
          style={{ all: 'unset', cursor: 'pointer', ...mono, padding: '6px 10px', borderRadius: 'var(--radius-sm)', border: '1px solid var(--accent)', color: 'var(--accent-hover)', opacity: state.status === 'thinking' || question.trim().length < 3 ? 0.5 : 1 }}>
          ASK
        </button>
      </div>
      {examples.length > 0 && (
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 8 }}>
          {examples.map(({ question: q, region }) => (
            <button key={q} type="button" onClick={() => { setQuestion(q); ask(q); }} title={region ? `Prepared over ${region}` : undefined}
              style={{ all: 'unset', cursor: 'pointer', ...mono, fontSize: 8.5, padding: '2px 6px', borderRadius: 2, border: '1px solid var(--divider)', color: 'var(--text-muted)', overflowWrap: 'anywhere' }}>{q}</button>
          ))}
        </div>
      )}
      {state.status !== 'idle' && (
        <div style={{ ...mono, color: 'var(--text-faint)', marginTop: 8, overflowWrap: 'anywhere' }}>Q · {state.question.toUpperCase()}</div>
      )}
      {state.status === 'thinking' && (
        <div style={{ ...mono, color: 'var(--accent-hover)', marginTop: 6, lineHeight: 1.6 }}>
          MIMIR IS THINKING · {Math.round((Date.now() - state.since) / 1000)} S · ABOUT A MINUTE ON THIS LAPTOP'S CPU
        </div>
      )}
      {state.status === 'failed' && (
        <div style={{ ...mono, color: 'var(--warn)', marginTop: 6, lineHeight: 1.6, overflowWrap: 'anywhere' }}>NO ANSWER · {state.reason.toUpperCase()}</div>
      )}
      {state.status === 'done' && (() => {
        const a = state.answer;
        return (
          <div style={{ marginTop: 6, borderLeft: `3px solid ${a.answer ? (a.grounded ? 'var(--secure)' : 'var(--warn)') : 'var(--divider)'}`, paddingLeft: 8, minWidth: 0 }}>
            {a.answer
              ? <div style={{ fontSize: 12.5, color: 'var(--text-body)', lineHeight: 1.5, overflowWrap: 'anywhere' }}>{a.answer}</div>
              : <div style={{ ...mono, color: 'var(--text-muted)', lineHeight: 1.6 }}>NO ANSWER FROM THE EVIDENCE{a.refusal ? ` · ${a.refusal.code}${a.refusal.detail ? ` · ${a.refusal.detail.toUpperCase()}` : ''}` : ''}</div>}
            {a.answer && (
              <div style={{ ...mono, marginTop: 4, color: a.grounded ? 'var(--secure)' : 'var(--warn)' }}>
                {a.grounded ? '✓ GROUNDED IN THE CITED ITEMS' : a.citations.length ? '⚠ SOME CITATIONS COULD NOT BE FOUND' : '⚠ NO CITATIONS: TREAT AS UNVERIFIED'}
              </div>
            )}
            {a.citations.length > 0 && (
              <div style={{ display: 'flex', flexWrap: 'wrap', gap: 4, marginTop: 4, minWidth: 0 }}>
                {a.citations.map((c) => {
                  const bad = a.unresolved.includes(c);
                  const summary = c === 'AREA-SUMMARY';
                  return (
                    <button key={c} type="button" disabled={bad || summary} onClick={() => pick(c)} title={summary ? 'The area counts Mimir was given' : bad ? 'Not found in the picture' : `Show ${c}`}
                      style={{ all: 'unset', cursor: bad || summary ? 'default' : 'pointer', ...mono, fontSize: 8.5, padding: '1px 5px', borderRadius: 2, maxWidth: '100%', boxSizing: 'border-box',
                        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', border: `1px solid ${bad ? 'var(--warn)' : 'var(--divider)'}`, color: bad ? 'var(--warn)' : 'var(--text-muted)' }}>
                      {summary ? 'AREA SUMMARY' : c}
                    </button>
                  );
                })}
              </div>
            )}
            {a.inferences.length > 0 && <div style={{ ...mono, color: 'var(--text-faint)', marginTop: 4, lineHeight: 1.5, overflowWrap: 'anywhere' }}>INFERRED, NOT GROUNDED · {a.inferences.join(' · ').toUpperCase()}</div>}
            {a.proposed.length > 0 && <div style={{ ...mono, color: 'var(--text-muted)', marginTop: 4, lineHeight: 1.5, overflowWrap: 'anywhere' }}>SUGGESTED (NOT ACTED ON) · {a.proposed.join(' · ').toUpperCase()}</div>}
            <div style={{ ...mono, fontSize: 8, color: 'var(--text-faint)', marginTop: 6, lineHeight: 1.6 }}>
              ADVISORY · {a.considered} ITEMS AS EVIDENCE · {Math.round(a.ms / 1000)} S · SOVEREIGN MODEL IN THE ENCLOSURE · RECORDED IN THE AUDIT LEDGER
            </div>
          </div>
        );
      })()}
    </Card>
  );
}
