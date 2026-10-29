/* The guided tour, as data. Each step opens one console at one route; `target`
   is visible text in that screen to ring (matched case-insensitively, exact
   match preferred, else the smallest element containing it). `play/scripts/check.mjs`
   fails the build check if a route is not a console route or a target is not
   on screen. Keep the copy true to what the sample-data gateways show.
   [ADR-PLAY-002] [ADR-PLAY-003] */

import type { ConsoleId } from './consoles';

export interface TourStep {
  console: ConsoleId;
  route: string;
  title: string;
  body: string[];
  /** Something for the visitor to do on this screen. */
  try?: string;
  /** Visible text to highlight. */
  target?: string;
}

export const TOUR: TourStep[] = [
  {
    console: 'midgard',
    route: '/',
    title: 'Welcome to the Midgard playground',
    body: [
      'What you are using is the console code in the public midgard-app repository, built for your browser. The data is sample data, seeded in each console, and nothing you do here leaves this page.',
      'This tour takes about ten minutes across all seven consoles. Use Next, or the arrow keys, to move on; every console stays fully clickable while you read.',
    ],
  },
  {
    console: 'midgard',
    route: '/',
    title: 'The object fabric',
    body: [
      'Midgard holds data as typed objects, not loose files. Each object type lists its properties, its instances and its marking: confidential, restricted or internal, which decides who may read it.',
      'Every object carries its lineage, and the figure at the top says how much of it is signed.',
    ],
    target: 'Lineage signed',
  },
  {
    console: 'midgard',
    route: '/',
    title: 'Actions that wait for a person',
    body: [
      'Actions that change the world, such as grounding an aircraft, are declared on the object type and marked with a human gate. An AI agent can propose them; only people can approve them.',
    ],
    try: 'Choose an aircraft under Act on, then press Ground aircraft. The request is recorded and waits for a second person.',
    target: 'Ground aircraft',
  },
  {
    console: 'midgard',
    route: '/policies',
    title: 'Policy, declared and dry-run',
    body: [
      'Access is decided by declarative policies over role, marking and purpose, and the default is deny. The dry-run evaluates a request before anyone relies on it, and the decision itself is signed and audited.',
    ],
    try: 'Pick a purpose and a marking, then press Evaluate to see which rule grants or denies it.',
    target: 'Evaluate',
  },
  {
    console: 'midgard',
    route: '/audit',
    title: 'One signed audit chain',
    body: [
      'Queries, writes, actions and agent activity land in one append-only chain. Each record is signed with ML-DSA, the post-quantum signature standard, and links to what caused it and what it led to.',
    ],
    try: 'Select any record to see its lineage trace.',
    target: 'records verified',
  },
  {
    console: 'forge',
    route: '/',
    title: 'Forge: from source to object',
    body: [
      'Forge builds pipelines from sources through transforms to versioned datasets, then projects them into the fabric as objects. Every version is sealed with AES-256-GCM under ML-KEM, and its lineage is signed.',
    ],
    try: 'Press Run build and watch the log resolve, transform, seal and project.',
    target: 'Run build',
  },
  {
    console: 'forge',
    route: '/health',
    title: 'Expectations that stop bad data',
    body: [
      'Expectations describe what good data looks like: bounds, freshness, integrity, volume. A blocking expectation that fails holds the projection back, so the fabric keeps the last good version instead of the bad one.',
    ],
    target: 'Projections blocked',
  },
  {
    console: 'mimir',
    route: '/',
    title: 'Mimir: agents grounded in the ontology',
    body: [
      'Mimir answers from the objects in the fabric, cites them, and runs on local models: nothing is sent to a cloud model. When an answer implies a change, it becomes a proposed action behind a human gate.',
    ],
    try: 'Read the dispatch-01 session, then authorise or reject the proposed maintenance.',
    target: 'Human gate required',
  },
  {
    console: 'mimir',
    route: '/guardrails',
    title: 'Guardrails you cannot switch off',
    body: [
      'Every agent runs through rails on its input, its plan, its tool calls and its output. The core rails enforce and cannot be disabled, and every trip is an audited incident.',
    ],
    try: 'Choose injection and press Run through rails to see an attack refused before any tool runs.',
    target: 'Run through rails',
  },
  {
    console: 'heimdall',
    route: '/',
    title: 'Heimdall: the operating picture',
    body: [
      'Heimdall fuses feeds into one picture. For an anomalous track, AI ranks the options with the evidence for each; the ranking is advisory, and a person authorises what happens.',
    ],
    try: 'Press Authorise on the ranked option and note the signed decision it produces.',
    target: 'Authorise',
  },
  {
    console: 'heimdall',
    route: '/lineage',
    title: 'Every decision can be replayed',
    body: [
      'Each decision is stored with the picture as it was, the options shown and who chose. Review and after-action read the same signed record, not a reconstruction.',
    ],
    target: 'Replay picture at decision time',
  },
  {
    console: 'bifrost',
    route: '/',
    title: 'Bifrost: verified, ringed delivery',
    body: [
      'Releases roll out in rings, canary first, each behind a health gate, and air-gapped sites receive signed offline bundles. Before promotion, Bifrost verifies the build is reproducible, has its SBOM and carries a valid signature.',
    ],
    target: 'promotion permitted',
  },
  {
    console: 'bifrost',
    route: '/rollback',
    title: 'Rollback, always armed',
    body: [
      'Rollback plans stand ready and are drilled. Execution is health-gated, reverses the ring order and is signed like any other decision.',
    ],
    try: 'Press Execute rollback.',
    target: 'Execute rollback',
  },
  {
    console: 'admin',
    route: '/approvals',
    title: 'Admin: dual control, no super-admin',
    body: [
      'High-risk actions, such as key rotation, privilege changes, model revocation and fleet releases, need a written justification and a second approver. Approving your own request is blocked.',
    ],
    target: 'Self-approval blocked',
  },
  {
    console: 'admin',
    route: '/breakglass',
    title: 'Break-glass that reviews itself',
    body: [
      'Emergency access is time-boxed, scoped to the emergency, alerts every security officer and forces a post-hoc review, which is itself audited.',
    ],
    target: 'Post-hoc review required',
  },
  {
    console: 'clients',
    route: '/',
    title: 'Clients: least capability',
    body: [
      'The web app keeps a sealed, policy-scoped cache of the fabric. Its browser extension holds no keys, reads only what you select, and sends privileged work to the enclosure.',
    ],
    try: 'Press Match against the fabric.',
    target: 'Match against the fabric',
  },
  {
    console: 'clients',
    route: '/mobile',
    title: 'Offline first',
    body: [
      'On mobile, edits go to a sealed local store and queue as conflict-free operations. Nothing leaves the device until it syncs.',
    ],
    target: 'Nothing leaves the device until sync',
  },
  {
    console: 'midgard',
    route: '/',
    title: 'Now run it yourself',
    body: [
      'Everything you have seen is open source under Apache-2.0. To run this playground offline on your own machine: pnpm install, then pnpm play.',
      'To see the Midgard console talking to the real Rust edge, with a post-quantum signed sign-in and a real audit ledger: just try. Development mode is for your own machine only.',
    ],
  },
];
