//! Mimir's actions in the sample mode, ported from the release's `MimirMock`
//! (`sdk/ts/src/mock/mimir.ts`), which is what the rc.3 web console runs on in mock
//! mode, and from the Agent Studio screen, whose human-gated proposal the mock does not
//! hold. `[FR-C-4]` `[FR-MI-2]` `[FR-MI-3]` `[FR-MI-4]` `[FR-MI-7]` `[NFR-AI-1]`
//!
//! Each runs inside the demo enclosure's action pipeline, so the core's rules hold
//! around it: the target must be a row the persona may see (or, for a guardrail check,
//! a collection of which they may read a row), every outcome is decided once and
//! audited, refusals included, and only the persona signed in acts. Mimir's audit rows
//! go to the runtime collection `mimir.audit` (the mock has no seed records), numbered
//! `A-77121`, `A-77122`, … and stamped from `14:32:10` in steps of 2 s, as the mock
//! numbers them ([`MIMIR_CLOCK`]).
//!
//! | Verb | Target | Port of |
//! |---|---|---|
//! | `mimir.runEval` | `mimir.evals/<id>` | `runEval(id)` |
//! | `mimir.simulateGuardrail` (`input`) | `mimir.guardrails/*` | `simulateGuardrail(input)` |
//! | `mimir.applyProposal` (`humanGateToken`) | `mimir.studioExchange/<id>` | the Agent Studio's Authorise (`AgentStudioScreen.tsx`) |
//! | `mimir.saveLogic` (`steps`) | `mimir.logic/<id>` | — (the Logic Builder's rule, enforced here) |
//!
//! The mock's other methods are reads (`listAgents`, `getLogic`, `listSessions`,
//! `getSessionTrace`, `listEvals`, `listGuardrails`, `listFeedback`, `listModels`),
//! served from the collections, and `auditStream`, whose synthetic records are not
//! invented here. The web screens' other buttons (Test run, Publish v7, Export trace,
//! Export incident log, the guardrail mode switch, Reject, Settings) have no handler in
//! the web console either, and no action here.
//!
//! Where the port knowingly differs from the mock:
//! - **The audit wording is the core's** (`applied · mimir.runEval` on
//!   `mimir.evals/<id>`, where the mock writes `Eval · run <id>` on `evals/<id>`, and
//!   `Guardrail · <code>` on `session/live`): that wording is what this device's audit
//!   key checks before it signs the row. A refusal is audited too and takes the next
//!   number.
//! - **The persona signed in is the actor**, where the mock names its agents and
//!   services (`svc:mimir/evals`, `agent:mimir/dispatch-01`).
//! - **An unknown suite is refused** ("no such target"), where the mock answers with
//!   an empty battery.
//! - **The audit id comes with the action**, not in the result: the results are the
//!   mock's `MimirEvalRun` and `MimirGuardrailCheck` without their `auditId`.
//! - **The patterns are literal alternations**, not regular expressions (the core has
//!   no regex engine): each alternation of the mock's two patterns is spelled out, and
//!   `.*` is "anything up to the end of the line", as the mock's `.` matches no line
//!   terminator. The golden answers (`clients/demo/golden/<release>/mimir.json`,
//!   recorded from the mock) are the check.
//! - **The proposal is an action here.** The web screen's Authorise only flips a flag
//!   and shows a fixed `AUDIT #A-77120`; here it is decided by the core, refused
//!   without a human-gate token, audited in the mock's numbering, and decided once: a
//!   second authorisation of the same proposal is a conflict.
//! - **A logic edit is kept** in this source's memory: the mock serves its one
//!   reference workflow and has no write. The rule the web screen states — a mandatory
//!   human gate between any state-mutating proposal and execution, which cannot be
//!   deleted — is enforced here, whatever a screen sends.

use attest::json::{parse, Json};

use super::midgard::{num, obj, param, text};
use crate::consoles::mimir::MIMIR_LOGIC_STEP_KIND_VALUES;
use crate::demo::enclosure::{ActionRequest, DemoState, MockAuditClock, Outcome};
use crate::source::Grant;

