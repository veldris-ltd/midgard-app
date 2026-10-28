# Midgard console and gateway: bug fix plan

Source: the 19 notes Phill Snelling filed in Brokkr (project.veldris.dev, Notes) on
2026-10-14. Each one was rechecked against `origin/dev` at `cc3d097`, merged on
2026-10-28. The bug numbers follow the order the notes were created, which matches
the numbers the notes use to refer to each other ("see Bug 08", and so on).

## Status against current dev

| #  | Note                                              | Severity | Still on dev? | Where |
|----|---------------------------------------------------|----------|---------------|-------|
| 01 | Object types table shrinks and disappears over time | High   | Yes | `ObjectFabricScreen.tsx:162`, `ConsoleShell.tsx:214` |
| 02 | Object types table hidden at laptop size          | High     | Yes. The `dev_mac` fix was never committed, and `dev_mac` is now behind dev | same |
| 03 | Policies and Keys tables squeezed                 | Medium   | Yes | `PoliciesScreen.tsx:155`, `KeysScreen.tsx:117` |
| 04 | Object types tabs do nothing                      | Low      | Yes | `ObjectFabricScreen.tsx:30,157` |
| 05 | Audit category filters always empty               | Medium   | Yes | `AuditScreen.tsx:33-40` |
| 06 | Search-around shows old results after a failed search | Medium | Yes | `SearchAroundScreen.tsx:39,102` |
| 07 | Actions without approval give no feedback         | Low      | Yes | `ObjectFabricScreen.tsx:46` |
| 08 | Actions change every object of the type           | High     | Yes | `ObjectFabricScreen.tsx:46,59` sends `target: <type>/*`; the gateway uses `<type>/*` by default too (`enclosure.rs:381`) |
| 09 | Approval gate accepts any token                   | High (security) | Yes | `enclosure.rs:375` checks only for a non-empty token; the console makes up `HG-…` itself (`ObjectFabricScreen.tsx:60`) |
| 10 | Policy dry-run uses the wrong user                | Medium   | Partly fixed. The gateway now takes roles and clearance from the token (`enclosure.rs:653`), but the console still shows the hard-coded `j.benjamin` context (`sdk/ts/src/http/HttpGateway.ts:44`, `PoliciesScreen.tsx:200`) | |
| 11 | Buttons and search boxes that do nothing          | Low, widespread | Yes | all 10 screens, plus the shell header |
| 12 | Made-up figures shown as real data                | Medium   | Yes | `ObjectFabricScreen.tsx:150-157`, `SearchAroundScreen.tsx:63-65,107` |
| 13 | Expired login shows empty pages instead of sign-in | Medium  | Yes | `AuthGate.tsx:39,56`, `DataProvider.tsx:53` |
| 14 | No limit on failed logins                         | Low in demo mode, higher in a real enclosure | Yes | `lib.rs:247` `handle_login` |
| 15 | Oversized request returns 'not found'             | Low (also a memory DoS) | Yes | `http.rs:91`: the body buffer is sized from `Content-Length`, with no upper limit |
| 16 | Missing browser-tab icon                          | Trivial  | Yes | `products/midgard/ui/index.html` has no icon, and there is no `public/` folder |
| 17 | Build fails because a cargo config file is missing | High    | **Fixed.** `.cargo/config.toml` is on dev and main | needs only a check, then close |
| 18 | Search-around silently cuts results short         | Medium   | Yes | `enclosure.rs:47-51,462,541` |
| 19 | No way to search data in the console              | Medium (feature) | Yes | there is no query UI; filters in `/api/v1/query` are exact-match only |

18 of the 19 still need work. The plan groups them into six work packages,
ordered by risk. Every package is a separate PR into `dev`.

---

## WP1: Actions and approvals (security and data integrity): Bugs 09, 08, 07

These go first because they let anyone with a login change live data without
approval, and change every object of a type at once.

