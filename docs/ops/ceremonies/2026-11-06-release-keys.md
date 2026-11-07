# Key ceremony, 6 November 2026

Release signing and Bifrost admission keys, and the release tag signers, per
`docs/ops/KEY_CEREMONY.md`. Held with Dvalin 0.1.0, built on the ceremony machine from
dvalin-app `main` at `3d8725640c27911f7e2c2e49428cbaf0f9c5cbf0`, against midgard-app
commit `ebd0aeba07d138cc85c0e9b836225e80ad506f21`. Dvalin was started at
2026-11-06T20:55:41Z and the keys were generated at 2026-11-06T20:59:22Z (UTC; see
"Timeline").

The model-signing key was not generated at this ceremony; see "Not generated at this
ceremony" below, which gives the procedure for the follow-up session.

## Keys

| Key | Algorithm | Key id | Published as | SHA-256 of the published file |
|---|---|---|---|---|
| Release signing | ML-DSA-87 | `midgard-release-2026` | `deploy/trust/release-trust.pub` | `0b4d013bbbc265cab2f57b4532ef8da11f037fb781d6d3e8ea3e0b9d40b54919` |
| Bifrost admission | ML-DSA-65 | `midgard-bifrost-2026` | `deploy/trust/bifrost-trust.pub` | `07e6e4bbdb37b17c1cf99f821b8480fa28f273c8859311f86ff0935e3cd2fd6d` |

The private halves are sealed keystores held outside the repository. Where they are
held is not recorded here. Each is Midgard's keystore format 3: its root is wrapped
under a passphrase of its own, generated at the ceremony, recorded in the password
manager and confirmed by typing it back. No file in the repository holds one.

## Release tag signers

`deploy/trust/allowed_signers`, SHA-256 `80dbf0aa276f91f2644719a0eaa726281bb2453723fdc91fa4dccd041f2e269f`:

| Principal | SSH key fingerprint | Private key |
|---|---|---|
| jb.benjamin@veldris.io | `SHA256:y6RRuNoo49OPZc44e+OvgyztD7l/SQc6UV++WMTVM2Q` | on a security key (FIDO2) |

## Self-test

Before anything was published, each key as held in its keystore signed a test
artefact, and the signature verified against the published anchor, through
Midgard's `attest::sign_artefact` and `attest::verify_artefact`:

- `midgard-release-2026` (ML-DSA-87): signed and verified
- `midgard-bifrost-2026` (ML-DSA-65): signed and verified

## Machine isolation

The ceremony machine's links, read at the moment the keys were generated. Dvalin reads
them afresh when GENERATE is confirmed, writes that reading into the record, and will
not open or continue the ceremony while any link is up, so the machine was offline
from before "Hold the ceremony" was chosen until Dvalin was closed:

- Wi-Fi off: radio off
- Ethernet disconnected: no wired adapter connected
- Bluetooth off: radio off, or no Bluetooth hardware
- NFC off: no NFC radio found
- Other links none: no virtual, VPN or mobile link up
- Read at 2026-11-06T20:59:22Z (UTC), from the operating system, sending nothing.

The first version of this record dated the generation 20:55:41Z and said the links were
read "just before the keys were generated", which did not agree. The earlier time is
when Dvalin was started: Dvalin 0.1.0 takes its clock once, at start-up, and writes that
as the generation time. The reading at 20:59:22Z is the generation itself, and the
keystores and the published files carry that time.

## Timeline

All times UTC. Times to the second are from Dvalin, the custody register, git or
GitHub; times to the minute are file times on the ceremony machine.