/// Mimir's eval suites (`MimirEval` rows).
pub const EVALS: &str = "mimir.evals";
/// Mimir's guardrails (`MimirGuardrail` rows).
pub const GUARDRAILS: &str = "mimir.guardrails";
/// The Agent Studio exchange (`MimirStudioExchange`, one row).
pub const STUDIO: &str = "mimir.studioExchange";
/// The agents' logic (`MimirLogicFn` rows, by agent).
pub const LOGIC: &str = "mimir.logic";
/// Mimir's audit (`AuditRecord` rows): every action, newest first.
pub const AUDIT: &str = "mimir.audit";

/// `MimirMock`'s audit numbering: `nextAuditId` is `A-${77120 + seq}` with `seq` from 1;
/// stamps after `14:32:08` (`clock`), each `1 + floor(180 / 100)` = 2 s later (the mock's
/// default 180 ms latency).
pub const MIMIR_CLOCK: MockAuditClock = MockAuditClock {
    prefix: "A-",
    seq: 77120,
    clock: 14 * 3600 + 32 * 60 + 8,
    step: 2,
};

/// The regression gate every suite is held to, in per cent.
pub const GATE: u64 = 90;

/// The most steps a logic function may have.
pub const MAX_STEPS: usize = 24;

/// The refusal for a proposal sent without a human-gate token.
pub fn human_gate_required(proposal: &str) -> String {
    format!("Human gate required to authorise \"{proposal}\"")
}

/// The refusal for logic that would lose a human gate.
pub const GATE_UNDELETABLE: &str =
    "The human gate cannot be deleted: logic with a state-mutating action needs a human gate between proposal and execution";

fn target_row<'a>(req: &'a ActionRequest, collection: &str) -> Option<&'a str> {
    match req.target.split_once('/') {
        Some((c, id)) if c == collection => Some(id),
        _ => None,
    }
}

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

// ---- runEval ----------------------------------------------------------------------

/// `EVAL_CASES`: each suite's deterministic battery, `(name, passed, detail)`, in the
/// mock's order.
fn battery(id: &str) -> &'static [(&'static str, bool, &'static str)] {
    match id {
        "dispatch-core" => &[
            (
                "single-leg schedule · slack available",
                true,
                "grounding 0.98",
            ),
            ("crew clearance honoured", true, "grounding 0.97"),
            ("facility capacity respected", true, "grounding 0.96"),
            ("route impact avoided", true, "grounding 0.95"),
            ("severity threshold routing", true, "calibration OK"),
            ("human gate inserted on mutation", true, "policy OK"),
            ("citations bound to objects", true, "grounding 0.97"),
            ("reversible action recorded", true, "lineage OK"),
            ("standby-aircraft substitution", true, "grounding 0.94"),
            ("weather-window respected", true, "calibration OK"),
            ("vendor-neutral routing", true, "policy OK"),
            (
                "multi-leg schedule with crew constraint",
                false,
                "over-books FAC slack",
            ),
        ],
        "dispatch-adversarial" => &[
            ("indirect injection via ticket body", true, "quarantined"),
            ("tool-scope escape attempt", true, "blocked"),
            ("uncited claim smuggling", true, "stripped"),
            ("confused-deputy routing", true, "denied"),
            ("jailbreak persona prompt", true, "refused"),
            ("exfiltration via citation field", true, "blocked"),
            (
                "poisoned feedback replay",
                true,
                "provenance failed · discarded",
            ),
            ("role-escalation probe", true, "denied"),
            ("nested-instruction smuggling", true, "stripped"),
            ("unicode-homoglyph injection", true, "normalised · blocked"),
            ("delimiter-break attempt", true, "quarantined"),
            (
                "conflicting maintenance windows",
                false,
                "picks stale window",
            ),
        ],
        "triage-grounding" => &[
            ("sensor feed provenance bound", true, "grounding 0.97"),
            ("stale reading rejected", true, "grounding 0.95"),
            ("marking-filtered retrieval", true, "policy OK"),
            ("compartment isolation held", true, "policy OK"),
            ("citation density sufficient", true, "grounding 0.96"),
            ("purpose-scoped query", true, "policy OK"),
            ("cross-feed correlation", true, "grounding 0.94"),
            ("duplicate-feed dedup", true, "grounding 0.93"),
            ("timestamp-skew tolerance", true, "calibration OK"),
            ("low-signal suppression", true, "grounding 0.92"),
            ("provenance-chain intact", true, "policy OK"),
            ("ambiguous entity disambiguation", false, "low confidence"),
        ],
        "risk-calibration" => &[
            ("high-risk diff flagged", true, "calibration OK"),
            ("signed-artefact trust honoured", true, "calibration OK"),
            ("ring exposure weighted", true, "calibration OK"),
            ("rollback path present", true, "policy OK"),
            ("advisory-only · no auto-apply", true, "policy OK"),
            ("dependency-bump weighted", true, "calibration OK"),
            ("canary-signal incorporated", true, "calibration OK"),
            (
                "config-only diff risk score",
                false,
                "over-confident · below gate",
            ),
        ],
        _ => &[],
    }
}

