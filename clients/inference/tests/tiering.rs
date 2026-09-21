//! AC-C-5 (Tier-1 on capable-offline / graceful disable on weak-offline), AC-C-6
//! (remote delegation is grounded + cited), and the client-prompt Evals.
#![cfg(feature = "test-backend")]

use midgard_client_inference::evals::{
    ev_ac1_clearance, ev_ac1_offline, ev_ac2_policy, ev_ac2_privacy, ev_ac2_toobig, ev_ac_parity,
};
use midgard_client_inference::{
    serve, Connectivity, DeviceCapability, FixedReasoner, InferenceRequest, InferenceResult,
    MockOnDeviceRuntime, MockRemoteInference, OnDeviceRuntime, PrivacyFirstReasoner,
    PrivacyPreference, RemoteInference, TaskComplexity, Tier, TierContext, TierSource,
};

fn cap(ram: u32, model: bool) -> DeviceCapability {
    DeviceCapability {
        ram_mb: ram,
        has_accelerator: model,
        on_device_model: model,
    }
}

fn ctx(
    capability: DeviceCapability,
    connectivity: Connectivity,
    allow_egress: bool,
    privacy: PrivacyPreference,
    complexity: TaskComplexity,
) -> TierContext {
    TierContext {
        capability,
        connectivity,
        allow_egress,
        privacy,
        complexity,
    }
}

fn req() -> InferenceRequest {
    InferenceRequest::new("summarise case:1", vec!["obj:1".into()])
}

const RT: MockOnDeviceRuntime = MockOnDeviceRuntime;
const RM: MockRemoteInference = MockRemoteInference;

// --- AC-C-5 ---------------------------------------------------------------

#[test]
fn capable_device_offline_runs_tier_1() {
    let c = ctx(
        cap(8192, true),
        Connectivity::Offline,
        true,
        PrivacyPreference::Standard,
        TaskComplexity::Moderate,
    );
    let served = serve(&c, &req(), Some(&RT), Some(&RM), &PrivacyFirstReasoner).unwrap();
    assert_eq!(served.decision.tier, Tier::OnDevice);
    assert!(matches!(served.result, InferenceResult::OnDevice(_)));
    assert!(
        !served.audit.data_left_device,
        "Tier 1 keeps data on device"
    );
}

#[test]
fn weak_device_offline_disables_gracefully() {
    let c = ctx(
        cap(1024, false),
        Connectivity::Offline,
        true,
        PrivacyPreference::Standard,
        TaskComplexity::Light,
    );
    // No runtime, no remote — a weak offline device. It must NOT error.
    let served = serve::<MockOnDeviceRuntime, MockRemoteInference>(
        &c,
        &req(),
        None,
        None,
        &PrivacyFirstReasoner,
    )
    .unwrap();
    assert_eq!(served.decision.tier, Tier::Disable);
    assert!(matches!(served.result, InferenceResult::Disabled { .. }));
}

// --- AC-C-6 ---------------------------------------------------------------

#[test]
fn connected_heavy_task_delegates_remote_grounded_and_cited() {
    let c = ctx(
        cap(1024, false), // not capable locally
        Connectivity::Online,
        true,
        PrivacyPreference::Standard,
        TaskComplexity::Heavy,
    );
    let served = serve(&c, &req(), Some(&RT), Some(&RM), &PrivacyFirstReasoner).unwrap();
    assert_eq!(served.decision.tier, Tier::Remote);
    match &served.result {
        InferenceResult::Remote(out) => {
            assert!(out.grounded, "remote result must be grounded");
            assert!(!out.citations.is_empty(), "remote result must be cited");
        }
        other => panic!("expected remote, got {other:?}"),
    }
    assert!(served.audit.data_left_device);
    assert!(served.audit.grounded);
}

#[test]
fn connected_capable_heavy_task_is_hybrid() {
    let c = ctx(
        cap(8192, true),
        Connectivity::Online,
        true,
        PrivacyPreference::Standard,
        TaskComplexity::Heavy,
    );
    let served = serve(&c, &req(), Some(&RT), Some(&RM), &PrivacyFirstReasoner).unwrap();
    assert_eq!(served.decision.tier, Tier::Hybrid);
    assert!(matches!(served.result, InferenceResult::Hybrid { .. }));
}

// --- Evals ----------------------------------------------------------------

#[test]
fn ev_ac1_offline_and_clearance() {
    // Offline task needing remote data.
    let mut r = req();
    r.needs_remote_data = true;
    let out = RT.infer("prompt", &r).unwrap();
    assert!(ev_ac1_offline(&out).passed());

    // Above-clearance request.
    let mut r2 = req();
    r2.above_clearance = true;
    let out2 = RT.infer("prompt", &r2).unwrap();
    assert!(ev_ac1_clearance(&out2).passed());
}

#[test]
fn ev_ac2_privacy_keeps_sensitive_on_device() {
    // Sensitive data (no egress) on a capable device → on-device, no egress.
    let c = ctx(
        cap(8192, true),
        Connectivity::Online,
        false,
        PrivacyPreference::Standard,
        TaskComplexity::Moderate,
    );
    let served = serve(&c, &req(), Some(&RT), Some(&RM), &PrivacyFirstReasoner).unwrap();
    assert!(ev_ac2_privacy(&served).passed());
    assert_eq!(served.decision.source, TierSource::Heuristic);
}

#[test]
fn ev_ac2_toobig_online_remote_offline_disable() {
    let online = ctx(
        cap(1024, false),
        Connectivity::Online,
        true,
        PrivacyPreference::Standard,
        TaskComplexity::Heavy,
    );
    let s_online = serve(&online, &req(), Some(&RT), Some(&RM), &PrivacyFirstReasoner).unwrap();
    assert!(ev_ac2_toobig(&s_online, false).passed());

    let offline = ctx(
        cap(1024, false),
        Connectivity::Offline,
        true,
        PrivacyPreference::Standard,
        TaskComplexity::Heavy,
    );
    let s_offline = serve::<MockOnDeviceRuntime, MockRemoteInference>(
        &offline,
        &req(),
        None,
        None,
        &PrivacyFirstReasoner,
    )
    .unwrap();
    assert!(ev_ac2_toobig(&s_offline, true).passed());
}

#[test]
fn ev_ac2_policy_never_egresses_under_no_egress() {
    // Even a remote-biased reasoner is never consulted under a no-egress policy:
    // the heuristic decides on-device before the model weighs in.
    let c = ctx(
        cap(8192, true),
        Connectivity::Online,
        false, // no egress
        PrivacyPreference::Standard,
        TaskComplexity::Moderate,
    );
    let biased = FixedReasoner::new(Tier::Remote, true);
    let served = serve(&c, &req(), Some(&RT), Some(&RM), &biased).unwrap();
    assert!(ev_ac2_policy(&served).passed());
    assert_eq!(served.decision.tier, Tier::OnDevice);
    assert_eq!(served.decision.source, TierSource::Heuristic);
}

#[test]
fn ev_ac_parity_tier1_matches_tier2() {
    let r = req();
    let on_device = RT.infer("prompt", &r).unwrap();
    let remote = RM.infer(&r).unwrap();
    assert!(ev_ac_parity(&on_device, &remote).passed());
}