| Time | What happened |
|---|---|
| 6 Nov 20:33:34 | dvalin-app `main` at `3d87256`, the commit Dvalin was built from |
| 6 Nov 20:36 | Dvalin built on the ceremony machine from that commit |
| 6 Nov 20:48 | midgard-app cloned on the ceremony machine at `ebd0aeb`, then `main` |
| 6 Nov 20:55:41 | Dvalin started; the two people present |
| 6 Nov 20:59:22 | GENERATE confirmed: links read, both keys generated, written to the keys folder, read back and self-tested; the three trust files and this record written into the checkout |
| 6 Nov 21:01:57 | release keystore passphrase recorded in the password manager, typed back and confirmed, the witness present |
| 6 Nov 21:03:25 | Bifrost keystore passphrase recorded, typed back and confirmed, the witness present |
| 6 Nov 21:03 | second-channel text saved beside the checkout, unchanged |
| 6 Nov 21:05:37 | the four files committed, signed, as `9a821e3` |
| 6 Nov 21:08 | keystores and custody register copied to the backup folder; the copies match the originals by SHA-256 (see "Backup") |
| 6 Nov 21:59:14 | pull request opened |
| 7 Nov 00:13:11 | review by the witness: changes requested (second channel empty, sign-off blank, model-signing key missing, isolation time) |
| 7 Nov 00:40 | second channel published in Brokkr (see "Second channel"); this record corrected |

## Backup

Key fragmentation was not used. The backup is a copy of the keystores and the custody
register, made at 21:08Z and checked identical by digest.

Where it is, as found on 7 November: the copy is a second folder on the same internal
disk of the ceremony machine as the keys folder, not on a separate encrypted drive. The
first version of this record said "a second encrypted drive, held apart from the first";
that is the intended custody (dvalin-app `docs/KEY_CEREMONY.md`, B8), not yet what
exists. Still to do, and to be recorded here when done: copy the keys folder to an
encrypted `KEYS-BACKUP` drive, check the copies by digest, remove the copy from the
ceremony machine's disk, and move the keys folder itself to the encrypted `KEYS` drive;
the password manager's database is kept apart from both.

## Not generated at this ceremony: the model-signing key

`deploy/trust/model-signing.pub` is not in this commit. `docs/ops/KEY_CEREMONY.md` lists
it among the five files the ceremony commits, but Dvalin 0.1.0 generates only the
release and admission keys, and the separate step that makes the model-signing key
(`mg-model keygen`, under "The model-signing key" in that document) was not run on the
day: `mg-model` had not been built on the ceremony machine, and the omission was noticed
in review.

Until it is committed:

- the release preflight (`tools/ci/release-preflight.sh`) refuses to run, so no release
  can be signed: fail-closed, as it was before this ceremony;
- `mimir-server` serves no real model under a pinned anchor; `deploy/compose/granite-smoke.sh`
  and `stage-model.sh --development` keep staging the Granite demo under a development
  key, with their warning;
- the Granite demo manifest cannot be re-signed under a production key.

### Procedure for the follow-up session

Held like the ceremony: offline, the Security Officer and the witness present
throughout, on the ceremony machine, in PowerShell. `mg-model keygen` writes
`model-signing.keystore` (format 3, its root wrapped under the passphrase file it is
given; it refuses to run without one and never overwrites) and `model-signing.pub`.

1. Take the machine offline: unplug any cable, Airplane mode on, NFC off. Both people
   check the lights the way Dvalin shows them, or Settings, Network and internet.
2. Build `mg-model` from the reviewed commit, in the ceremony checkout:

   ```powershell
   cd E:\key-signing\midgard-app
   git log -1 --format="%H %s"
   cargo build --release --locked -p mg-model
   ```

   The first line must print `ebd0aeba07d138cc85c0e9b836225e80ad506f21`.
3. Make the keystore passphrase in the password manager: a new entry "Midgard
   model-signing keystore 2026", a generated passphrase of at least 16 characters (32
   is fine), the witness watching it recorded. The passphrase must never be typed on
   a command line (PowerShell keeps a history), so put it in a file with Notepad:

   ```powershell
   notepad "$HOME\model-signing.passphrase"
   ```

   Paste the passphrase as the only line, save, close. The file is outside every
   checkout and off the keys folder, as `attest` requires.
4. Generate the key, into the folder that holds the other two keystores:

   ```powershell
   $env:MIDGARD_KEYSTORE_PASSPHRASE_FILE = "$HOME\model-signing.passphrase"
   .\target\release\mg-model.exe keygen --out E:\key-signing\keys\midgard-keys_2026 --key-id midgard-model-2026
   ```

   It prints the two paths it wrote. The key id follows the other two
   (`midgard-release-2026`, `midgard-bifrost-2026`).