> **Status (2026-10-28): implemented, not yet compiled.** What changed from the plan
> below:
> - The gate token is the approval's id (`APR-n`). It is checked against the
>   approval kept on the server, not signed. This is as safe as a signed token: the
>   approval is bound to the requester, the action and the object, is used once,
>   expires after 15 minutes, and only a second person holding the policy's
>   `Approve` action can grant it.
> - Approvals are kept in memory and lost on restart.
> - The dev gateway adds an `approver` / `midgard-approve` login. mg-admin's starter
>   policy and the shipped policies (samples, compose, installer profiles, guide
>   example) gain `approver-read` and `approver-approve` rules.
> - The gateway marks each approval with `mine` (the caller asked for it), so the
>   console doesn't need the WP3 identity fix to know which approvals it can run
>   and which it can decide.
> - Mock mode has no second person, so a simulated approver approves a request
>   after 3 s.

**Bug 09: real approval tokens**
1. Gateway: add a list of pending approvals to `Enclosure`.
   - `POST /api/v1/approvals` (requester): `{actionType, target}` → `{approvalId}`.
   - `POST /api/v1/approvals/{id}/approve` (approver): needs the approver role, and
     the approver must not be the requester (two-person rule). It returns a
     single-use token bound to `(actionType, target, requester, expiry)`, signed
     with the gateway's existing PQ signing key.
   - `apply_action` checks the signature, that the token matches this action and
     target, that the caller is the requester, that it has not expired, and that it
     has not been used. It then marks the token as spent. Each failure returns
     403 and writes an audit entry.
2. Console: replace the made-up `HG-${Date.now()}` value with a "Request approval"
   step, plus a pending-approvals list where an approver can approve. The
   requester then runs the action using the token the gateway issued.
3. Tests: the gateway rejects `"x"` and `" "`, a token for another action or target,
   a reused token, an expired token, and an approval by the requester.
4. **Open decision:** whether the demo dataset should provision a second login
   with the approver role, so the demo can still show the flow.

**Bug 08: act on one object**
1. Gateway: refuse `apply_action` when no `target` is given, or when the target is
   a wildcard (`<type>/*`), unless the action is explicitly marked as a bulk
   action. Return 400 with a clear message. Remove the default at
   `enclosure.rs:381`.
2. Console: in the Object types side panel, add an instance picker. It loads
   instances through `POST /api/v1/query {objectType}`, and the action buttons
   stay disabled until one instance is picked. Send `target: <type>/<id>`.
3. Test: grounding AC-2214 leaves the other three aircraft unchanged.

**Bug 07: feedback when an action runs**
1. Console: show a toast or inline banner after `applyAction`: "Event closed ·
   AUD-123" on success, or the gateway's error text on failure. The Approve flow
   above uses the same component.

## WP2: Gateway hardening: Bugs 15, 14

> **Status (2026-10-28): implemented.** The body limit is 1 MiB, and the head is > also capped at 16 KiB (431). A chunked body gets 411 rather than being read as > empty, which is the likely cause of the 404 in the note. Refusals are drained > for up to 2 s, so the client sees the status rather than a reset. Logins are > throttled per username only, because the edge sees no client address: after 5 > failures the name is locked for 30 s, doubling to a 15-minute cap. Up to 10,000 > names are tracked, in memory.

**Bug 15: request body limit**
1. `http.rs`: add `MAX_BODY_BYTES` (for example 1 MiB; it may need to be larger
   for any import route). If `Content-Length` is above the limit, respond with
   `413 Payload Too Large` straight away, without reading or allocating the body.
   Add `413` to the status-text match at `http.rs:155`.
2. Test: a 5 MB body gets 413 in under 100 ms, and the gateway stays healthy.

**Bug 14: limit failed logins**
1. `lib.rs::handle_login`: track failures per username, and per source address
   where the transport provides one. Use exponential backoff (for example after
   5 failures), then a temporary lockout (for example 15 minutes). Return 429 with
   `Retry-After`. Audit `LOGIN throttled` and `LOGIN locked`.
