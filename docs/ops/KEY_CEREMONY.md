# Release key ceremony

The step-by-step procedure for generating Midgard's release keys. Follow it top to
bottom and tick each box as you go. It is written so that two people who have never
held a ceremony can hold this one correctly the first time. [RT-35] [AC-RF-CA-6a]
[NFR-PQ-3]

Most of the work is done by one program, **`mg-ceremony`** (`mg-ceremony.exe` on
Windows). It asks one question at a time, checks every answer as you give it, shows
you everything before a key exists, and will not start until you type `GENERATE`. It
can also run a **rehearsal** with throwaway keys, so you can practise first. It uses
no network, commits nothing and pushes nothing.

Why each step exists, and what the keys protect, is in [PKI.md](PKI.md) ("Signing
roots" and "Git signing"). You do not need to read it to hold the ceremony.

## What the ceremony produces

| Output | What it is | Where it goes | Secret? |
|---|---|---|---|
| `release.keystore` | the **release signing key** (ML-DSA-87), which signs every release artefact | the encrypted custody drive, then the signing runner | **Yes.** Whoever holds this file can sign as Midgard. |
| `bifrost.keystore` | the **Bifrost admission key** (ML-DSA-65), which admits artefacts to the registry | the encrypted custody drive, then the signing runner | **Yes.** |
| `deploy/trust/release-trust.pub` | the public half of the release key | committed to the repository | No |
| `deploy/trust/bifrost-trust.pub` | the public half of the admission key | committed to the repository | No |
| `deploy/trust/allowed_signers` | the SSH keys of the people who may sign a release tag | committed to the repository | No |
| `docs/ops/ceremonies/<date>-release-keys.md` | the ceremony record: every digest and fingerprint, and the sign-off | committed to the repository | No |

Until these exist, no release can be tagged or signed: every check **fails closed**.
That is deliberate.

## The procedure at a glance

| Part | When | Who | Time (anticipated) |
|---|---|---|---|
| [A. Prepare](#part-a-prepare) | the week before | Security Officer, each tag signer | about 1 hour in total |
| [B. Hold the ceremony](#part-b-hold-the-ceremony) | the day | Security Officer and witness, together | about 30 minutes |
| [C. Publish and commit](#part-c-publish-and-commit) | straight after | Security Officer and witness | about 20 minutes |
| [D. Provision the signing runner](#part-d-provision-the-signing-runner) | within the week | whoever administers the runner, with the Security Officer | about 30 minutes |

## Roles

| Role | Who | Does |
|---|---|---|
| **Security Officer** | the duty Security Officer | operates the ceremony machine and answers the program's questions |
| **Witness** | a second person, **not** the Security Officer | reads every value on screen, checks it against this document and the record, and co-signs |
| **Tag signer** | each person who may cut a release tag (the Security Officer may be one) | provides their SSH **public** key before the day |

The two people at the ceremony must be different people (dual control). The program
refuses the same name for both.

## What you need

- [ ] **A ceremony machine.** A Windows 11 or Linux laptop that is fully updated, has
  full-disk encryption on (BitLocker on Windows) and its anti-malware running. Its
  network is switched off during part B.
- [ ] **Two encrypted USB drives**, labelled `KEYS` and `KEYS-BACKUP`. On Windows,
  BitLocker To Go; on Linux, LUKS. Each holds one copy of the keystores, and they
  are stored in different places afterwards.
- [ ] **A password manager** (KeePass, for example) for the two drives' passwords and
  BitLocker recovery keys. Never store a recovery key on the drive it unlocks.
- [ ] **A Midgard checkout** on the ceremony machine, at a reviewed commit of the
  default branch, with `mg-ceremony` built from it (step A3).
- [ ] **Each tag signer's SSH public key**, the `.pub` file (step A2). It is public, so
  it can arrive by email or USB stick.
- [ ] **A second channel**, agreed in advance (step A1).

---

## Part A: Prepare

### A1. Agree the date, the people and the second channel

- [ ] Choose the Security Officer, the witness and the tag signers. Write their names
  down; the program asks for them.
- [ ] Agree the **second channel**. After the ceremony you publish the fingerprints and
  digests of the new public keys somewhere other than the git repository, so that
  anyone can confirm the committed files were not swapped. It must be a place
  readers trust independently of the repository: for example the Veldris website,
  an email to the release mailing list, or a post in the security team's channel.
  Note which one now.

### A2. Each tag signer: make an SSH signing key

Each person who may sign a release tag needs an SSH signing key of their own, and sends
the **public** half (`.pub`) to the Security Officer. Skip this if you already sign
your git commits with an ed25519 SSH key.

**Windows** (PowerShell; the OpenSSH client is built into Windows 10 and 11):

```powershell
ssh-keygen -t ed25519 -C "you@veldris.io" -f "$HOME\.ssh\id_ed25519_midgard_signing"
git config --global gpg.format ssh
git config --global user.signingkey "$HOME\.ssh\id_ed25519_midgard_signing.pub"
```

**Linux**:

```bash
ssh-keygen -t ed25519 -C "you@veldris.io" -f ~/.ssh/id_ed25519_midgard_signing
git config --global gpg.format ssh
git config --global user.signingkey ~/.ssh/id_ed25519_midgard_signing.pub
```

- [ ] Give the key a strong passphrase when `ssh-keygen` asks, and keep it in your
  password manager.
- [ ] If you have a FIDO2 security key and your OpenSSH supports it, use
  `-t ed25519-sk` instead: the private key then never leaves the hardware.
- [ ] Use the email address you tag with (`git config user.email`) in `-C`.
- [ ] Send `id_ed25519_midgard_signing.pub` (the `.pub` file only) to the Security
  Officer, and tell them its fingerprint through a different route (a call, or
  in person). Print the fingerprint with:

  ```text
  ssh-keygen -lf ~/.ssh/id_ed25519_midgard_signing.pub
  ```

> Never send the file without `.pub`. That is the private key. The program refuses a
> private key if one is offered, but it should never leave your machine at all.

The program can also make this key for a person on the spot during the ceremony (option
`n` at step 4), but making it in advance is better: the key then lives on the signer's
own machine, not the ceremony machine.

### A3. Security Officer: build `mg-ceremony` on the ceremony machine

Build it from the reviewed checkout, so the program that generates the keys is one you
can trace to a commit. Do this while the machine is still online; the build itself
needs no network, but installing the toolchain does.

**Windows:**

1. Install [Git for Windows](https://git-scm.com/download/win) and
   [rustup](https://rustup.rs). Rustup asks for the Visual Studio C++ Build Tools if
   they are missing; accept its default ("Desktop development with C++").
2. In PowerShell:

   ```powershell
   git clone https://github.com/veldris-ltd/midgard-app.git
   cd midgard-app
   git log -1 --format="%H %s"
   cargo build --release --offline -p mg-ceremony
   ```

   The repository pins its Rust version (`rust-toolchain.toml`), and rustup fetches it
   on first use. `--offline` builds only from the crates committed under `vendor/`.
3. Check `git log` printed the commit you reviewed. The program is now
   `target\release\mg-ceremony.exe`.

**Linux:**

```bash
git clone https://github.com/veldris-ltd/midgard-app.git
cd midgard-app
git log -1 --format="%H %s"
cargo build --release --offline -p mg-ceremony
```

The program is now `target/release/mg-ceremony`. (`just ceremony` and
`bash deploy/trust/ceremony.sh` build and start it in one go.)

- [ ] Built from the reviewed commit, and that commit hash noted. The record also
  captures it automatically.

### A4. Rehearse

A rehearsal asks the real questions, generates throwaway keys in a temporary folder,
shows the real output, then deletes everything. Do it at least once, ideally with the
witness, so that nothing on the day is a surprise.

- [ ] Start the program (double-click `target\release\mg-ceremony.exe` on Windows, or
  run `target/release/mg-ceremony` on Linux).
- [ ] Choose **1 Rehearse**, and answer the questions. If a signer's key has not
  arrived yet, choose `t` to use a practice key; it is offered in rehearsals only.
- [ ] Read the output to the end. Part B step B6 explains it.

### A5. Prepare the two USB drives

**Windows:** insert each drive, then in File Explorer right-click it, choose
**Turn on BitLocker**, choose **Use a password to unlock the drive**, and save the
recovery key to your password manager (or print it), never to the drive itself.

**Linux:** format each drive as LUKS, for example with GNOME Disks
("Encrypt, compatible with Linux systems"), and keep the passphrase in your password
manager.

- [ ] Both drives encrypted, labelled `KEYS` and `KEYS-BACKUP`, and empty.

---

## Part B: Hold the ceremony

Both people are present for the whole of part B. The witness reads each value on
screen aloud; the Security Officer confirms it.

### B1. Open

- [ ] Both people present. Note the start time.
- [ ] The checkout is at the reviewed commit:
  `git -C <checkout> log -1 --format="%H %s"`.
- [ ] `git -C <checkout> status` shows nothing uncommitted.

### B2. Take the ceremony machine offline

- [ ] Switch off Wi-Fi and unplug any network cable (on Windows, **Airplane mode** on).
  The keys are generated with no network path at all.

### B3. Insert and unlock the `KEYS` drive

- [ ] Insert `KEYS` and unlock it. Note its drive letter or mount point, for example
  `E:\` on Windows or `/media/<you>/KEYS` on Linux.
- [ ] Leave `KEYS-BACKUP` out for now.

### B4. Start `mg-ceremony` and choose 2

- [ ] Windows: double-click `target\release\mg-ceremony.exe`. Linux: run
  `target/release/mg-ceremony` from the checkout, or `just ceremony`.
- [ ] Choose **2 Hold the ceremony**.

### B5. Answer its five questions

| Step | It asks | You answer | The witness checks |
|---|---|---|---|
| 1 | the Midgard checkout | `y`, if the folder it found is the checkout from B1; otherwise type the checkout's folder | it is the checkout from B1 |
| 2 | the Security Officer's and the witness's full names | both names, as they should appear in the record | the names are right and are two different people |
| 3 | where the private keys go | a **new** folder on the `KEYS` drive, for example `E:\midgard-keys-2026` or `/media/<you>/KEYS/midgard-keys-2026` | it is on `KEYS`, not the laptop's own disk |
| 4 | each tag signer: their email address, then their `.pub` file | the email they tag with, then `p` and the path of their `.pub` file (or the number of a key it lists) | the fingerprint it prints matches the one the signer gave you by the other route in A2 |
| 5 | a summary of everything, then the word `GENERATE` | read the summary aloud; type `GENERATE` only if all of it is right | every line of the summary |

The program checks each answer as you give it and asks again if one is wrong, for
example a folder inside the checkout, a folder that is not empty, or a private key
offered as a signer. Anything other than `GENERATE` at step 5 stops it, and nothing is
generated.

### B6. Read the result

After `GENERATE` the program prints:

1. **Keystores**: the two files on `KEYS`, with their algorithms and key ids
   (`midgard-release-<year>`, `midgard-bifrost-<year>`).
2. **Ceremony record**: the record it wrote under `docs/ops/ceremonies/`.
3. **The second-channel text**: the SHA-256 of each published file and the fingerprint
   of each tag signer's key. You publish this, unchanged, in part C.
4. **Next steps**: the same as part C below, with the exact commands for this
   ceremony.

- [ ] The witness reads the second-channel text aloud and compares it with the record
  file (open it in a text editor). They must match exactly.
- [ ] Copy the second-channel text into a text file (for example
  `second-channel.txt` beside the checkout, not inside it). You need it in C1, and
  it is public, so this is safe. On Windows, the console window stays open until
  you press Enter, so copy it before you close it.

### B7. Make the backup copy (escrow)

The disaster-recovery plan needs a second copy of the keys, held apart from the first
([DR_PLAN.md](DR_PLAN.md), step 4).

- [ ] Insert and unlock `KEYS-BACKUP`.
- [ ] Copy the whole `midgard-keys-<year>` folder from `KEYS` to `KEYS-BACKUP`.
- [ ] Check the copies are identical. Windows (PowerShell; replace the drive letters):

  ```powershell
  Get-FileHash E:\midgard-keys-2026\*.keystore, F:\midgard-keys-2026\*.keystore
  ```

  Linux:

  ```bash
  sha256sum /media/$USER/KEYS/midgard-keys-2026/*.keystore /media/$USER/KEYS-BACKUP/midgard-keys-2026/*.keystore
  ```

  Each keystore's hash must be the same on both drives.

### B8. Close

- [ ] Eject both drives (**Eject** in File Explorer, or `udisksctl unmount` and
  `udisksctl lock` on Linux) and remove them.
- [ ] Check no keystore was left on the laptop: search the laptop's disk for
  `*.keystore`. There must be none.
- [ ] Store `KEYS` and `KEYS-BACKUP` in separate locked places. Note where in your
  security log, not in the repository: the record deliberately does not say where
  the keys are.
- [ ] Only now turn the network back on.

---

## Part C: Publish and commit

### C1. Publish the second-channel text

- [ ] Publish the text from B6, unchanged, through the second channel agreed in A1.
- [ ] Open the record (`docs/ops/ceremonies/<date>-release-keys.md`) and, under
  "Second channel", replace `_where, and when_` with where you published it and
  the date and time.

### C2. Sign off the record

- [ ] Both people compare the record with the published second-channel text once more.
- [ ] In the record's "Sign-off" table, each fills in "Confirmed" (`Yes`) and the date.

### C3. Commit, in one signed commit

From the checkout (the program printed these lines with the real record name):

```bash
git switch -c ops/key-ceremony-2026
git add deploy/trust/release-trust.pub deploy/trust/bifrost-trust.pub deploy/trust/allowed_signers docs/ops/ceremonies/<date>-release-keys.md
git commit -S -m "ops: release key ceremony, <date>"
git push -u origin ops/key-ceremony-2026
```

- [ ] Open a pull request (GitHub) or merge request (CodeLab) as usual. The reviewer
  checks that the values in the diff match the second-channel text before
  approving.
- [ ] `git status` shows no `.keystore` file anywhere. (The repository ignores
  `*.keystore`, and the program never writes one inside the checkout, but check.)

### C4. Register each tag signer's key on the forge

Each tag signer registers the **same** `.pub` file as a **signing** key, so that the
branch rule requiring signed commits accepts them:

- **GitHub:** Settings, then **SSH and GPG keys**, then **New SSH key**; set
  **Key type** to **Signing Key** and paste the contents of the `.pub` file.
- **GitLab (CodeLab):** Preferences, then **SSH Keys**, then **Add new key**; set
  **Usage type** to **Signing** (or **Authentication & Signing**).

### C5. Anyone can check

Once the commit is merged, anyone can confirm the committed files match what was
published:

```text
mg-ceremony check --repo <their checkout>
```

It applies the release's own checks to the three files and prints the same
second-channel text, to compare line by line. (Started with no arguments, the program
offers this as option **3**.)

---

## Part D: Provision the signing runner

The release jobs run on a self-hosted runner labelled `[self-hosted, kms]`. It needs
the two keystores, as files outside any checkout. The full list of what the runner
needs is [PKI.md, "Signing runner"](PKI.md#signing-runner-release); the key steps are
below.

- [ ] On the runner host, as an administrator, with `KEYS` inserted (replace the user
  and paths for your host):

  ```bash
  sudo install -d -m 700 -o runner -g runner /srv/midgard-keys /srv/midgard-registry
  sudo install -m 400 -o runner -g runner /media/$USER/KEYS/midgard-keys-2026/release.keystore /srv/midgard-keys/release.keystore
  sudo install -m 400 -o runner -g runner /media/$USER/KEYS/midgard-keys-2026/bifrost.keystore /srv/midgard-keys/bifrost.keystore
  ```

- [ ] Set the three secrets to those paths. They are paths, not the keys themselves.
  - **GitHub:** repository Settings, then **Secrets and variables**, then
    **Actions**, then **New repository secret**.
  - **GitLab:** Settings, then **CI/CD**, then **Variables**; mark each
    **Protected**.

  | Secret | Value |
  |---|---|
  | `MIDGARD_SIGNING_KEYSTORE_PATH` | `/srv/midgard-keys/release.keystore` |
  | `MIDGARD_BIFROST_KEYSTORE_PATH` | `/srv/midgard-keys/bifrost.keystore` |
  | `MIDGARD_BIFROST_REGISTRY_PATH` | `/srv/midgard-registry` |

- [ ] Run the preflight on the runner, from a checkout that includes the ceremony
  commit:

  ```bash
  MIDGARD_SIGNING_KEYSTORE=/srv/midgard-keys/release.keystore \
  MIDGARD_BIFROST_KEYSTORE=/srv/midgard-keys/bifrost.keystore \
  MIDGARD_BIFROST_REGISTRY=/srv/midgard-registry \
  bash tools/ci/release-preflight.sh
  ```

  It ends with `preflight: the signing runner is provisioned`, or names what is
  missing.
- [ ] Eject `KEYS` and return it to its locked place.

---

## If something goes wrong

| What you see | What it means | What to do |
|---|---|---|
| `deploy/trust/... already exists` | a ceremony has been held in this checkout already | Stop. Rotation is a different procedure (see "Rotation"). If the files are left from an abandoned attempt, see the last row. |
| `... is inside this checkout` | the custody folder you typed is inside the repository | Type a folder on the `KEYS` drive. |
| `... is not empty` | the custody folder already holds something | Type a new folder name, for example add `-2` to it. |
| `this is a private key` | the file offered for a signer is a private key | Use the file with the same name ending in `.pub`. Tell the signer their private key has been handled; if it left their machine, they make a new key (A2). |
| `not an SSH public key git can sign tags with (got 'ssh-rsa')` | the signer's key is RSA, or not an SSH key | The signer makes an ed25519 key (A2). |
| `the key does not match its stated type` | the `.pub` file is damaged or was edited | Ask the signer for the file again. |
| The window closes at once (Windows) | the program stopped with an error before it could pause | Open PowerShell in `target\release` and run `.\mg-ceremony.exe` to read the message. |
| No `ssh-keygen` (option `n` not offered) | the OpenSSH client is not installed | Windows: Settings, then **System**, then **Optional features**, then add **OpenSSH Client**. Or make the key in advance (A2). |
| The fingerprint a signer gave you differs from the one on screen | the `.pub` file is not the key they hold | Stop at step 5 (type anything but `GENERATE`). Get the right file and start again. |
| You need to abandon after `GENERATE` | for example a value is wrong | Delete `deploy/trust/release-trust.pub`, `deploy/trust/bifrost-trust.pub`, `deploy/trust/allowed_signers` and the new record in the checkout, and the custody folder on `KEYS` (and on `KEYS-BACKUP`, if copied). Nothing was committed, so nothing else needs undoing. Then start again from B4 with a new custody folder. |

If a keystore is lost or may have been copied by someone it should not have been,
treat it as an incident ([INCIDENT_RESPONSE.md](INCIDENT_RESPONSE.md)) and rotate the
key.

## Rotation

The release and admission keys are rotated annually, or at once on suspicion of
compromise ([PKI.md, "Rotation cadence"](PKI.md#rotation-cadence)). `mg-ceremony`
holds a **first** ceremony only: it refuses when the published files exist, so it can
never overwrite an anchor. To rotate, generate the new key with
`mg-sign keygen --alg ML-DSA-87 --key-id midgard-release-<year> --keystore <custody>/release.keystore --export-verifying-key new.pub`
(ML-DSA-65 and `midgard-bifrost-<year>` for the admission key), **append** the new
anchor line beside the old one, and remove the old line once everything signed under
it has been re-signed ([deploy/trust/README.md](../../deploy/trust/README.md)). Hold
the rotation with the same two people, the same offline machine and the same record
discipline as part B.

## Custody today, and the target

Today the keystores are files. Each holds its signing key sealed under an ML-KEM root
in the same file, so **the file is the key**: there is no separate passphrase, and its
protection is the encrypted drive, the runner's file permissions and who can reach
them. The target is a FIPS 140-3 HSM, with the key generated on the device and never
exportable ([PKI.md, "HSM key ceremony"](PKI.md#hsm-key-ceremony-ca--release-signing-key));
that path is not wired in yet. When it is, this procedure changes at step B5 (custody)
and part D, and the rest stays.

## What `mg-ceremony` checks, for reference

Before anything is generated, it refuses:

- a folder that is not a Midgard checkout, or a checkout where any of the three
  published files already exists;
- a custody folder inside the checkout, or one that is not empty;
- a signer file that is a private key, is not an ed25519 or ECDSA P-256 SSH key
  (including their FIDO forms), or whose key does not match its stated type;
- a signer listed twice, an email address with spaces or quotes, or no signer at all;
- the same person as Security Officer and witness (guided ceremony).

After generating, it writes each keystore and reads it back before publishing anything,
so a published anchor always pins a key that is held; then it checks the three
published files exactly as `tools/ci/release-preflight.sh` and `tools/ci/verify-tag.sh`
will. The key generation is `attest::Keystore`, the same code `mg-sign keygen` runs;
`mg-ceremony` adds no cryptography of its own. Its tests are
`tools/mg-ceremony/tests/cli.rs` and `tools/ci/tests/ceremony.test.sh`.

Command-line form, for scripted or repeat use:

```text
mg-ceremony run --repo <checkout> --custody <new folder> --officer "<name>" --witness "<name>" --signer <email>=<file.pub> [--signer ...] [--year YYYY]
mg-ceremony check --repo <checkout>
```
