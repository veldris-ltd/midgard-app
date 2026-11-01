//! VLD-ANB-MIDGARD-0041: a captured signed observation cannot be replayed into the
//! picture. After its signature verifies, an observation's per-feed sequence must
//! advance, its `observed_at` must be inside the freshness window around the
//! gateway's clock, and its id must not be in the feed's bounded window of recent ids;
//! a signature failure consumes none of that state; and over PQ-mTLS a peer submits
//! only its own feed's observations, or a feed delegated to it. Fixtures as in
//! `ac_h_2.rs`, with a fixed clock of 1_000. [FR-H-3] [NFR-PQ-3] [AC-59.1] [AC-59.2]
//! [AC-59.3] [AC-59.4] [AC-59.5]
#![cfg(feature = "backend-fips")]

use std::collections::BTreeSet;
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use iam::{Idp, IDP_SIG_ALG};
use policy::{
    Action, ClassificationLevel, DefaultPdp, Effect, Marking, PolicySet, Purpose, Resource,
    ResourceId, Role, Rule, RuleId, Selector,
};
use pq_mtls::{IdentityCert, Initiator, Posture, Responder, ServiceId, WorkloadIdentity};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::registry::{shared, CryptoProvider};
use pqc_agility::{Rng, SharedProvider, SigId, SignatureScheme, SigningKey, VerifyingKey};

use heimdall_feed_gateway::{
    serve_feed_connection, sign_observation, FeedClient, FeedError, FeedGateway, FeedRegistry,
    FeedSequencer, FeedTransportError, FreshnessPolicy, ReplayGuard, SignedObservation,
    MAX_SEEN_PER_FEED,
};
use heimdall_picture::{Attr, FeedId, Observation, ObservationId};

const ALG: SigId = SigId::MlDsa65;
/// The gateway's fixed clock.
const NOW: u64 = 1_000;
/// The window the book's acceptance criteria are stated in: 300 s behind, 60 s ahead.
const WINDOW: FreshnessPolicy = FreshnessPolicy {
    max_age_secs: 300,
    max_future_skew_secs: 60,
};

/// A deterministic seeded RNG (SplitMix64), test-only, matches the suite's pattern.
struct SeedRng(u64);
impl Rng for SeedRng {
    fn fill(&mut self, buf: &mut [u8]) {
        let mut i = 0;
        while i < buf.len() {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            let b = z.to_le_bytes();
            let take = (buf.len() - i).min(8);
            buf[i..i + take].copy_from_slice(&b[..take]);
            i += take;
        }
    }
}

fn provider() -> SharedProvider {
    let mut p = CryptoProvider::new();
    p.register_sig(Box::new(FipsSig::new(ALG)));
    shared(p)
}

fn keypair(seed: u64) -> (VerifyingKey, SigningKey) {
    FipsSig::new(ALG).keypair(&mut SeedRng(seed)).unwrap()
}

fn feed(name: &str) -> FeedId {
    FeedId(name.into())
}

fn observation(feed_name: &str, id: &str, sequence: u64, observed_at: u64) -> Observation {
    Observation::new(id, feed(feed_name), "air-track", "trk-9", observed_at)
        .with_attr("speed", Attr::Float(480.0))
        .with_sequence(sequence)
}

fn sign(obs: Observation, sk: &SigningKey) -> SignedObservation {
    sign_observation(&FipsSig::new(ALG), sk, obs).unwrap()
}

/// A gateway trusting radar-1, at the fixed clock, under the book's window.
fn gateway() -> (FeedGateway, SigningKey) {
    let (vk, sk) = keypair(1);
    let mut registry = FeedRegistry::new();
    registry.register(feed("radar-1"), vk);
    let gateway = FeedGateway::new(registry, provider())
        .with_clock(|| NOW)
        .with_freshness(WINDOW);
    (gateway, sk)
}

/// The same signed observation twice: the second is not monotonic and yields no
/// `VerifiedObservation`. An observation re-signed with a higher sequence but the
/// same id meets the id window instead. [AC-59.1]
#[test]
fn an_observation_replayed_with_the_same_signature_is_refused() {
    let (gateway, sk) = gateway();
    let signed = sign(observation("radar-1", "obs-1", 1, NOW), &sk);
    assert!(gateway.authenticate(&signed).is_ok(), "first copy admitted");
    assert_eq!(
        gateway.authenticate(&signed),
        Err(FeedError::SequenceNotMonotonic {
            feed: feed("radar-1"),
            got: 1,
            last: 1,
        })
    );

    // Only the id check applies when the sequence moves on: still refused.
    let same_id = sign(observation("radar-1", "obs-1", 2, NOW), &sk);
    assert_eq!(
        gateway.authenticate(&same_id),
        Err(FeedError::Replayed {
            feed: feed("radar-1"),
            id: ObservationId("obs-1".into()),
        })
    );
    assert_eq!(gateway.replay_guard().counters(), (1, 2));
}