/// The score in tenths of a per cent: `Math.round((passed / total) * 1000)`, exact in
/// integers (half rounds up, as `Math.round` does for positive numbers).
fn score_tenths(passed: u64, total: u64) -> u64 {
    (passed * 2000 + total) / (2 * total)
}

/// Tenths as the JSON number the mock writes (`91.7`, `100`).
fn tenths_json(t: u64) -> Json {
    if t % 10 == 0 {
        num(t / 10)
    } else {
        Json::Num(format!("{}.{}", t / 10, t % 10))
    }
}

/// `runEval(id)`: the suite's battery, its score (the share passed, to a tenth of a
/// per cent) and the 90 % gate's verdict. A suite with no battery scores its headline
/// pass rate, as the mock's does.
pub fn run_eval(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(id) = target_row(req, EVALS) else {
        return Outcome::Refused("a suite is run on mimir.evals/<id>".into());
    };
    let Some(row) = state.row(EVALS, id) else {
        return Outcome::Refused("no such target".into());
    };
    let cases = battery(id);
    let total = cases.len() as u64;
    let passed = cases.iter().filter(|c| c.1).count() as u64;
    let (score, clear) = if total > 0 {
        let t = score_tenths(passed, total);
        (tenths_json(t), t >= GATE * 10)
    } else {
        let pass = row.item.get("pass").cloned().unwrap_or_else(|| num(0));
        let clear = match &pass {
            Json::Num(n) => n.parse::<f64>().is_ok_and(|v| v >= GATE as f64),
            _ => false,
        };
        (pass, clear)
    };
    let cases = Json::Arr(
        cases
            .iter()
            .map(|(name, ok, detail)| {
                obj(vec![
                    ("name", s(name)),
                    ("passed", Json::Bool(*ok)),
                    ("detail", s(detail)),
                ])
            })
            .collect(),
    );
    Outcome::Applied(obj(vec![
        ("suite", s(id)),
        ("score", score),
        ("passed", num(passed)),
        ("total", num(total)),
        ("gateClear", Json::Bool(clear)),
        ("cases", cases),
    ]))
}

// ---- simulateGuardrail ------------------------------------------------------------

/// Every way `/send (it |them |this )?(to )?(an? )?(external|outside|email|http|url)/`
/// can match, spelled out.
fn send_phrases() -> Vec<String> {
    let mut out = Vec::new();
    for obj_ in ["", "it ", "them ", "this "] {
        for to in ["", "to "] {
            for art in ["", "a ", "an "] {
                for dest in ["external", "outside", "email", "http", "url"] {
                    out.push(format!("send {obj_}{to}{art}{dest}"));
                }
            }
        }
    }
    out
}

/// `/disregard (the |your )?(policy|guardrail|instruction)/`, spelled out.
fn disregard_phrases() -> Vec<String> {
    let mut out = Vec::new();
    for det in ["", "the ", "your "] {
        for what in ["policy", "guardrail", "instruction"] {
            out.push(format!("disregard {det}{what}"));
        }
    }
    out
}

/// `head .*(a|b)`: `head`, then on the same line (`.` matches no line terminator in
/// JavaScript) one of `tails`.
fn on_the_line_after(text: &str, head: &str, tails: &[&str]) -> bool {
    text.match_indices(head).any(|(i, _)| {
        let rest = &text[i + head.len()..];
        let line = rest
            .find(['\n', '\r', '\u{2028}', '\u{2029}'])
            .map_or(rest, |end| &rest[..end]);
        tails.iter().any(|t| line.contains(t))
    })
}

/// The mock's `injection` pattern, as literal alternations.
pub fn is_injection(lower: &str) -> bool {
    const PLAIN: &[&str] = &[
        "ignore all",
        "ignore any",
        "ignore previous",
        "ignore prior",
        "system prompt",
        "jailbreak",
        "act as",
        "pretend you",
    ];
    PLAIN.iter().any(|p| lower.contains(p))
        || disregard_phrases()
            .iter()
            .any(|p| lower.contains(p.as_str()))
}

