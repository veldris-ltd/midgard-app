//! Static platform read models the HTTP edge still serves from fixed data:
//! identities, keys, compute, observability, resilience and the SDK contract
//! surface. Each belongs to a subsystem (IAM, KMS, the scheduler, telemetry) that is
//! not yet wired to this edge.
//!
//! The object fabric and the governance spine are **not** here. Object types,
//! queries, search-around, actions, policies, the policy dry-run and the audit ledger
//! are served from the real `fabric` and `governance` crates — see `enclosure.rs`.
//! Those reads are caller-scoped, which is why they are reachable only through
//! `SecuredGateway` and never through the unauthenticated [`route`](crate::route).
//! JSON field names are camelCase to match `@midgard/sdk` types. [AC-M-4]

use crate::json::Json;

/// The static platform read models. Holds no state: every read is a fixed view.
pub struct State;

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    /// The static platform read models.
    pub fn new() -> State {
        State
    }

    // ---- platform read models (static) --------------------------------------

    pub fn identities(&self) -> Json {
        fn id(
            subject: &str,
            kind: &str,
            clearance: &str,
            roles: &[&str],
            groups: &[&str],
            status: &str,
            last: &str,
        ) -> Json {
            Json::obj(vec![
                ("subject", Json::s(subject)),
                ("kind", Json::s(kind)),
                ("clearance", Json::s(clearance)),
                ("roles", Json::arr_str(roles)),
                ("groups", Json::arr_str(groups)),
                ("status", Json::s(status)),
                ("lastSeen", Json::s(last)),
            ])
        }
        Json::Arr(vec![
            id(
                "j.benjamin",
                "user",
                "confidential",
                &["operator", "analyst"],
                &["ops-north"],
                "active",
                "14:31",
            ),
            id(
                "k.rao",
                "user",
                "restricted",
                &["analyst"],
                &["fusion"],
                "active",
                "13:58",
            ),
            id(
                "agent:mimir/dispatch-01",
                "agent",
                "confidential",
                &["agent-runtime"],
                &["mimir"],
                "active",
                "live",
            ),
            id(
                "svc:forge/projector",
                "service",
                "internal",
                &["projector"],
                &["forge"],
                "active",
                "live",
            ),
            id(
                "a.stone",
                "user",
                "internal",
                &["auditor"],
                &["governance"],
                "suspended",
                "2d ago",
            ),
        ])
    }

    pub fn keys(&self) -> Json {
        fn k(
            id: &str,
            purpose: &str,
            alg: &str,
            custody: &str,
            wrapped: bool,
            rotated: &str,
            status: &str,
        ) -> Json {
            Json::obj(vec![
                ("id", Json::s(id)),
                ("purpose", Json::s(purpose)),
                ("alg", Json::s(alg)),
                ("custody", Json::s(custody)),
                ("wrapped", Json::Bool(wrapped)),
                ("rotated", Json::s(rotated)),
                ("status", Json::s(status)),
            ])
        }
        Json::Arr(vec![
            k(
                "kek-root",
                "Root KEK",
                "ML-KEM-1024",
                "enclosure",
                false,
                "2026-06-01",
                "active",
            ),
            k(
                "dek-ops-n",
                "OPS-N data key",
                "AES-256-GCM",
                "enclosure",
                true,
                "2026-07-08",
                "rotating",
            ),
            k(
                "dek-sig",
                "SIG data key",
                "AES-256-GCM",
                "hsm",
                true,
                "2026-07-02",
                "active",
            ),
            k(
                "sig-svc-mesh",
                "Service mesh auth",
                "ML-DSA-65",
                "enclosure",
                false,
                "2026-07-09",
                "active",
            ),
        ])
    }

    pub fn compute(&self) -> Json {
        fn j(
            id: &str,
            kind: &str,
            backend: &str,
            state: &str,
            progress: i64,
            submitted: &str,
        ) -> Json {
            Json::obj(vec![
                ("id", Json::s(id)),
                ("kind", Json::s(kind)),
                ("backend", Json::s(backend)),
                ("state", Json::s(state)),
                ("progress", Json::Int(progress)),
                ("submitted", Json::s(submitted)),
            ])
        }
        Json::Arr(vec![
            j(
                "JOB-3391",
                "Typed query",
                "in-process",
                "running",
                62,
                "14:31",
            ),
            j(
                "JOB-3390",
                "Function apply",
                "in-process",
                "done",
                100,
                "14:29",
            ),
            j(
                "JOB-3389",
                "Projection rebuild",
                "forge-engine",
                "queued",
                0,
                "14:33",
            ),
            j(
                "JOB-3388",
                "Marking reindex",
                "in-process",
                "failed",
                41,
                "14:22",
            ),
        ])
    }

    pub fn observability(&self) -> Json {
        fn m(label: &str, value: &str, target: &str, healthy: bool) -> Json {
            Json::obj(vec![
                ("label", Json::s(label)),
                ("value", Json::s(value)),
                ("target", Json::s(target)),
                ("healthy", Json::Bool(healthy)),
            ])
        }
        Json::Arr(vec![
            m("CONTROL-PLANE SLO", "99.97%", "99.9%", true),
            m("QUERY P95", "38ms", "<100ms", true),
            m("LINEAGE SIGNED", "100%", "100%", true),
            m("PQ HANDSHAKE FAIL", "0", "0", true),
            m("DENY-BY-DEFAULT HITS", "1,204", "—", true),
        ])
    }

    pub fn resilience(&self) -> Json {
        fn r(id: &str, ring: &str, version: &str, progress: i64, state: &str, gate: &str) -> Json {
            Json::obj(vec![
                ("id", Json::s(id)),
                ("ring", Json::s(ring)),
                ("version", Json::s(version)),
                ("progress", Json::Int(progress)),
                ("state", Json::s(state)),
                ("healthGate", Json::s(gate)),
            ])
        }
        Json::Arr(vec![
            r("RLT-fabric", "canary", "v1.4.2", 100, "healthy", "pass"),
            r("RLT-gateway", "broad", "v1.4.2", 74, "in-progress", "watch"),
            r("RLT-governance", "broad", "v1.4.1", 100, "healthy", "pass"),
            r("RLT-kms", "canary", "v1.4.3-rc1", 20, "pending", "pass"),
        ])
    }

    pub fn contract_surface(&self) -> Json {
        fn rpc(name: &str, req: &str, res: &str, note: &str) -> Json {
            Json::obj(vec![
                ("name", Json::s(name)),
                ("request", Json::s(req)),
                ("response", Json::s(res)),
                ("note", Json::s(note)),
            ])
        }
        Json::Arr(vec![
            Json::obj(vec![
                ("service", Json::s("midgard.v1.ObjectFabric")),
                (
                    "rpcs",
                    Json::Arr(vec![
                        rpc(
                            "Query",
                            "QueryRequest",
                            "QueryResponse",
                            "Typed query under policy; marking-filtered + audited.",
                        ),
                        rpc(
                            "ApplyAction",
                            "ApplyActionRequest",
                            "ApplyActionResponse",
                            "Function-backed action; human-gated; returns audit id.",
                        ),
                    ]),
                ),
            ]),
            Json::obj(vec![
                ("service", Json::s("midgard.v1.common")),
                (
                    "rpcs",
                    Json::Arr(vec![
                        rpc(
                            "Marking",
                            "—",
                            "—",
                            "Classification tag (level + compartments).",
                        ),
                        rpc(
                            "PolicyContext",
                            "—",
                            "—",
                            "Subject / purpose / clearance / roles on every call.",
                        ),
                        rpc(
                            "AuditRecord",
                            "—",
                            "—",
                            "ML-DSA-signed append-only lineage entry.",
                        ),
                    ]),
                ),
            ]),
        ])
    }
}