/// Sequences must strictly increase per feed: equal or lower is refused. [AC-59.1]
#[test]
fn a_lower_or_equal_sequence_is_refused() {
    let (gateway, sk) = gateway();
    assert!(gateway
        .authenticate(&sign(observation("radar-1", "a", 5, NOW), &sk))
        .is_ok());
    for (id, sequence) in [("b", 5), ("c", 3)] {
        assert_eq!(
            gateway.authenticate(&sign(observation("radar-1", id, sequence, NOW), &sk)),
            Err(FeedError::SequenceNotMonotonic {
                feed: feed("radar-1"),
                got: sequence,
                last: 5,
            })
        );
    }
    assert!(gateway
        .authenticate(&sign(observation("radar-1", "d", 6, NOW), &sk))
        .is_ok());
    assert_eq!(gateway.replay_guard().last_sequence(&feed("radar-1")), 6);
}

/// Sequence 0 means unset (an observation built without a sequencer). [AC-59.1]
#[test]
fn sequence_zero_is_refused() {
    let (gateway, sk) = gateway();
    assert_eq!(
        gateway.authenticate(&sign(observation("radar-1", "a", 0, NOW), &sk)),
        Err(FeedError::SequenceUnset(feed("radar-1")))
    );
    assert_eq!(gateway.replay_guard().last_sequence(&feed("radar-1")), 0);
}

/// At a clock of 1_000 with a 300 s age and 60 s skew: 500 and 1_100 are stale,
/// 1_050 is admitted. A stale observation does not consume its sequence. [AC-59.2]
#[test]
fn an_observation_outside_the_freshness_window_is_refused() {
    let (gateway, sk) = gateway();
    for (id, observed_at) in [("old", 500), ("ahead", 1_100)] {
        assert_eq!(
            gateway.authenticate(&sign(observation("radar-1", id, 1, observed_at), &sk)),
            Err(FeedError::Stale {
                feed: feed("radar-1"),
                observed_at,
                now: NOW,
            })
        );
    }
    let admitted = gateway
        .authenticate(&sign(observation("radar-1", "near", 1, 1_050), &sk))
        .expect("inside the window");
    assert_eq!(admitted.observation().observed_at, 1_050);
    // The window's edges are inclusive.
    assert!(WINDOW.admits(700, NOW) && WINDOW.admits(1_060, NOW));
    assert!(!WINDOW.admits(699, NOW) && !WINDOW.admits(1_061, NOW));
}

/// A tampered observation fails its signature and leaves no trace in the replay
/// state: the genuine one with the same sequence is then admitted. [AC-59.3]
#[test]
fn a_signature_failure_does_not_consume_state() {
    let (gateway, sk) = gateway();
    let genuine = sign(observation("radar-1", "obs-1", 1, NOW), &sk);
    let mut tampered = genuine.clone();
    tampered
        .observation
        .attrs
        .insert("speed".into(), Attr::Float(50.0));
    assert_eq!(
        gateway.authenticate(&tampered),
        Err(FeedError::SignatureInvalid(feed("radar-1")))
    );
    assert_eq!(gateway.replay_guard().last_sequence(&feed("radar-1")), 0);
    assert_eq!(gateway.replay_guard().counters(), (0, 0));
    assert!(gateway.authenticate(&genuine).is_ok(), "genuine admitted");
}

/// 5_000 distinct ids keep the feed's window at 4_096: the oldest are forgotten, the
/// most recent still refused. [AC-59.5]
#[test]
fn the_seen_window_is_bounded() {
    let guard = ReplayGuard::new();
    let radar = feed("radar-1");
    for n in 0..5_000u64 {
        guard
            .check_and_record(
                &radar,
                &ObservationId(format!("obs-{n}")),
                n + 1,
                NOW,
                NOW,
                &WINDOW,
            )
            .unwrap();
    }
    assert_eq!(MAX_SEEN_PER_FEED, 4_096);
    assert_eq!(guard.seen_len(&radar), 4_096);
    // obs-4999 is in the window; obs-0 has been forgotten.
    assert_eq!(
        guard.check_and_record(
            &radar,
            &ObservationId("obs-4999".into()),
            5_001,
            NOW,
            NOW,
            &WINDOW
        ),
        Err(FeedError::Replayed {
            feed: radar.clone(),
            id: ObservationId("obs-4999".into()),
        })
    );
    assert!(guard
        .check_and_record(
            &radar,
            &ObservationId("obs-0".into()),
            5_001,
            NOW,
            NOW,
            &WINDOW
        )
        .is_ok());
    assert_eq!(guard.seen_len(&radar), 4_096);
}

/// A feed sequencer counts from 1, resumes after a saved value, and from the clock
/// starts above any second-resolution sequence. [AC-59.6]
#[test]
fn a_feed_sequencer_is_monotonic() {
    let s = FeedSequencer::new();
    assert_eq!((s.next(), s.next(), s.next()), (1, 2, 3));
    assert_eq!(FeedSequencer::resume_after(41).next(), 42);
    let clock = FeedSequencer::from_clock();
    let (a, b) = (clock.next(), clock.next());
    assert!(a > 1_000_000_000_000_000_000 && b == a + 1);
}