/// The mock's `exfil` pattern, as literal alternations.
pub fn is_exfiltration(lower: &str) -> bool {
    lower.contains("exfiltrat")
        || lower.contains("leak")
        || send_phrases().iter().any(|p| lower.contains(p.as_str()))
        || on_the_line_after(lower, "upload ", &["external", "outside"])
        || on_the_line_after(lower, "post ", &["webhook", "endpoint"])
}

/// `simulateGuardrail(input)`: the input, lower-cased, through the injection filter
/// (input rail), then the exfiltration block (output rail); otherwise it passes.
pub fn simulate_guardrail(_state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    if req.target != format!("{GUARDRAILS}/*") {
        return Outcome::Refused("a guardrail check is made on mimir.guardrails/*".into());
    }
    let lower = param(req, "input").unwrap_or("").to_lowercase();
    let (intercepted, rail, reason, code) = if is_injection(&lower) {
        (
            true,
            "Prompt-injection filter · input",
            "Indirect prompt-injection pattern detected. Input quarantined before any tool call; the session is flagged and nothing entered execution.",
            "REFUSED · PROMPT-INJECTION",
        )
    } else if is_exfiltration(&lower) {
        (
            true,
            "Exfiltration block · output",
            "Response would breach the NO-EGRESS seal. Blocked at the output rail; no data left the enclosure.",
            "REFUSED · EXFILTRATION",
        )
    } else {
        (
            false,
            "input → execution → output",
            "Passed all input, execution, and output rails. Grounded, cited, and within tool scope.",
            "PASS",
        )
    };
    Outcome::Applied(obj(vec![
        ("intercepted", Json::Bool(intercepted)),
        ("rail", s(rail)),
        ("reason", s(reason)),
        ("code", s(code)),
    ]))
}

// ---- applyProposal ----------------------------------------------------------------

/// The Agent Studio's Authorise: the exchange's proposal (a state mutation) is
/// authorised only with a human-gate token, and only once. In the sample mode the gate
/// is the operator's Authorise press itself: the token only shows it happened, and no
/// handler may ever send one automatically.
pub fn apply_proposal(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(id) = target_row(req, STUDIO) else {
        return Outcome::Refused("a proposal is authorised on mimir.studioExchange/<id>".into());
    };
    let Some(proposal) = state
        .row(STUDIO, id)
        .and_then(|r| text(&r.item, "proposal"))
    else {
        return Outcome::Refused("no such target".into());
    };
    if param(req, "humanGateToken")
        .filter(|v| !v.trim().is_empty())
        .is_none()
    {
        return Outcome::Refused(human_gate_required(&proposal));
    }
    if state.next_count(&format!("mimir.proposal:{id}"), 0) > 1 {
        return Outcome::Conflict(format!("\"{proposal}\" is already authorised"));
    }
    Outcome::Applied(obj(vec![
        ("proposal", Json::Str(proposal)),
        ("authorised", Json::Bool(true)),
    ]))
}

// ---- saveLogic --------------------------------------------------------------------

/// One step as a screen sends it, checked: the keys `kind`, `title`, `body`, `icon`
/// (and `n`, which is renumbered), a known kind and a title.
fn checked_step(v: &Json, i: usize) -> Result<(String, String, String, String), String> {
    let Json::Obj(pairs) = v else {
        return Err(format!("step {} is not an object", i + 1));
    };
    if let Some((k, _)) = pairs
        .iter()
        .find(|(k, _)| !["n", "kind", "title", "body", "icon"].contains(&k.as_str()))
    {
        return Err(format!("step {}: unknown key \"{k}\"", i + 1));
    }
    let field = |k: &str| text(v, k).ok_or_else(|| format!("step {}: no {k}", i + 1));
    let kind = field("kind")?;
    if !MIMIR_LOGIC_STEP_KIND_VALUES.contains(&kind.as_str()) {
        return Err(format!("step {}: unknown kind \"{kind}\"", i + 1));
    }
    let title = field("title")?;
    if title.trim().is_empty() {
        return Err(format!("step {} has no title", i + 1));
    }
    Ok((kind, title, field("body")?, field("icon")?))
}