2. Keep the counters in memory for a single gateway. Record in the design notes
   that a clustered enclosure needs shared state.
3. Tests: the 6th fast wrong password is throttled; the correct password during
   lockout is refused; the counter resets after a success or after the window.

## WP3: Session and identity in the console: Bugs 13, 10, and sign-out from 11

> **Status (2026-10-28): implemented.** The gateway gained `GET /api/v1/whoami` > rather than more fields on the login response, so a token restored after a > reload is checked and identified too. `AuthGate` checks a stored token before > rendering and ends the session on any 401, at the token's expiry, or on > **Account → Sign out**. The console's policy context, which the dry-run shows, > is the signed-in user's; `j.benjamin` now exists only in the mock.

**Bug 13: expired login**
1. `DataProvider.tsx`: when any call returns 401, clear the stored token, stop
   polling the audit log, and raise an "unauthenticated" event.
2. `AuthGate.tsx`: listen for that event and show the sign-in card with the
   message "Your session expired. Sign in again." Also check `expiresAt` from the
   login response at start-up and when a timer fires.
3. Header Account menu: add **Sign out**, which clears the token (and fixes the
   Account part of Bug 11).

**Bug 10: dry-run as the signed-in user**
1. Gateway: return `roles` and `clearance` in the login response next to `subject`
   (or add `GET /api/v1/whoami`).
2. SDK: remove the `j.benjamin` `DEFAULT_POLICY` from `HttpGateway.ts`. Build the
   context from the login response, held by `AuthGate` and passed in through
   `HttpGatewayOptions.policy`. Keep `j.benjamin` only in the mock gateway.
3. `PoliciesScreen.tsx`: show the real subject, clearance and roles. The gateway
   already ignores roles sent in the request body, so only the display needs to
   change.

## WP4: Layout: Bugs 01, 02, 03

1. Use one shared fix instead of patching each screen: add a `ScrollTable` (or
   `TableWrap`) component to `ui/shared` with `flexShrink: 0` and
   `overflowX: 'auto'`, and use it for every table wrapper in a console screen
   (Object types, Audit, Policies, Keys, and any other screen that has one).
   This takes over the `dev_mac` fix that was never committed.
