window.KIT = {
 "slides": 18,
 "titles": [
  "Midgard",
  "Agenda",
  "The problem",
  "The suite",
  "Design principles",
  "Architecture",
  "Object fabric",
  "Governance",
  "Forge",
  "Mimir",
  "Heimdall",
  "Bifrost",
  "Admin and clients",
  "Live demo",
  "Post-quantum by default",
  "Proof",
  "Where we are",
  "Next steps"
 ],
 "modes": {
  "dark": {
   "deck": "Midgard-Launch.pptx",
   "deckBytes": 24073810,
   "video": "midgard-launch-demo.mp4",
   "videoBytes": 21224045,
   "videoSeconds": 249
  },
  "light": {
   "deck": "Midgard-Launch_light.pptx",
   "deckBytes": 20374201,
   "video": "midgard-launch-demo-light.mp4",
   "videoBytes": 17529311,
   "videoSeconds": 190
  }
 },
 "notes": [
  "Welcome. Midgard is a Veldris Ltd product: a sovereign, open-source, post-quantum data and decision platform, built and verified against a complete specification. Over the next twenty minutes I will show you what it is, watch it run end to end, and be precise about what is proven today and what still stands between this release candidate and general availability. (≈1 min)",
  "Five parts. The problem we are solving, the platform itself, a recorded live demo of the software running, the security and verification evidence, and an honest statement of where the release stands. Questions are welcome at the end. (≈0.5 min)",
  "Three pressures. First, the tooling organisations use for analytics, AI and software delivery assumes an always-on cloud; for regulated, defence and critical-infrastructure work that is disqualifying. Second, harvest-now-decrypt-later: traffic and archives captured today become readable once quantum computers mature, and NIST has already finalised the replacement algorithms, so migration is now a programme, not a research topic. Third, the incumbent platform is closed, hosted and opaque. You cannot audit what it does with your data and you cannot leave easily. Midgard was designed to answer all three at once. (≈2 min)",
  "Midgard is the core platform: the enclosure. Identity, the object and ontology fabric, storage, compute, governance and post-quantum cryptography by default. It is the foundation every product builds on. On top sit four independently useful products, each with its own domain: Forge for data operations, Mimir for local agentic AI, Bifrost for continuous delivery, and Heimdall for operational fusion. Two cross-cutting layers complete it: a self-governing admin control panel with no super-admin, and native mobile, desktop and web clients. (≈1.5 min)",
  "Six principles, and each one is enforced by construction rather than by policy document. Sovereign: no core function requires an outbound call, and the acceptance suite proves it under docker network none. Post-quantum by default: hybrid ML-KEM in transit, ML-KEM-wrapped keys at rest, ML-DSA signatures everywhere. The object fabric is the centre, not a bolt-on. Every product calls Mimir for AI, never a hosted model. Every claim has a source and every action has lineage. And the core is open: Apache-2.0, reproducible builds, a CycloneDX SBOM with every release. (≈1 min)",
  "The shape of a deployment. Operators reach the enclosure through one edge, the HTTP gateway, which terminates TLS and issues ML-DSA-signed session tokens. Behind it the core composes identity, the object fabric, key management and the governance ledger in one process, so edge admission and every fabric call land in a single PQ-signed ledger. The four product services talk to the core and to each other only over PQ-mTLS, each with a certified mesh identity. The only things that cross the boundary are deliberate: Forge reaches external sources through an egress broker with a pinned CA per route, and Bifrost carries releases into air-gapped sites as signed bundles that verify themselves offline. (≈1.5 min)",
  "This is the Midgard console against the real edge. The left panel is the object-type registry; the inspector on the right shows an object type, its links and the actions a person may take. Ground aircraft is human-gated: the first attempt is refused with a human-gate-required error, an operator authorises it, and the authorisation comes back with the audit id that recorded it. The same flow works from the API with curl. (≈1 min)",
  "Governance is the spine, not a report. Policy is deny by default: role, marking and purpose must all match a written rule, mandatory access control runs first, and anything not granted is denied. You can dry-run a decision for a subject, clearance and purpose before it goes live, and the decision itself is ML-DSA-signed. Every read, write, action and AI step lands in one append-only ledger, and the service re-verifies that ledger on every start: a tampered ledger, keystore or policy file stops it from starting. (≈1 min)",
  "Forge is the data-operations product. Draw a pipeline or write it in code; either way every build is repeatable and versioned like source control. The worker service already runs: it reads a source, commits rows to a sealed dataset store, projects them onto the ontology and syncs them into the fabric through the gateway's PQ-mTLS door. Connectors have been tested against real PostgreSQL 16 and MinIO. (≈1 min)",
  "Mimir is the local AI product. The inference plane is a running service: a certified caller opens a PQ-mTLS channel, policy decides whether it may infer, and every call, permitted or denied, is written to a sealed ledger the service re-verifies on restart. We have run a real model end to end: Granite 3.3 2B on CPU, staged from a signed manifest. (≈1 min)",
  "Heimdall is the operational-fusion product. The feed gateway runs as a service today: a source with a certified identity signs its observations, four checks gate each one, refusals are audited too, and the fused picture survives a restart. The console view here is the product's own synthetic reference layout. (≈0.75 min)",
  "Bifrost is the continuous-delivery product, and it is also how Midgard itself ships. The release pipeline has been rehearsed end to end: all eighteen binaries gated, six images built and started through their own verifying entrypoints, a fifty-four-artefact bundle packed and installed on a cluster, and an image with one changed byte refusing to start. (≈0.75 min)",
  "Two cross-cutting layers. The admin control panel governs itself: role-separated RBAC with no super-admin, dual control on every high-risk action, a self-approval attempt refused and recorded, time-boxed break-glass with real alerts, and one tamper-evident ledger over all seven subsystems. And the client platforms: one shared core behind native mobile, desktop and web shells, keys held in device hardware, offline-first with conflict-free sync when the connection returns. (≈0.75 min)",
  "Play the video. It is about four minutes and it was recorded against the software as it is: the Midgard console is talking to the real Rust edge running in a container, and the other consoles run on their built-in sample data. Watch for three things: the token that comes back from login is post-quantum signed; the grounding action is refused until a person authorises it, and the authorisation is audited; and every ledger entry shows as re-verified. (≈5 min)",
  "The cryptography, layer by layer. In transit, every hop is a hybrid handshake: classical X25519 alongside ML-KEM-768, so an attacker has to break both. At rest, data keys are AES-256-GCM, wrapped under ML-KEM-1024 custody roots. Service identities, session tokens and audit entries are signed with ML-DSA-65; release signing uses ML-DSA-87; SLH-DSA is available for long-lived artefacts. The FIPS backend passes the official known-answer tests in the repository, and the two backends are mutually exclusive at compile time so a build cannot quietly mix them. What is not yet done is FIPS 140-3 module certification itself, which is external. (≈1.5 min)",
  "These are measurements from the repository, not projections. The seventy-two acceptance criteria pass end to end inside the pinned toolchain container with networking disabled. The full test sweep runs both crypto backends. Every one of the eighteen binaries the workspace builds passes the per-artefact gate, and a missing one fails the build. The release pipeline produced a fifty-four-artefact signed bundle that installed on a three-node cluster, and the deployment tier passed its restart and cross-node handshake checks there. The GPU and HSM checks skipped because we have no such hardware yet, and the record says so. (≈1.5 min)",
  "Where we are, honestly. The implementation is complete and verified, and version 0.1.0-rc.1 is prepared: the edge on the real fabric, five services on one PQ-mTLS runtime, compose and Helm deployment, connectors, real model serving, and a release pipeline rehearsed end to end on the fifteenth of September. What blocks general availability is external or hardware-bound: HSM-backed key custody, GPU serving, FIPS 140-3 certification, and the key ceremony and signing runner that let us tag the release. The authoritative go/no-go record is GA_READINESS.md and it currently reads no-go, naming every blocker. The next ninety days are about closing those and putting live data through Forge into the fabric. (≈2 min)",
  "To close. Midgard runs on a laptop with one command, on docker compose with five services, on Kubernetes through a hardened Helm chart, and in air-gapped sites from a signed bundle. We are looking for three things: design partners willing to run a pilot on the compose or Helm path, security teams to review the open core, and feedback on the roadmap you just saw. The repository, the specification and the verification record are all public. Thank you. Questions. (≈1 min)"
 ]
};