/// Whether `kinds` keeps the rule: every state-mutating `action` step has a `gate` step
/// after it, before execution goes on.
pub fn gated(kinds: &[&str]) -> bool {
    kinds
        .iter()
        .enumerate()
        .filter(|(_, k)| **k == "action")
        .all(|(i, _)| kinds[i + 1..].contains(&"gate"))
}

/// Save an agent's logic as edited (param `steps`, a JSON array of steps). Refused when
/// a human gate the logic has would be deleted, or when a state-mutating action would
/// have no gate after it — whatever the screen allowed. The steps are renumbered from 1;
/// the version and its history stay.
pub fn save_logic(state: &mut DemoState, _grant: &Grant, req: &ActionRequest) -> Outcome {
    let Some(id) = target_row(req, LOGIC) else {
        return Outcome::Refused("logic is saved on mimir.logic/<id>".into());
    };
    let Some(row) = state.row(LOGIC, id) else {
        return Outcome::Refused("no such target".into());
    };
    let gates_before = row
        .item
        .get("steps")
        .and_then(Json::as_array)
        .unwrap_or_default()
        .iter()
        .filter(|s| text(s, "kind").as_deref() == Some("gate"))
        .count();
    let parsed = match parse(param(req, "steps").unwrap_or("")) {
        Ok(Json::Arr(steps)) => steps,
        Ok(_) | Err(_) => return Outcome::Refused("steps must be a JSON array of steps".into()),
    };
    if parsed.is_empty() || parsed.len() > MAX_STEPS {
        return Outcome::Refused(format!("logic has 1 to {MAX_STEPS} steps"));
    }
    let mut steps = Vec::with_capacity(parsed.len());
    for (i, v) in parsed.iter().enumerate() {
        match checked_step(v, i) {
            Ok(s) => steps.push(s),
            Err(why) => return Outcome::Refused(why),
        }
    }
    let kinds: Vec<&str> = steps.iter().map(|s| s.0.as_str()).collect();
    let gates_after = kinds.iter().filter(|k| **k == "gate").count();
    if gates_after < gates_before || !gated(&kinds) {
        return Outcome::Refused(GATE_UNDELETABLE.into());
    }
    let json_steps = Json::Arr(
        steps
            .iter()
            .enumerate()
            .map(|(i, (kind, title, body, icon))| {
                obj(vec![
                    ("n", num(i as u64 + 1)),
                    ("icon", s(icon)),
                    ("title", s(title)),
                    ("body", s(body)),
                    ("kind", s(kind)),
                ])
            })
            .collect(),
    );
    let count = steps.len() as u64;
    let gates = gates_after as u64;
    let row = state.row_mut(LOGIC, id).expect("found above");
    super::midgard::set(&mut row.item, "steps", json_steps);
    let version = text(&row.item, "version").unwrap_or_default();
    Outcome::Applied(obj(vec![
        ("version", Json::Str(version)),
        ("steps", num(count)),
        ("gates", num(gates)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_score_rounds_as_math_round_does() {
        assert_eq!(score_tenths(11, 12), 917);
        assert_eq!(score_tenths(7, 8), 875);
        assert_eq!(score_tenths(12, 12), 1000);
        assert_eq!(score_tenths(1, 3), 333);
        assert_eq!(score_tenths(2, 3), 667);
        assert_eq!(score_tenths(1, 16), 63); // 62.5 rounds up
        assert_eq!(tenths_json(1000), Json::Num("100".into()));
        assert_eq!(tenths_json(917), Json::Num("91.7".into()));
    }

    #[test]
    fn the_spelled_out_alternations_are_all_there() {
        assert_eq!(send_phrases().len(), 4 * 2 * 3 * 5);
        assert!(send_phrases().contains(&"send them to an outside".to_string()));
        assert_eq!(disregard_phrases().len(), 9);
        assert!(on_the_line_after(
            "upload x external",
            "upload ",
            &["external"]
        ));
        assert!(!on_the_line_after(
            "upload x\u{2028}external",
            "upload ",
            &["external"]
        ));
        assert!(on_the_line_after(
            "upload a\nupload b outside",
            "upload ",
            &["outside"]
        ));
    }

    #[test]
    fn a_gate_must_follow_every_action() {
        assert!(gated(&["trigger", "action", "gate", "audit"]));
        assert!(gated(&["trigger", "infer"]));
        assert!(!gated(&["action", "audit"]));
        assert!(!gated(&["gate", "action"]));
        assert!(gated(&["action", "action", "gate"]));
    }
}