// ---- over a loopback PQ-mTLS connection ------------------------------------

/// A pq-mtls workload identity certified by the IdP (the channel identity).
fn workload(idp: &Idp, id: &str, roles: &[&str], seed: u64) -> (IdentityCert, SigningKey) {
    let (vk, sk) = idp
        .provider()
        .sig_for(IDP_SIG_ALG)
        .unwrap()
        .keypair(&mut SeedRng(seed))
        .unwrap();
    let identity = WorkloadIdentity {
        id: ServiceId(id.into()),
        roles: roles.iter().map(|r| Role(r.to_string())).collect(),
        sig_alg: IDP_SIG_ALG,
        verifying_key: vk,
    };
    let terms =
        pq_mtls::CertTerms::new(1, 0, u64::MAX, Posture::at(ClassificationLevel::TOP_SECRET));
    (idp.certify(&identity, terms).unwrap(), sk)
}

fn channel_resource() -> Resource {
    Resource {
        id: ResourceId("channel".into()),
        marking: Marking {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
        allowed_purposes: [Purpose("investigation".into())].into_iter().collect(),
    }
}

fn feed_pdp() -> DefaultPdp {
    DefaultPdp::new(
        PolicySet::new(vec![Rule {
            id: RuleId("feed-channel".into()),
            role: Role("feed".into()),
            action: Action("OpenChannel".into()),
            purpose: None,
            selector: Selector::Any,
            effect: Effect::Permit,
        }])
        .unwrap(),
    )
}

/// Serve one feed connection through `gateway` while the channel identity
/// `feed-source-1` sends `signed`; the listener's result and how many it admitted.
fn submit_over_pqmtls(
    gateway: FeedGateway,
    signed: SignedObservation,
) -> (Result<usize, FeedTransportError>, usize) {
    let idp = Idp::generate("idp", &mut SeedRng(1)).unwrap();
    let anchor = idp.trust_anchor();
    let (source_cert, source_sk) = workload(&idp, "feed-source-1", &["feed"], 2);
    let (node_cert, node_sk) = workload(&idp, "node-a", &["node"], 3);

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server_anchor = anchor.clone();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let provider = fips_provider();
        let pdp = feed_pdp();
        let mut rng = SeedRng(101);
        let responder = Responder::new(
            &provider,
            node_cert,
            &node_sk,
            server_anchor,
            &pdp,
            channel_resource(),
            &mut rng,
        );
        let mut admitted = 0usize;
        let result = serve_feed_connection(stream, responder, &gateway, |_| admitted += 1);
        (result, admitted)
    });

    {
        let provider = fips_provider();
        let mut rng = SeedRng(102);
        let init = Initiator::new(
            &provider,
            source_cert,
            &source_sk,
            anchor,
            ServiceId("node-a".into()),
            Purpose("investigation".into()),
            Posture {
                level: ClassificationLevel::UNCLASSIFIED,
                compartments: BTreeSet::new(),
            },
            &mut rng,
        );
        let stream = TcpStream::connect(addr).unwrap();
        let mut client = FeedClient::connect(stream, init).expect("feed client connects");
        client.send(&signed).expect("observation sent");
    }
    server.join().unwrap()
}

/// The channel peer feed-source-1 sends an observation for feed-source-2, correctly
/// signed with feed-source-2's key: refused as NotFromPeer; with feed-source-2
/// delegated to feed-source-1 in the registry, admitted. [AC-59.4]
#[test]
fn a_feed_cannot_submit_another_feeds_observations() {
    let (other_vk, other_sk) = keypair(7);
    let signed = sign(observation("feed-source-2", "obs-1", 1, NOW), &other_sk);
    let registry = || {
        let mut r = FeedRegistry::new();
        r.register(feed("feed-source-2"), other_vk.clone());
        r
    };
    let guard = Arc::new(ReplayGuard::new());
    let gateway = |registry: FeedRegistry| {
        FeedGateway::new(registry, provider())
            .with_clock(|| NOW)
            .with_freshness(WINDOW)
            .with_replay_guard(guard.clone())
    };

    let (result, admitted) = submit_over_pqmtls(gateway(registry()), signed.clone());
    match result {
        Err(FeedTransportError::Rejected(FeedError::NotFromPeer { feed: f, peer })) => {
            assert_eq!(f, feed("feed-source-2"));
            assert_eq!(peer, ServiceId("feed-source-1".into()));
        }
        other => panic!("expected NotFromPeer, got {other:?}"),
    }
    assert_eq!(admitted, 0);
    // The binding refusal came before the signature and replay checks.
    assert_eq!(guard.last_sequence(&feed("feed-source-2")), 0);

    let mut delegated = registry();
    delegated.delegate(ServiceId("feed-source-1".into()), feed("feed-source-2"));
    let (result, admitted) = submit_over_pqmtls(gateway(delegated), signed);
    assert_eq!(result.expect("delegated feed admitted"), 1);
    assert_eq!(admitted, 1);
}
