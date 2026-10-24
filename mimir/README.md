# mimir/ — the local AI plane & agent runtime (Phases 2–3)

Sovereign, in-enclosure inference and the governed agent runtime every product's
AI calls. Source: `SAD_02_Mimir.md`, `SAD_02b_Mimir_Model_Supply_Resolution.md`.

| Directory | Role | Stack | Provenance |
|---|---|---|---|
| `model-server/` | Local serving: typed API, batching, KV-cache, quantisation, PQ-mTLS + per-call audit | Rust control plane; GPU engine out-of-process | `FR-MI-1` `FR-MI-8` |
| `model-abstraction/` | Per-family adapters (Mistral/Nemotron/Gemma/Granite); templating/stop-tokens | Python | `FR-MS-7` |
| `model-registry/` | System of record; fail-closed load; hw + licence gates | Rust/Python | `FR-MS-3..6` |
| `agent-runtime/` | 8-layer assembly, OAG response, governed loop (INV-1..8), self-* loops, builders | Rust | `NFR-AI-1` `FR-MI-3/4/5/7` |
| `oag-retriever/` | Ontology-augmented grounding: A.2 planning + cited fabric retrieval | Rust | `FR-MI-2` |
| `prompts/` | The verbatim A.1–A.5 system-prompt library (`mimir-prompts`) | Rust | SAD_05 Part A |
| `evals/` | The EV-* Eval suites (regression for AI behaviour) | Python | `NFR-AI-1` |

> **KDD — model-server is a Rust control plane, not a Python service.** The C4
> containers diagram notes "Python vLLM" for serving; the enclosure's
> sovereignty/air-gap/bit-reproducibility posture takes precedence. The security-
> critical control plane (typed API, batching, KV-cache, quantisation accounting,
> PQ-mTLS termination, per-call audit) is lean-std Rust; the actual GPU engine
> (vLLM/TensorRT-LLM class) is an **out-of-process backend behind a typed
> `ModelBackend` seam**, selected **autonomously from the node's hardware** at
> bootstrap and degrading to the deterministic in-enclosure reference engine where
> no GPU is present. The reference engine proves AC-MI-1..8 offline; the GPU engine
> is delivered/verified like a model artefact (Phase 3/5). See
> `memory/mimir-model-server-seam`.
>
> **Status.** Built: `model-server/` (Prompt 2.b — FR-MI-1, FR-MI-8; AC-MI-1,
> AC-MI-8), `prompts/` + `oag-retriever/` + the `agent-runtime/` assembly pipeline
> and OAG-grounded response (Prompt 2.c — FR-MI-2; AC-MI-2), and the
> `agent-runtime/` **core agent (A.1) + governed plan-act-observe loop** enforcing
> envelope invariants INV-1..8 with the A.1 starter Evals EV-A1-* (Prompt 2.d —
> FR-MI-3; AC-MI-3). State-changing actions are proposed, Evals-checked, and
> human-gated unless pre-authorised for a reversible, evaluated scope. The three
> **self-\* loops** are wired (Prompt 2.e — FR-MI-4/5/7; AC-MI-4/5/6):
> self-defending (A.3) blocks prompt-injection at assembly layer 8 before any tool
> executes; self-healing (A.4) retries/reroutes/rolls back within budget and
> escalates to a human on security events; self-learning (A.5) distils governed
> feedback into retrieval/prompt/policy updates with provenance and never weight
> changes. EV-A3/A4/A5 run per model family (Mistral/Nemotron/Gemma/Granite). The no-code
> **Logic builder** and **pro-code builder** produce the same agent contract, and
> model artefacts **verify-on-load** (ML-DSA signature + licence + hardware, fail
> closed) (Prompt 2.f — FR-MI-4; AC-MI-7). **Phase 2 is complete: AC-MI-1..8 and
> EV-A1..A5 all pass** (`just verify-mimir`, green under `docker --network none`).
> Phase 3 adds the Model Registry + air-gapped supply chain (AC-MS-1..9); other
> directories follow.