5. Remove the passphrase file and the variable at once:

   ```powershell
   Remove-Item "$HOME\model-signing.passphrase"
   Remove-Item Env:\MIDGARD_KEYSTORE_PASSPHRASE_FILE
   ```

6. Publish the public half and read its digest, both people reading it aloud:

   ```powershell
   Copy-Item E:\key-signing\keys\midgard-keys_2026\model-signing.pub deploy\trust\model-signing.pub
   Get-Content deploy\trust\model-signing.pub
   Get-FileHash deploy\trust\model-signing.pub -Algorithm SHA256
   ```

   The file is one line beginning `ML-DSA-65 midgard-model-2026`.
7. Back up the keystore beside the other backups and check the copy:

   ```powershell
   Copy-Item E:\key-signing\keys\midgard-keys_2026\model-signing.keystore E:\key-signing\keys-backup\midgard-keys_2026\
   Get-FileHash E:\key-signing\keys\midgard-keys_2026\model-signing.keystore, E:\key-signing\keys-backup\midgard-keys_2026\model-signing.keystore -Algorithm SHA256
   ```

   (Or to the `KEYS-BACKUP` drive, once "Backup" above is done.)
8. Record it: add a row to the "Keys" table of this record (`Model signing`,
   `ML-DSA-65`, `midgard-model-2026`, `deploy/trust/model-signing.pub`, the digest),
   a timeline line with the generation time, and replace the first paragraph of this
   section with when it was generated. Commit the two files signed, and push:

   ```powershell
   git add deploy\trust\model-signing.pub docs\ops\ceremonies\2026-11-06-release-keys.md
   git commit -S -m "ops: model-signing key, <date>"
   git push
   ```

9. Second channel: add a dated update to the Brokkr note named under "Second channel"
   with the key id and the digest, and ask the witness to confirm it there.
10. Close: search the machine's disks for `*.passphrase` and any `*.keystore` outside the
    keys folder and its backup; there must be none. Only then go back online.
11. Afterwards, per `docs/ops/KEY_CEREMONY.md`: the keystore and its passphrase go to
    the staging enclave (the `midgard-model-signing` and
    `midgard-model-signing-passphrase` volumes), the Granite demo manifest is re-signed
    with `deploy/compose/stage-model.sh`, and the development key's volumes are removed.

## Second channel

The channel is Brokkr (`project.veldris.dev`, Veldris's SuiteCRM): a note under the
project **Midgard // Open Source**, written by the Security Officer from their own
account. Brokkr is where Midgard's findings and fixes are already tracked, the witness
has an account there, each note carries its author and time, and it is a separate
system from GitHub with separate credentials, so whoever could alter the repository
could not also alter the note. The note carries Dvalin's second-channel text, the
block that begins "Midgard key ceremony, 6 November 2026", with one change of
spelling: the placeholder `<your checkout>` in its last line is written
`(your checkout)`, because Brokkr hides angle brackets. It also says where the files
are, how to check them, and asks the witness to confirm there.

The fingerprints and SHA-256 values above were published at:

- Brokkr, project Midgard // Open Source, note "Key ceremony 6 November 2026 · second
  channel: trust-file digests and signer fingerprint",
  <https://project.veldris.dev/#/notes/record/7c92d400-5352-4260-9c06-7bc1893db01d>,
  on 2026-11-07 at 00:40 (UTC), by JB Webb-Benjamin.

## Sign-off

Each person checked the values above against the second channel and the screen of
the ceremony machine.

How this table is filled (dvalin-app `docs/KEY_CEREMONY.md`, part C2): each person
compares the Brokkr note line by line with this record and with the diff of this pull
request, then `Yes` and the date go in their row. The Security Officer edits this file
on this branch and commits the change signed; the witness's row is written only after
the witness has said in writing that the values match, and the row cites where (a
dated update on the Brokkr note, or a review comment on this pull request). Approving
the pull request is not the witness's confirmation. The same two people also sign by
hand the sign-off table of the custody register kept with the keys, which is not in
the repository.

| Role | Name | Confirmed | Date |
|---|---|---|---|
| Security Officer | JB Webb-Benjamin | | |
| Witness | Philip Snelling | | |
