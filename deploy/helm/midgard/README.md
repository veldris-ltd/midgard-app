# Midgard Helm chart: secret custody

The chart deploys the five services (see [deploy/README.md](../../README.md)). This
page is about the one thing the chart does not create: each service's identity,
keystore passphrase and seed. They are the enclosure's custody material, so where they
come from is an explicit choice, `secrets.provider`. [VLD-ANB-MIDGARD-0018]

## What each workload mounts

| Name | Holds | Where the service sees it |
|------|-------|---------------------------|
| `<name>-identity` | `<name>.keystore`, `<name>.audit.keystore`, `<name>.cert`, `mesh.anchor`, `mesh.crl` | `/identity`, read-only |
| `<name>-keystore-passphrase` | `passphrase`: the passphrase the keystores are wrapped under (keystore format version 3) | `/keystore-passphrase/passphrase`, through `MIDGARD_KEYSTORE_PASSPHRASE_FILE` |
| `<name>-seed` (http-gateway, mimir-server, bifrost-fleet, heimdall-feed) | `seed.tar`: keys, trust anchors and configuration, unpacked into `/state` on first start only | `/state` |

For http-gateway the passphrase is the enclosure's: it unwraps `keys/idp.keystore` and
`keys/audit.keystore` in the seed as well as the gateway's mesh identity. The seed
holds `keys/`, `trust/`, `policy.json`, `ontology.json` (`channels.json` for
bifrost-fleet) and, for http-gateway, the governance ledger and its checkpoint that
`mg-admin init` created: the gateway refuses to start without them, because a missing
ledger is indistinguishable from a deleted one (MG-SEC-05). It never holds the identity
directory, the object store, admin state or any other journal; the services create
those empty on first start.

### Why the passphrase is copied before the service starts

The services refuse a passphrase file that any group or other user can read (it must
be mode 0600 or 0400; `attest::keystore::RootWrap::passphrase_from_file`). A Kubernetes
secret volume is written by the kubelet as root: in a pod that runs as a non-root user
the file is readable only through `fsGroup`, which makes it group-readable (0440), and
Kubernetes has no setting that gives a secret volume's files to the pod's user (only
projected service-account tokens get that). CSI volumes are no better. So the service
never mounts the passphrase volume. An init container, running as the service's own
user, copies it into a memory-backed `emptyDir` as a file it owns, mode 0400, and the
service mounts that read-only. The 0600 rule is unchanged, and the copy lives only in
the pod's memory, as the secret volume itself does.

## Providers

### `kubernetes` (default): Secrets created outside the chart

```yaml
secrets:
  provider: kubernetes
  acknowledgeEtcdEncryption: true   # only once etcd encrypts Secrets at rest
```

The chart mounts Secrets of the names above. A Kubernetes Secret is stored in etcd, and
in every etcd backup, so **encryption at rest is mandatory for this provider**:
configure the API server with an
[`EncryptionConfiguration`](https://kubernetes.io/docs/tasks/administer-cluster/encrypt-data/)
that encrypts `secrets` with a
[KMS v2 provider](https://kubernetes.io/docs/tasks/administer-cluster/kms-provider/)
backed by your key custody, not `identity` or a static `aescbc` key on the control-plane
disk. Outside the kind profile (`image.single` empty) the render fails until you set
`secrets.acknowledgeEtcdEncryption: true`, so the decision is recorded in your values.
`deploy/k8s/enclosure.yaml` is rendered with it set; its header says so.

### `csi`: Secrets Store CSI driver

```yaml
secrets:
  provider: csi
```

Each volume is `csi: { driver: secrets-store.csi.k8s.io, readOnly: true,
volumeAttributes: { secretProviderClass: <name> } }` for `<name>-identity`,
`<name>-keystore-passphrase` and `<name>-seed`. **You supply the SecretProviderClass
objects**, one per name, in the namespace, for your store's provider (Vault, Azure Key
Vault, AWS Secrets Manager, GCP Secret Manager), each mapping the files in the table
above. Nothing reaches etcd unless you enable the driver's `secretObjects` sync; do not.
The driver and the provider are installed by the cluster operator; the chart does not
install them.

### `externalSecrets`: external-secrets operator

```yaml
secrets:
  provider: externalSecrets
  externalSecrets:
    storeRef: { name: midgard-custody, kind: ClusterSecretStore }
    keyPrefix: midgard/
    refreshInterval: 1h
    decodingStrategy: Base64
```

The chart renders one `external-secrets.io/v1beta1` `ExternalSecret` per Secret each
workload mounts, populating exactly the names the `kubernetes` provider expects, so the
pod template is the same. Secret `<name>-identity` reads remote key
`<keyPrefix><name>-identity` with one property per file (`<name>.keystore`,
`<name>.audit.keystore`, `<name>.cert`, `mesh.anchor`, `mesh.crl`);
`<name>-keystore-passphrase` reads its own remote key, property `passphrase`; and
`<name>-seed` reads property `seed.tar`. Store each property base64-encoded (keystores
and seeds are binary). Keep the passphrase under a different access policy from the
keystores: either alone does not unseal anything. The operator installs
external-secrets and the store. The Secrets it writes are still in etcd: encrypt etcd
at rest here too; the store remains the source of truth for rotation and revocation.

## RBAC

- No cluster-wide `get`, `list` or `watch` on Secrets for people or controllers that
  do not need them; a `ClusterRole` that grants Secret `list` grants every keystore and
  passphrase in the cluster.
- In the namespace, a `Role` limited by `resourceNames` to the chart's Secrets
  (`<name>-identity`, `<name>-keystore-passphrase`, `<name>-seed`, the clients'
  identities and passphrases), bound only to whoever provisions them. The pods need no
  Secret API access: the chart sets `automountServiceAccountToken: false`.
- With external-secrets or CSI, the store's access policy is the custody boundary:
  grant each workload's remote keys to that workload's store identity only.

## Provisioning

- **kind only:** `deploy/kind/bootstrap-secrets.sh` (run by `deploy/kind/up.sh`) mints
  throwaway keys with the compose `init` and creates the Secrets. It refuses any kubectl
  context whose name does not start with `kind-`.
- **A real cluster:** issue identities from your key custody (`mg-admin init` and
  `mg-admin mesh issue`, [docs/ops/KEY_CEREMONY.md](../../../docs/ops/KEY_CEREMONY.md)),
  generate one passphrase per keystore holder and keep it apart from the keystores
  ([docs/ops/BACKUP_RESTORE.md](../../../docs/ops/BACKUP_RESTORE.md)), and load them
  into your store for the `csi` or `externalSecrets` provider.
- The mesh clients that run as Jobs (`mimir-probe`, `fleet-clients`,
  `heimdall-clients`) use `<client>-identity` and `<client>-keystore-passphrase` the
  same way; the chart does not render their Jobs.

## Renewal and revocation

Certificates are renewed with `mg-admin mesh renew` and revoked with
`mg-admin mesh revoke`, which writes a newer signed `mesh.crl`
([docs/ops/PKI.md](../../../docs/ops/PKI.md), "Renewal" and "Revocation"). Update
`<name>-identity` in your store (or the Secret, for the `kubernetes` provider): the
kubelet refreshes the mounted files and each service re-reads `mesh.crl` within 30
seconds. A changed passphrase needs a pod restart, since it is copied at pod start.

## Testing

`bash deploy/helm/midgard/tests/render-providers.sh` renders each provider and checks
the result (it needs helm); `bash tools/ci/run.sh k8s-render` runs it with the check
that `deploy/k8s/enclosure.yaml` is what the chart renders.
