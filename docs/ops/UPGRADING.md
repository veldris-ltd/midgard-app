# Upgrading to the security release

How to move an enclosure created by an earlier build (`0.1.0-rc.1`, or `main` before the
`security-patch_oct` fixes MG-SEC-01 to MG-SEC-15) onto this one without losing its keys,
its audit history or its sealed state. Every step is safe to run again: on something
already current it changes nothing.

The short version, for each way of running Midgard:

| You run it with | Do this |
|---|---|
| `mg-install` on a server | Update the checkout, run `sudo deploy/installer/mg-install` and choose **Check and repair it**. It runs every step below. |
| docker compose (`just compose-up`) | `just compose-down`, update the checkout, `just compose-up`. The `init` service runs every step below before anything starts. |
| kind (`deploy/kind/up.sh`) | A local cluster is not upgraded in place: delete the namespace and run `deploy/kind/up.sh` again. |
| The Helm chart on a real cluster | Choose `secrets.provider` (`csi` or `externalSecrets`, or `kubernetes` with etcd encrypted at rest and `secrets.acknowledgeEtcdEncryption: true`; the render fails otherwise) and add a `<name>-keystore-passphrase` for every workload: [deploy/helm/midgard/README.md](../../deploy/helm/midgard/README.md). Keystores are migrated as in [Keystores](#keystores-wrap-the-custody-root) before they are loaded into the store. [VLD-ANB-MIDGARD-0018] |
| The binaries by hand | Follow the steps below. |

## What changes, and why it needs a step

| Change | Bug | What it means for an existing enclosure |
|---|---|---|
| The mesh protocol is version 2 (X-Wing key combination, hybrid key exchange required, an end-of-stream marker). | MG-SEC-09, MG-SEC-12 | A service on this release refuses one on the earlier release. **Upgrade every service together**; there is no mixed-version window. |
| Certificates carry a serial, a validity window and a clearance; trust anchors have a new format. | MG-SEC-02, MG-SEC-04 | Certificates (`*.cert`) and anchors (`mesh.anchor`) written by the earlier release are refused. → [Step 4](#4--bring-every-mesh-identity-up-to-date). |
| Revocation is a signed list, `mesh.crl`, beside every identity. | MG-SEC-03 | A service without `mesh.crl` does not start. → [Step 4](#4--bring-every-mesh-identity-up-to-date). |
| Every audit ledger keeps a signed checkpoint. | MG-SEC-05 | A ledger written before checkpoints does not open. → [Step 3](#3--adopt-the-audit-ledgers). |
| The gateway does not start without `--state-dir`; development mode needs `--dev` and listens on loopback only. | MG-SEC-06 | Change any command line, unit file or script that starts `http-gateway` with no options or with a bare address. → [Step 5](#5--change-the-gateways-command-line). |
| Passwords are stored with Argon2id; stored hashes outside fixed cost bounds are refused. | MG-SEC-11 | Nothing to do. Existing PBKDF2 passwords (210,000 iterations) still sign in, and each is replaced with Argon2id at its owner's next sign-in. |
| Break-glass needs two people holding the right roles. | MG-SEC-07 | The requester needs `breakglass.request` and the approver `breakglass.approve` (the Security-Officer role), each signed in with their own passkey. Check your role bindings before you need it. |
| Model manifests are format 2, signed over their own bytes; a model sidecar loads a staged copy of the verified weights. | MG-SEC-14 | A format-1 manifest is refused, and `mimir-server` with `--engine llama-cpp` or `vllm` needs `--serve-dir`. → [Step 6](#6--re-sign-models-and-give-the-sidecar-a-serve-directory). |
| The image no longer reads `MIDGARD_RELEASE_TRUST_KEY`. | MG-SEC-14 | Verification always uses `/trust/release-trust.pub`. Remove the variable from your environment; it has no effect. |
| Keystores are format version 3: the custody root is wrapped under a passphrase, bound to the whole header, and every program that opens one needs `--keystore-passphrase-file` or `MIDGARD_KEYSTORE_PASSPHRASE_FILE`. | VLD-ANB-MIDGARD-0007 | A version 1 keystore (root in the clear) and a version 2 keystore (header not authenticated) are refused everywhere. → [Keystores](#keystores-wrap-the-custody-root). |
| Audit ledger entries are schema 2; identity directory records are version 3, then 4. | VLD-ANB-MIDGARD-0054, VLD-ANB-MIDGARD-0026, VLD-ANB-MIDGARD-0084 | One-way: once the new build has written to them, an earlier build cannot open the ledger or the directory. Rolling back means restoring the backup. → [Step 1](#1--back-up) and [Rolling back](#rolling-back). |
| Administrators prove each request with a passkey **and** a one-time code the gateway checks. | VLD-ANB-MIDGARD-0008 | An administrator with no TOTP secret gets 401 on every administration route, `enrol` included, until one is set. → [Step 5a](#5a--give-every-administrator-a-totp-secret). |
| Session tokens are `midgard-token-v2`. | VLD-ANB-MIDGARD-0021 | A token minted before the upgrade no longer verifies: **everyone signs in again**. Nothing to run; tell your users. |
| Heimdall observations are format version 2 (a sequence inside the signed bytes). | VLD-ANB-MIDGARD-0041 | A version 1 observation is refused. **Rebuild every feed source** and deploy it with `heimdall-feed`. → [Step 6a](#6a--rebuild-every-heimdall-feed-source). |

## 1 — Back up

Take the backup in [BACKUP_RESTORE.md](BACKUP_RESTORE.md) of the gateway's state
directory, every service's state directory, and every identity directory. The steps below
write new certificates and checkpoints and rewrite each keystore once, as version 3 (the
keys inside are kept), but a backup is how you go back. The new build also rewrites the
audit ledgers (entry schema 2) and the identity directory (record versions 3 and 4) the
first time it writes to them, and no earlier build opens either afterwards: the backup is
the **only** way back ([Rolling back](#rolling-back)). Keep the pre-migration keystores in
the secret store, not on the host, until the release has soaked, then destroy them:
version 1 files hold the custody root in the clear.

## 2 — Stop everything, then install the new build

Stop the gateway and every mesh service (`mimir-server`, `bifrost-fleet`, `heimdall-feed`,
`forge-worker`, and any agents, sources or probes). The steps below take the state
directory's lock, which a running gateway holds, and a service must not be appending to a
ledger while it is adopted. Then install the new binaries or images.

### Keystores: wrap the custody root

Before anything opens a keystore, give each keystore holder a passphrase and migrate its
keystores. Compose `init` and `mg-install`'s repair do this themselves: each holder gets a
random passphrase in a volume of its own (`<holder>-passphrase`), and every version 1 or
version 2 keystore is migrated to version 3 under it (a version 2 keystore under the
passphrase it already has). By hand, migrate the state directory's audit keystore
**first, and without `--state-dir`**:

```bash
mg-admin keystore migrate --keystore STATE/keys/audit.keystore --keystore-passphrase-file /secrets/gateway.passphrase
```

Given `--state-dir`, `keystore migrate` records the migration in the gateway's ledger,
which it opens with `STATE/keys/audit.keystore`; that only works once the audit keystore
is version 3 and the ledger has been adopted in [step 3](#3--adopt-the-audit-ledgers). So
migrate every keystore here without `--state-dir`, and after step 3 record each one with
`mg-admin keystore verify --state-dir STATE --keystore FILE --keystore-passphrase-file FILE`.
Then, for each holder (the state directory's `keys/` and the gateway's mesh identity
share one; each other service directory has its own):

```bash
umask 077; head -c 32 /dev/urandom | od -An -vtx1 | tr -d ' \n' > /secrets/mimir.passphrase
mg-admin keystore migrate --keystore DIR/mimir-server.keystore --keystore-passphrase-file /secrets/mimir.passphrase
mg-admin keystore migrate --keystore DIR/mimir-server.audit.keystore --keystore-passphrase-file /secrets/mimir.passphrase
mg-admin keystore verify --keystore DIR/mimir-server.keystore --keystore-passphrase-file /secrets/mimir.passphrase
```

Then pass each service `--keystore-passphrase-file FILE` (or set
`MIDGARD_KEYSTORE_PASSPHRASE_FILE`). Keep the passphrase off the keystore's volume and out
of its backups: it is the recovery secret, and a lost passphrase is a lost enclosure.
Migration keeps the same root, so destroy every other copy of each version 1 or 2 file
(backups, volumes, Secrets). An administrator's passkey (`*.passkey`) is a keystore too:
its holder migrates it the same way, under the passkey passphrase. Release and admission
keystores on the signing runner, the model-signing keystore, and Dvalin's ceremony
output, are migrated the same way ([PKI.md](PKI.md), [KEY_CEREMONY.md](KEY_CEREMONY.md)).

Where a version 1 file may have been exposed, its root is known, so after migrating it
rotate the root where that is allowed:

```bash
mg-admin keystore rotate-root --keystore FILE --keystore-passphrase-file /secrets/FILE.passphrase [--state-dir STATE]
```

This gives the keystore a new custody root and keeps its signing key and verifying key, so
trust anchors and certificates stay valid. It is allowed for keystores whose root seals
nothing else: release, admission and model-signing keystores, a mesh service's identity
keystore (`<service>.keystore`) and a passkey. It is refused for a state directory's
`keys/*.keystore` and a mesh service's `<service>.audit.keystore`, whose roots seal the
enclosure's identity directory, ledgers and stores: where one of those may have been
exposed, re-initialise that enclosure instead. [VLD-ANB-MIDGARD-0007]

## 3 — Adopt the audit ledgers

A ledger from the earlier release has entries and no checkpoint. Adopting it verifies its
whole chain and records its first signed checkpoint; from then on it opens normally. A
ledger that already has a checkpoint, or does not exist yet, is left alone.

Do this **before** step 4: upgrading an identity is recorded in the gateway's ledger, which
does not open until it is adopted. Adopting a service's ledger needs only its audit
keystore, which the earlier release already wrote.

The gateway's two ledgers (governance, and dual-control), under the pinned audit key:

```bash
mg-admin audit checkpoint --state-dir STATE --audit-pin /etc/midgard/audit.pin
```

Use a copy of `STATE/trust/audit.pub` kept where the state directory's writer cannot
reach it (see step 5). Each mesh service's own ledger, under its audit keystore, with the
service stopped:

```bash
mg-admin audit checkpoint --identity-dir /srv/mimir-identity --service mimir-server \
    --ledger /srv/mimir-state/mimir.ledger --ledger-id mimir-inference
```

| Service | Ledger (in its state directory) | `--ledger-id` |
|---|---|---|
| `mimir-server` | `mimir.ledger` | `mimir-inference` |
| `bifrost-fleet` | `fleet.ledger` | `bifrost-fleet-audit` |
| `heimdall-feed` | `feed.ledger` | `heimdall-feed-audit` |
| `forge-worker` | `worker.ledger` | `forge-worker-audit` |

Add `--audit-pin FILE` to check a service's entries under a copy of its audit key kept
elsewhere, rather than the key in its keystore. A ledger that does not verify is not
adopted, and nothing is written: restore it from the backup and investigate before going
further ([INCIDENT_RESPONSE.md](INCIDENT_RESPONSE.md)).

**Optional: a checkpoint mirror.** To make rolling the ledger and its checkpoint back
together detectable, give the gateway a directory on another mount and seed it once:

```bash
mg-admin audit checkpoint --state-dir STATE --audit-pin /etc/midgard/audit.pin \
    --checkpoint-mirror /mnt/audit-mirror
```

then start the gateway with `--checkpoint-mirror /mnt/audit-mirror` (step 5). Where to put
the mirror, and how it interacts with a restore, is in
[BACKUP_RESTORE.md](BACKUP_RESTORE.md).

## 4 — Bring every mesh identity up to date

For each identity directory, with the roles it was issued with:

```bash
mg-admin mesh upgrade --state-dir STATE --service mimir-server --roles inference-server \
    --out /srv/mimir-identity
```

`mesh upgrade` reissues the certificate for the key the service **already holds**,
rewrites `mesh.anchor`, and writes the current signed `mesh.crl`. It keeps both
keystores. That matters: a service's audit keystore signs its ledger and is the root its
sealed state is encrypted under, so a new one — which is what `mg-admin mesh issue`
makes — would leave the service unable to open either. `--days` defaults to 30. Without
`--clearance` or `--purposes`, the service keeps the clearance and purposes its
certificate already grants (a certificate in an earlier format grants none, so the
defaults of `mesh issue` apply: clearance `open`, no purposes); `--no-purposes` drops a
purpose binding. On a service whose certificate is already current, and grants exactly
the roles, clearance and purposes asked for, it changes nothing; a certificate whose
grant differs is reissued, so `--purposes` binds purposes into a certificate that had
none. [VLD-ANB-MIDGARD-0063]

The services, roles and directories the compose deployment uses (`deploy/compose/init.sh`):

| Service | Roles | Identity directory |
|---|---|---|
| `mimir-server` | `inference-server` | mimir-identity |
| `mimir-probe` | `inference-consumer` | probe-identity |
| `bifrost-fleet` | `fleet-manager` | fleet-identity |
| `fleet-agent-1`, `fleet-operator` | `fleet-agent`, `fleet-operator` | fleet-clients |
| `heimdall-feed` | `feed-gateway` | heimdall-identity |
| `feed-source-1` (and each feed), `picture-analyst` | `feed-source`, `picture-reader` | heimdall-clients |
| `http-gateway` | `fabric-gateway` | gateway-identity |
| `forge-worker` | `forge-worker` | forge-identity |

A directory shared by several identities holds one `mesh.anchor` and one `mesh.crl`; run
`mesh upgrade` for each identity in it.

## 5 — Change the gateway's command line

```bash
http-gateway --state-dir STATE --audit-pin /etc/midgard/audit.pin \
    [--checkpoint-mirror /mnt/audit-mirror] [--bind ADDR] [--allow-origin URL]... \
    [--mesh-bind ADDR --mesh-identity-dir DIR]
```

- `--state-dir` is required. `http-gateway` with no options, or with only an address,
  now refuses to start rather than serving the demonstration logins.
- `--dev [ADDR]` is the development gateway (seeded data, the published demo logins), on
  a loopback address only. Never use it for an enclosure.
- `--audit-pin` names a copy of `STATE/trust/audit.pub` kept outside the state directory
  (a read-only mount, a config map). Without it the gateway pins the copy inside the state
  directory and says so at start.

## 5a — Give every administrator a TOTP secret

Administration now takes a passkey assertion and a one-time code, and the gateway checks
the code against the administrator's secret in `STATE/admin/totp.journal`. Administrators
enrolled by an earlier build have no secret there, so every request they make, `enrol`
included, is refused with 401 until they have one. Compose `init` and `mg-install` do not
do this for you: each administrator scans the URI it prints. With the gateway stopped, for
each administrator:

```bash
mg-admin admin totp-reset --state-dir STATE --admin NAME --keystore-passphrase-file /secrets/gateway.passphrase
```

It prints the new `otpauth://` URI once; hand it to that administrator only, and have them
add it to their authenticator before the gateway starts. Each reset is recorded as
`ROTATE administrator totp`. The gateway's start-up line says how many administrators
still have no secret; it should say none. [VLD-ANB-MIDGARD-0008]

## 6 — Re-sign models and give the sidecar a serve directory

Only for `mimir-server` serving a real model (`--engine llama-cpp` or `vllm`):

1. Sign each model again so its manifest is format 2. In staging, with the model key:
   ```bash
   rm MODEL_DIR/model.manifest
   mg-model sign --weights MODEL_DIR/model.gguf --id ... --family ... --version ... \
       --quantisation ... --licence ... --sha256 PUBLISHED_SHA256 \
       --keystore model-signing.keystore
   mg-model verify --dir MODEL_DIR --trust model-signing.pub
   ```
   `mg-model sign` now requires `--sha256`, the digest the upstream published. On
   compose, `bash deploy/compose/stage-model.sh` does this.
2. Move the trust key out of the models volume. `mimir-server` refuses a
   `--model-trust` inside `--model-dir`, and compose and the chart now trust the pinned
   anchor `deploy/trust/model-signing.pub` (in the image at
   `/etc/midgard/trust/model-signing.pub`; the chart's `mimir-model-trust` ConfigMap is
   built from it). A key an earlier `stage-model.sh` left under the models volume is no
   longer read: once the key ceremony has committed the anchor, re-sign under the
   staging key and run `bash deploy/compose/stage-model.sh`; until then, run it with
   `--development` and add `deploy/compose/compose.dev-model.yaml`. Without either,
   `mimir-server` does not start (fail closed). [VLD-ANB-MIDGARD-0028]
3. Give `mimir-server` a `--serve-dir` that only it can write. It stages the verified
   weights there, and the sidecar loads them from there, read-only — never from the model
   directory. `deploy/compose/compose.granite.yaml` and the Helm chart show the wiring.

## 6a — Rebuild every Heimdall feed source

Observations are format version 2: each carries its feed's sequence inside the signed
bytes, and `heimdall-feed` refuses a version 1 observation ("unsupported observation
format version 1 (no sequence)"). Rebuild every feed source from this release and deploy
it together with `heimdall-feed`; a source left on the earlier build has every
observation refused. Pictures and deltas are not affected. [VLD-ANB-MIDGARD-0041]

## 7 — Start and check

Start the gateway first, then the mesh services. Each one re-verifies its ledger, its
certificate and its revocation list at start and refuses to start if anything is wrong;
its log says which file and why. Then:

- sign in to the console with an existing user (their password hash is upgraded now).
  Every session from before the upgrade has ended: tokens are `midgard-token-v2`, so
  everyone signs in once more. [VLD-ANB-MIDGARD-0021]
- make one administration request as each administrator, with their new one-time code;
- `mg-admin user list --state-dir STATE` with the gateway stopped, or the console's
  admin views, to see the enrolments are intact;
- on compose, `just compose-probe` for one audited inference call over the mesh.

## 8 — Afterwards

- **Renew certificates before they expire.** They are valid for 30 days by default.
  Run `mg-admin mesh renew --within 10` for each service daily, from a timer, and restart
  the service it renews ([PKI.md](PKI.md)). Services do not yet renew themselves.
- **Revocation now works.** `mg-admin mesh revoke --cert FILE --out DIR,DIR,...` signs a
  newer list into every directory named; running services pick it up within 30 seconds.
- **Check the break-glass role bindings.** A grant now needs the requester to hold
  `breakglass.request` and a different administrator holding `breakglass.approve`.

## Rolling back

There is no in-place rollback. The audit ledgers (entry schema 2), the identity directory
(record versions 3 and 4) and the keystores (version 3) written by this release do not
open on an earlier build. To go back:

1. Stop the gateway and every mesh service.
2. Restore the backup from [step 1](#1--back-up): the state directory, every service's
   state directory and every identity directory, all from the same backup.
3. Restore the pre-migration keystores from the secret store, and start the earlier
   build.

Anything recorded after the upgrade (ledger entries, enrolments, sealed state) is lost
with the restore; export what you need first.
