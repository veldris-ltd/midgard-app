# KMS trust root — residual risk of launching software-only

The Midgard KMS seals data at rest with **AES-256-GCM under an ML-KEM-1024-wrapped
key, bound to a trust root** (`services/kms`). The trust root is where the root
secret lives and where unwrapping happens. Two implementations exist behind the
`TrustRoot` trait:

| Root | Where the root secret lives | Unwrap runs | Intended use |
|------|-----------------------------|-------------|--------------|
| `SoftwareTrustRoot` | **In process memory** | In process | Dev / CI only |
| `HardwareTrustRoot` (over a `HardwareToken`) | **Inside the device** (TPM 2.0 / PKCS#11 HSM) | On the device | Production |

`HardwareToken` is the device driver seam. A production build supplies a real
PKCS#11 or TPM 2.0 driver (see `src/hardware.rs` for the operation mapping);
`SoftHsmToken` is a software simulation of the device boundary used offline so the
architecture is testable without hardware.

## What the hardware root guarantees (RC-28)

- The root ML-KEM private key is **generated on and never leaves** the device
  (`CKA_EXTRACTABLE=false` / `fixedTPM`).
- Sealing needs only the **public** key (encapsulation); unsealing sends the wrapped
  key to the device and receives back the shared secret. The KMS process **never
  holds the root secret**.
- The device issues a **signed attestation** binding the root key id + public key and
  asserting non-exportability; operators verify it at provisioning
  (`HardwareTrustRoot::verify_attestation`).
- **Rotation** (`RotatingKms`) seals new data to the new root and keeps prior roots
  for unsealing — **old blobs are never rewritten**.

## Residual risk if you launch with `SoftwareTrustRoot`

Launching software-only is supported for development but carries real risk in
production. If you must, understand and accept the following:

1. **Root secret exposure.** The ML-KEM-1024 root private key is resident in process
   memory. Any adversary who can read the process address space — a memory-disclosure
   bug, a core dump, swap paged to disk, a malicious co-tenant, a debugger, or a live
   VM/container snapshot — **can exfiltrate the root and decrypt every blob ever
   sealed to it**, offline and undetectably. No PQ strength helps once the key leaks.
2. **No hardware attestation.** There is no device quote proving the key is
   non-exportable; `verify_attestation` is unavailable, so a compromised or swapped
   software root cannot be distinguished from a genuine one by remote attestation.
3. **Weaker rotation blast-radius control.** Rotation still works, but because a
   leaked software root cannot be revoked in hardware, a disclosed root compromises
   all data sealed under it until that data is re-sealed under a new root **and** the
   old blobs are destroyed/expired.
4. **Key-at-rest for the root itself.** A software root must persist its secret
   somewhere to survive restart; that storage becomes a second at-rest secret needing
   its own protection, reintroducing the bootstrapping problem the HSM/TPM solves.

## Recommendation

- **Production / any classified or regulated data:** use `HardwareTrustRoot` over a
  FIPS 140-3 (or CC EAL4+) PKCS#11 HSM or a TPM 2.0 with a PQ-capable command set.
  Verify the attestation at provisioning and on a schedule.
- **Development / CI / air-gapped demos:** `SoftwareTrustRoot` (or `SoftHsmToken`) is
  acceptable, provided no production secrets are sealed under it.
- **If launching software-only in production is unavoidable:** record the accepted
  risk in the deployment's risk register, restrict process memory access (locked
  memory, no swap, no core dumps, no snapshotting), encrypt the persisted root under
  an independent operator-held key, and plan a migration to a hardware root before
  handling sensitive material.