2. Separately, fix the audit log feeding itself (Bug 01's second cause): every
   poll writes a `PERMIT Read /api/v1/audit` row, so the list grows on its own.
   Either leave audit reads by the console's own poller out of what
   `useAuditStream` shows, or poll for "since seq N" so that reads which find
   nothing new don't add rows. Decide whether the gateway should keep auditing
   them; the security posture says reads are audited, so probably keep them
   audited and filter them on the console side.
3. Check in headless Chrome at 1024×768, 1440×900, 1508×746 (watched for 60 s)
   and 390×844, on Object types, Policies and Keys. If the repo's Playwright
   setup supports it, add these as UI tests.

## WP5: Truthful UI: Bugs 12, 04, 05, 06, 11, 16

**Bug 12: made-up figures**
- Object types header: work out the type count and the sum of `instanceCount`
  from `getObjectTypes()`, and use the same values in the nav badge and the
  "All types" tab badge. Remove the `Change requests 4` badge, or tie it to real
  data.
- Search-around side panel: fetch the selected node's real properties with
  `POST /api/v1/query`, and show the gateway's redaction markers. Base the
  `REDACTIONS` count on what was actually redacted.
- Identity, Keys, Compute, Observability and Resilience: where the gateway serves
  fixed sample data, show a clear "Sample data" tag in the page header. Add a
  field to those endpoints' responses so the console can tell sample data apart.

**Bug 04: Object types tabs**: filter by `tab`, or show an empty state per tab
("No interfaces defined"). Hide Change requests until there is a backend for it.

**Bug 05: Audit category filters**: base the categories on the gateway's real
action format (`PERMIT <verb>`, `DENY …`, `LOGIN …`). Better still, have the
gateway send a structured `kind` field (`action | query | write | login | admin`)
in each audit record and filter on that, so the console stops matching text
prefixes. Update the mock fixtures to the same format.

**Bug 06: stale Search-around results**
- Keep a separate `submittedSeed` that changes only when Run is pressed, and
  use it in the heading.
- When the call fails (`error` from `useAsync`), clear the graph and side panel
  and show the gateway's message ("No object AC-9999"). Also check that
  `useAsync` resets `data` to null when the key changes.

**Bug 11: controls that do nothing**: for each control in the note, either
wire it up or show it disabled with a "Not available yet" tooltip. Scope:
- Wire up: Account → Sign out (WP3); top search box → the new search page (WP6);
  Explore instances → the instance picker or search, filtered by type; Export
  chain → download the filtered audit records as JSON.
- Disable and label: Notifications, New object type, Edit schema, New policy,
  New identity, Edit grants, Revoke session, Rotate now, Schedule job,
  Download SDKs. Each has a backend that doesn't exist yet.
- Add a check (lint rule or test) that flags a `<Button>` with no `onClick`
  that isn't `disabled`.

**Bug 16: favicon**: add `products/midgard/ui/public/favicon.svg` and a
`<link rel="icon">` in `index.html`. Do the same in the other product UIs if
they also lack one.

## WP6: Search: Bugs 18, 19

**Bug 18: Search-around truncation**
1. Gateway: add `truncated: bool`, `totalRoots` and `nodeLimit` to the
   search-around response, and set them when `MAX_ROOTS` or `MAX_NODES` is hit.
   Add an optional `limit` the caller can send, up to a raised maximum (for
   example 500).
2. Console: show "Showing 60 of N — narrow the seed or raise the limit"
   whenever `truncated` is set. Make the Hops dropdown go up to the gateway's
   `MAX_HOPS` (4).

**Bug 19: data search page** (feature, last in order)
1. Gateway `/api/v1/query`: add range and comparison operators
   (`{field: {gt|lt|gte|lte|between|ne|in}}`), `orderBy`, and cursor paging
   beyond the 500 limit. Policy checks and redaction stay unchanged.
2. Console: a new **Search** screen with a type picker, filter rows (field,
   operator, value), a results table showing every property, paging, and an
   "Open in Search-around" link per row. The header search box opens it.
3. Acceptance: "large airports" returns 17 in the UK airports enclosure;
   "lit runways over 5,000 ft" works.

## WP0: Housekeeping

- Bug 17: check that a fresh clone of `main` builds offline
  (`cargo build --offline --release -p http-gateway`), then close the note.
- Bug 02: close or rebase the `dev_mac` branch once WP4 lands, because its
  uncommitted fix is replaced.
- After each WP is merged, update its Brokkr notes with the commit and how the
  fix was verified, the same way Phill recorded the Bug 17 update.

## Order and sizing

| Order | WP | Bugs | Rough size |
|-------|----|------|------------|
| 1 | WP1 Actions and approvals | 09, 08, 07 | L (gateway + console + tests) |
| 2 | WP2 Gateway hardening | 15, 14 | S–M |
| 3 | WP3 Session and identity | 13, 10, sign-out | M |
| 4 | WP4 Layout | 01, 02, 03 | S |
| 5 | WP5 Truthful UI | 12, 04, 05, 06, 11, 16 | M |
| 6 | WP6 Search | 18, 19 | M (18) + L (19) |
| any | WP0 Housekeeping | 17, 02 | XS |

WP4 is small and very visible, so it can be done alongside WP1 if speed matters.
WP2, WP3 and WP4 touch separate files and can run in parallel. WP5 depends on WP3
(sign-out) and WP6 (the target for the search box) only for those two controls.

## Decisions needed

1. Approval model (WP1): two-person rule with a demo approver account, or
   self-approval with a second factor?
2. Bulk actions (WP1): should any action legitimately apply to `<type>/*`, and if
   so, does it need its own approval?
3. Login limits (WP2): thresholds and lockout length for a real enclosure.
4. Sample-data screens (WP5): label them, or hide them in a live enclosure?
