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
| Keystores are format version 2: the custody root is wrapped under a passphrase, and every program that opens one needs `--keystore-passphrase-file` or `MIDGARD_KEYSTORE_PASSPHRASE_FILE`. | VLD-ANB-MIDGARD-0007 | A version 1 keystore (root in the clear) is refused everywhere. → [Keystores](#keystores-wrap-the-custody-root). |

## 1 — Back up

Take the backup in [BACKUP_RESTORE.md](BACKUP_RESTORE.md) of the gateway's state
directory, every service's state directory, and every identity directory. The steps below
write new certificates and checkpoints and rewrite each keystore once, wrapped (the keys
inside are kept), but a backup is how you go back. Keep the pre-migration keystores in
the secret store, not on the host, until the release has soaked, then destroy them: they
hold the custody root in the clear.

## 2 — Stop everything, then install the new build

Stop the gateway and every mesh service (`mimir-server`, `bifrost-fleet`, `heimdall-feed`,
`forge-worker`, and any agents, sources or probes). The steps below take the state
directory's lock, which a running gateway holds, and a service must not be appending to a
ledger while it is adopted. Then install the new binaries or images.

### Keystores: wrap the custody root

Before anything opens a keystore, give each keystore holder a passphrase and migrate its
keystores. Compose `init` and `mg-install`'s repair do this themselves: each holder gets a
random passphrase in a volume of its own (`<holder>-passphrase`), and every version 1
keystore is migrated under it. By hand, for each holder (the state directory's `keys/`
and the gateway's mesh identity share one; each other service directory has its own):

```bash
umask 077; head -c 32 /dev/urandom | od -An -vtx1 | tr -d ' \n' > /secrets/mimir.passphrase
mg-admin keystore migrate --keystore DIR/mimir-server.keystore --keystore-passphrase-file /secrets/mimir.passphrase
mg-admin keystore migrate --keystore DIR/mimir-server.audit.keystore --keystore-passphrase-file /secrets/mimir.passphrase
mg-admin keystore verify --keystore DIR/mimir-server.keystore --keystore-passphrase-file /secrets/mimir.passphrase
```

Then pass each service `--keystore-passphrase-file FILE` (or set
`MIDGARD_KEYSTORE_PASSPHRASE_FILE`). Keep the passphrase off the keystore's volume and out
of its backups: it is the recovery secret, and a lost passphrase is a lost enclosure.
Migration keeps the same root, so destroy every other copy of each version 1 file
(backups, volumes, Secrets); where one may have been exposed, re-initialise that
enclosure instead of migrating it. Release and admission keystores on the signing runner,
and Dvalin's ceremony output, are migrated the same way ([PKI.md](PKI.md),
[KEY_CEREMONY.md](KEY_CEREMONY.md)). [VLD-ANB-MIDGARD-0007]

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
makes — would leave the service unable to open either. `--days` (default 30) and
`--clearance` (default `open`) apply as at issue. On a service that is already current it
changes nothing.

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

## 6 — Re-sign models and give the sidecar a serve directory

Only for `mimir-server` serving a real model (`--engine llama-cpp` or `vllm`):

1. Sign each model again so its manifest is format 2. In staging, with the model key:
   ```bash
   rm MODEL_DIR/model.manifest
   mg-model sign --weights MODEL_DIR/model.gguf --id ... --family ... --version ... \
       --quantisation ... --licence ... --keystore model-signing.keystore
   mg-model verify --dir MODEL_DIR --trust model-signing.pub
   ```
   On compose, `bash deploy/compose/stage-model.sh` does this.
2. Give `mimir-server` a `--serve-dir` that only it can write. It stages the verified
   weights there, and the sidecar loads them from there, read-only — never from the model
   directory. `deploy/compose/compose.granite.yaml` and the Helm chart show the wiring.

## 7 — Start and check

Start the gateway first, then the mesh services. Each one re-verifies its ledger, its
certificate and its revocation list at start and refuses to start if anything is wrong;
its log says which file and why. Then:

- sign in to the console with an existing user (their password hash is upgraded now);
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
