//! `mg-admin` — the operator tool for an enclosure's state directory.
//!
//! It creates the directory and its custody keys, and enrols the principals who log in
//! to the gateway. Everything it writes stays under `--state-dir`; it never reaches the
//! network, and the only secrets it prints are a TOTP enrolment URI the operator asked
//! for and, under dual control, the activation code an approver hands to the new
//! principal. [RC-29] [NFR-SEC-2] [NFR-PRV-1]
//!
//! Usage:
//!   mg-admin init --state-dir <dir> [--checkpoint-mirror <dir>]
//!   mg-admin user add --state-dir <dir> --subject <name> --roles <r,..>
//!                     --clearance <band> --purposes <p,..> [--compartments <c,..>] [--totp]
//!   mg-admin user list --state-dir <dir>
//!   mg-admin import --state-dir <dir> --file <objects.jsonl>
//!   mg-admin admin add --state-dir <dir> --admin <name> --role <role> --keystore <path>
//!   mg-admin admin totp-reset --state-dir <dir> --admin <name>
//!   mg-admin enrol request|approve|reject|list --gateway <url> --admin <name> --keystore <path>
//!                [--totp <code>] ..
//!   mg-admin mesh issue --state-dir <dir> --service <name> --roles <r,..> --out <dir>
//!                       [--days <n>] [--clearance <band>] [--compartments <c,..>]
//!                       [--purposes <p,..>]
//!   mg-admin mesh renew --state-dir <dir> --service <name> --out <dir> [--within <days>]
//!                       [--days <n>] [--clearance <band>] [--compartments <c,..>]
//!                       [--purposes <p,..>]
//!   mg-admin mesh revoke --state-dir <dir> --cert <file> --out <dir,..>
//!   mg-admin mesh upgrade --state-dir <dir> --service <name> --roles <r,..> --out <dir>
//!                         [--days <n>] [--clearance <band>] [--compartments <c,..>]
//!                         [--purposes <p,..>]
//!   mg-admin audit checkpoint --state-dir <dir> --audit-pin <file> [--checkpoint-mirror <dir>]
//!   mg-admin audit checkpoint --identity-dir <dir> --service <name> --ledger <file>
//!                             --ledger-id <id> [--audit-pin <file>]
//!   mg-admin keystore migrate --keystore <file> [--state-dir <dir>]
//!   mg-admin keystore verify --keystore <file> [--state-dir <dir>]
//!   mg-admin keystore rotate-root --keystore <file> [--state-dir <dir>]
//!
//! Every command opens keystores, and every keystore's custody root is wrapped under a
//! passphrase read from `--keystore-passphrase-file <file>`, or from the file named by
//! `MIDGARD_KEYSTORE_PASSPHRASE_FILE`; with neither, the command refuses. `mesh
//! issue|renew|upgrade` take `--identity-passphrase-file` for the service's own
//! keystores, and `admin add` and `enrol` take `--passkey-passphrase-file` for the
//! administrator's passkey; each defaults to the keystore passphrase. Version 1
//! keystores, which held the root in the clear, and version 2 keystores, whose header
//! was not authenticated, are refused until `keystore migrate` converts them.
//! [VLD-ANB-MIDGARD-0007]
//!
//! The password for `user add` is read from standard input, and only from a pipe or a
//! file: std has no portable way to stop a terminal echoing it, so an interactive
//! terminal is refused rather than let the password appear on screen.

mod admin;
mod http_client;
mod mesh;

use std::collections::BTreeSet;
use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};

use attest::keystore::{
    create_private_dir, format_version, migrate, passphrase_file, write_restricted, PASSPHRASE_ENV,
    PASSPHRASE_FLAG, VERSION as KEYSTORE_VERSION,
};
use attest::{Keystore, RootWrap};
use audit_writer::{
    Action as AuditAction, Actor, AuditRecord, AuditWriter, CheckpointMirror, FileCheckpointMirror,
    FileLedger, LedgerId, RecordId, Resource as AuditResource, Timestamp,
};
use fabric::{DurableObjectStore, ObjectStore};
use http_gateway::boot::{
    checkpoint_legacy_ledger, init_audit_ledger_mirrored, mirror_audit_ledger, read_audit_pin,
    Adoption,
};
use http_gateway::enclosure::{band, level_of};
use http_gateway::import_file::parse_import;
use http_gateway::ontology_file::parse_ontology;
use http_gateway::policy_file::render_policy;
use http_gateway::state_dir::{StateDir, StateLock, AUDIT_KEY_ID, IDP_KEY_ID, LEDGER_ID, STORE_ID};
use iam::{
    Credentials, DurableDirectory, Enrollment, PasswordHasher, Pbkdf2Hasher, TotpSecret, UserRecord,
};
use kms::{Kms, KmsSealer};
use policy::{
    Action, Clearance, Compartment, Effect, PolicyError, PolicySet, Purpose, ResourceId, Role,
    Rule, RuleId, Selector,
};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{OsRng, Rng, SecretBytes, SigningKey};
use service_runtime::checkpoint_service_ledger;

const USAGE: &str = "\
mg-admin — create an enclosure's state directory and enrol its principals

USAGE:
  mg-admin init --state-dir <dir> [--checkpoint-mirror <dir>]
  mg-admin user add --state-dir <dir> --subject <name> --roles <r,..> \\
                    --clearance <band> --purposes <p,..> [--compartments <c,..>] [--totp]
  mg-admin user list --state-dir <dir>
  mg-admin import --state-dir <dir> --file <objects.jsonl>
  mg-admin admin add --state-dir <dir> --admin <name> --role <role> --keystore <path> \\
                     [--scope <subsystem>] [--days <n>]
  mg-admin admin totp-reset --state-dir <dir> --admin <name>
  mg-admin enrol request --gateway <url> --admin <name> --keystore <path> [--totp <code>] \\
                         --subject <name> --roles <r,..> --clearance <band> \\
                         --purposes <p,..> --justification <text> [--compartments <c,..>]
  mg-admin enrol approve --gateway <url> --admin <name> --keystore <path> [--totp <code>] \\
                         --id <enrolment> --subject <name>
  mg-admin enrol reject  --gateway <url> --admin <name> --keystore <path> [--totp <code>] \\
                         --id <enrolment> --reason <text>
  mg-admin enrol list    --gateway <url> --admin <name> --keystore <path> [--totp <code>]
  mg-admin mesh issue --state-dir <dir> --service <name> --roles <r,..> --out <dir> \\
                      [--days <n>] [--clearance <band>] [--compartments <c,..>] \\
                      [--purposes <p,..>]
  mg-admin mesh renew --state-dir <dir> --service <name> --out <dir> [--within <days>] \\
                      [--days <n>] [--clearance <band>] [--compartments <c,..>] \\
                      [--purposes <p,..>]
  mg-admin mesh revoke --state-dir <dir> --cert <file> --out <dir,..>
  mg-admin mesh upgrade --state-dir <dir> --service <name> --roles <r,..> --out <dir> \\
                        [--days <n>] [--clearance <band>] [--compartments <c,..>] \\
                        [--purposes <p,..>]
  mg-admin audit checkpoint --state-dir <dir> --audit-pin <file> [--checkpoint-mirror <dir>]
  mg-admin audit checkpoint --identity-dir <dir> --service <name> --ledger <file> \\
                            --ledger-id <id> [--audit-pin <file>]
  mg-admin keystore migrate --keystore <file> [--state-dir <dir>]
  mg-admin keystore verify --keystore <file> [--state-dir <dir>]
  mg-admin keystore rotate-root --keystore <file> [--state-dir <dir>]

  Every command also takes --keystore-passphrase-file <file> (default: the file named
  by MIDGARD_KEYSTORE_PASSPHRASE_FILE; with neither it refuses). mesh issue, renew and
  upgrade take --identity-passphrase-file for the service's keystores, and admin add and
  enrol take --passkey-passphrase-file for the passkey; both default to it.

COMMANDS:
  init       create <dir> with fresh issuer and audit keystores, their verifying keys
             under trust/, a starter policy.json (role operator may Read and Apply;
             role approver may Read and Approve human gates; both may read
             the audit view) and an empty ontology.json. Refuses a directory that already has anything
             in it. --checkpoint-mirror also writes the audit ledger's signed
             checkpoint to that directory, which must exist on another mount; serve
             the gateway with the same --checkpoint-mirror.
  user add   enrol a principal. The password is read from standard input and must come
             from a pipe or a file, never a terminal. --clearance is one of open,
             internal, restricted, confidential. --totp also enrols a TOTP second
             factor and prints its otpauth:// URI once. An enrolled principal is never
             replaced.
  user list  list enrolled principals with their roles, clearance and purposes.
  import     add objects and links from a JSON Lines file to the object store. Every
             record is checked against ontology.json first; the first bad line, an
             object that already exists, or a link to an object that does not refuses
             the whole file. The import is recorded in the audit ledger.
  admin add  make someone an administrator: create their post-quantum passkey in a new
             sealed keystore at --keystore (theirs to keep, never in the state
             directory), and bind a standard role — IAM-Admin, Security-Officer,
             Model-Steward, Release-Manager or Auditor — in --scope (default iam) for
             --days (default 365). Also enrols their TOTP second factor and prints its
             otpauth:// URI once. Recorded in the audit ledger.
  admin totp-reset
             give an administrator who lost their authenticator a new TOTP secret,
             printed once; the old one stops working. Recorded in the audit ledger.
  enrol      enrol a principal while the gateway runs, under dual control. `request`
             asks the gateway at --gateway, a loopback URL such as
             http://127.0.0.1:7710, to enrol them with roles and purposes the served
             policy names; no password or second factor is sent. The request waits
             until a *different* IAM administrator runs `approve`, naming the
             --subject they approve: the gateway then creates the credentials and
             prints a single-use activation code, valid for 72 hours, for the approver
             to hand to the principal out of band. The principal sends it to
             POST /api/v1/activate with the password they choose, and receives their
             TOTP second factor; until then they cannot log in. `reject` discards a
             waiting request with a --reason. `list` shows each waiting request in
             full. Each command signs a fresh challenge, issued to that
             administrator alone, with the administrator's passkey and sends a
             one-time code from their authenticator app, from --totp or typed when
             asked; each code is accepted once.
  mesh issue give a service its mesh identity in --out: its own identity and audit-signing
             keystores (secrets, for that service alone), a certificate for --roles signed
             by the enclosure's issuer, mesh.anchor, the trust anchor every service
             verifies its peers against, and mesh.crl, the signed revocation list. The
             certificate is valid for --days (default 30) and grants --clearance (a band
             as for user add, default open) with --compartments. With --purposes it
             also binds the purposes the service may open channels for, and a channel
             declared for any other is refused; without, it binds none. Never
             overwrites an existing service's identity.
  mesh renew give a service a fresh key and certificate in --out, keeping its roles
             and, unless --purposes is given, the purposes its certificate binds.
             With --within, does nothing until the certificate expires within that many
             days, so it can run from a timer. Restart the service afterwards.
  mesh revoke
             revoke the certificate in --cert: sign the next revocation list and write it
             as mesh.crl into each --out service directory. Running services pick it up
             without a restart.
  mesh upgrade
             bring a service issued by an earlier release up to date in place: reissue its
             certificate for the key it already holds (with --roles, as at issue), and
             rewrite mesh.anchor and mesh.crl in the current formats. Both keystores are
             kept, so its audit ledger and sealed state still open. A current service is
             left alone. Run it for every service directory after upgrading.
  audit checkpoint
             on upgrade: verify the governance and dual-control ledgers written before
             ledgers had checkpoints under the audit key pinned in --audit-pin, and
             record their first signed checkpoints. The gateway refuses a ledger
             without one. A ledger that already has a checkpoint is left alone, so it
             is safe to run again.
             With --identity-dir: the same for one mesh service's own ledger
             (--ledger, --ledger-id), under its audit keystore in that directory, and
             under --audit-pin too when given. Stop the service first.
             With --checkpoint-mirror: verify the ledger under the pinned key and
             write its checkpoint to the mirror directory, adopting a ledger without
             a checkpoint at the same time. Enables a mirror for an existing ledger,
             or re-seeds a lost one; a mirror ahead of the ledger is never moved back.
  keystore migrate
             convert a version 1 keystore, which held its custody root in the clear, or
             a version 2 keystore, whose header was not authenticated, to version 3,
             the root wrapped under the keystore passphrase with the whole header bound
             to it: the new file must unseal the same signing key before it replaces
             the old one. The root is kept, so destroy every other copy of the old file
             (backups, volumes, Secrets), and where a version 1 copy may have leaked,
             rotate the root (keystore rotate-root) or, where that is refused,
             re-initialise. A version 3 file is checked and left alone. With
             --state-dir the migration is recorded in that enclosure's audit ledger.
  keystore verify
             unlock a keystore with the passphrase and print its key id, algorithm and
             format version. With --state-dir the check is recorded in the audit ledger.
  keystore rotate-root
             give a version 3 keystore a new custody root, keeping its signing key and
             verifying key: the new file must unseal the same signing key under a
             different root before it replaces the old one. Only for keystores whose
             root seals nothing but their own key (release, admission, model-signing,
             a mesh service's identity keystore, an administrator's passkey); a state
             directory's keystores and a mesh service's audit keystore are refused,
             since their root seals the enclosure's data. With --state-dir the rotation
             is recorded in that enclosure's audit ledger.

init, user, import, admin add and admin totp-reset need the state directory's lock, which a
running gateway holds: stop it first. The enrol commands talk to the running gateway
instead.
";

/// The shortest password `user add` accepts.
const MIN_PASSWORD_LEN: usize = 12;
/// TOTP shared-secret length in bytes (RFC 4226: at least 160 bits).
const TOTP_SECRET_LEN: usize = 20;
/// The issuer an authenticator app shows beside the enrolment.
const TOTP_ISSUER: &str = "Midgard";
/// The ontology `init` writes: valid, and declaring nothing until the operator does.
const STARTER_ONTOLOGY: &str =
    "{\"version\":1,\"objectTypes\":[],\"linkTypes\":[],\"actions\":[]}\n";

/// A usage or execution failure, so `main` decides the exit code once.
enum Failure {
    Usage(String),
    Run(String),
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    match run(&argv) {
        Ok(message) => println!("{message}"),
        Err(Failure::Usage(e)) => {
            eprintln!("mg-admin: {e}\n\n{USAGE}");
            exit(2);
        }
        Err(Failure::Run(e)) => {
            eprintln!("mg-admin: {e}");
            exit(1);
        }
    }
}

fn run(argv: &[String]) -> Result<String, Failure> {
    let words: Vec<&str> = argv.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["init", rest @ ..] => init(&options(rest, &["state-dir", "checkpoint-mirror"], &[])?),
        ["user", "add", rest @ ..] => user_add(&options(
            rest,
            &[
                "state-dir",
                "subject",
                "roles",
                "clearance",
                "purposes",
                "compartments",
            ],
            &["totp"],
        )?),
        ["user", "list", rest @ ..] => user_list(&options(rest, &["state-dir"], &[])?),
        ["import", rest @ ..] => import_data(&options(rest, &["state-dir", "file"], &[])?),
        ["admin", "add", rest @ ..] => admin::admin_add(&options(
            rest,
            &["state-dir", "admin", "role", "scope", "days", "keystore"],
            &[],
        )?),
        ["admin", "totp-reset", rest @ ..] => {
            admin::admin_totp_reset(&options(rest, &["state-dir", "admin"], &[])?)
        }
        ["enrol", "request", rest @ ..] => admin::enrol_request(&options(
            rest,
            &[
                "gateway",
                "admin",
                "keystore",
                "totp",
                "subject",
                "roles",
                "clearance",
                "compartments",
                "purposes",
                "justification",
            ],
            &[],
        )?),
        ["enrol", "approve", rest @ ..] => admin::enrol_approve(&options(
            rest,
            &["gateway", "admin", "keystore", "totp", "id", "subject"],
            &[],
        )?),
        ["enrol", "reject", rest @ ..] => admin::enrol_reject(&options(
            rest,
            &["gateway", "admin", "keystore", "totp", "id", "reason"],
            &[],
        )?),
        ["enrol", "list", rest @ ..] => admin::enrol_list(&options(
            rest,
            &["gateway", "admin", "keystore", "totp"],
            &[],
        )?),
        ["mesh", "issue", rest @ ..] => mesh::mesh_issue(&options(
            rest,
            &[
                "state-dir",
                "service",
                "roles",
                "out",
                "days",
                "clearance",
                "compartments",
                "purposes",
            ],
            &[],
        )?),
        ["mesh", "renew", rest @ ..] => mesh::mesh_renew(&options(
            rest,
            &[
                "state-dir",
                "service",
                "out",
                "within",
                "days",
                "clearance",
                "compartments",
                "purposes",
            ],
            &[],
        )?),
        ["mesh", "upgrade", rest @ ..] => mesh::mesh_upgrade(&options(
            rest,
            &[
                "state-dir",
                "service",
                "roles",
                "out",
                "days",
                "clearance",
                "compartments",
                "purposes",
            ],
            &[],
        )?),
        ["mesh", "revoke", rest @ ..] => {
            mesh::mesh_revoke(&options(rest, &["state-dir", "cert", "out"], &[])?)
        }
        ["audit", "checkpoint", rest @ ..] => audit_checkpoint(&options(
            rest,
            &[
                "state-dir",
                "audit-pin",
                "checkpoint-mirror",
                "identity-dir",
                "service",
                "ledger",
                "ledger-id",
            ],
            &[],
        )?),
        ["keystore", "migrate", rest @ ..] => {
            keystore_migrate(&options(rest, &["keystore", "state-dir"], &[])?)
        }
        ["keystore", "verify", rest @ ..] => {
            keystore_verify(&options(rest, &["keystore", "state-dir"], &[])?)
        }
        ["keystore", "rotate-root", rest @ ..] => {
            keystore_rotate_root(&options(rest, &["keystore", "state-dir"], &[])?)
        }
        [] => Err(Failure::Usage("no command given".into())),
        _ => Err(Failure::Usage(format!(
            "unknown command '{}'",
            words.join(" ")
        ))),
    }
}

// ---- commands --------------------------------------------------------------------

fn init(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let root = state.root().display().to_string();
    if state.root().exists() {
        let mut entries = std::fs::read_dir(state.root())
            .map_err(|e| Failure::Run(format!("cannot read {root}: {e}")))?;
        if entries.next().is_some() {
            return Err(Failure::Run(format!(
                "{root} is not empty; init never overwrites an existing enclosure"
            )));
        }
    }
    // The checkpoint mirror must be there, and not already vouch for another ledger,
    // before anything is written. [MG-SEC-05]
    if let Some(dir) = o.get("checkpoint-mirror") {
        let mirror = FileCheckpointMirror::new(dir).map_err(|e| Failure::Run(e.to_string()))?;
        let held = mirror
            .latest(&LedgerId(LEDGER_ID.into()))
            .map_err(|e| Failure::Run(e.to_string()))?;
        if held.is_some() {
            return Err(Failure::Run(format!(
                "the checkpoint mirror {dir} already holds an audit ledger checkpoint; give \
                 each enclosure a mirror directory of its own"
            )));
        }
    }
    let passphrase = passphrase(o)?;
    let wrap = RootWrap::Passphrase(&passphrase);
    for dir in [state.root().to_path_buf(), state.trust()] {
        std::fs::create_dir_all(&dir)
            .map_err(|e| Failure::Run(format!("cannot create {}: {e}", dir.display())))?;
    }
    // The keys directory is owner-only from the moment it exists. [VLD-ANB-MIDGARD-0007]
    create_private_dir(&state.keys()).map_err(|e| Failure::Run(e.to_string()))?;

    let mut rng = OsRng;
    let idp = Keystore::generate(IDP_KEY_ID, iam::IDP_SIG_ALG, &mut rng)
        .map_err(|e| Failure::Run(format!("cannot generate the issuer key: {e}")))?;
    let audit = Keystore::generate(AUDIT_KEY_ID, governance::GOV_SIG_ALG, &mut rng)
        .map_err(|e| Failure::Run(format!("cannot generate the audit key: {e}")))?;
    for (keystore, path) in [
        (&idp, state.idp_keystore()),
        (&audit, state.audit_keystore()),
    ] {
        keystore
            .write(&path, &wrap, &mut rng)
            .map_err(|e| Failure::Run(format!("cannot write {}: {e}", path.display())))?;
    }
    write_file(&state.idp_trust_anchor(), &idp.export_verifying_key())?;
    write_file(&state.audit_trust_anchor(), &audit.export_verifying_key())?;
    // The governance ledger exists from the start, vouched for by a signed empty
    // checkpoint, so the gateway can refuse one that later goes missing; with a
    // mirror, a copy of it is kept off the server too. [MG-SEC-05]
    init_audit_ledger_mirrored(&state, o.get("checkpoint-mirror").map(Path::new), &wrap)
        .map_err(Failure::Run)?;

    let policy = starter_policy()
        .map_err(|e| Failure::Run(format!("cannot build the starter policy: {e}")))?;
    let text = render_policy(&policy)
        .map_err(|e| Failure::Run(format!("cannot render the starter policy: {e}")))?;
    write_file(&state.policy(), &format!("{text}\n"))?;
    write_file(&state.ontology(), STARTER_ONTOLOGY)?;

    Ok(format!(
        "initialised {root}\n\
         \x20 keys/        issuer and audit keystores, roots wrapped under the keystore\n\
         \x20              passphrase; keep the passphrase apart from this directory\n\
         \x20              and its backups: it is the recovery secret\n\
         \x20 trust/       their verifying keys, safe to distribute; keep a copy of\n\
         \x20              trust/audit.pub outside {root} for http-gateway --audit-pin\n\
         \x20 governance.ledger  the audit ledger, vouched for by a signed checkpoint\n\
         \x20 policy.json  starter rules: operator may Read and Apply, approver may Approve\n\
         \x20 ontology.json  empty: declare object types, link types and actions here\n\
         next: printf '%s\\n' \"$PASSWORD\" | mg-admin user add --state-dir {root} \\\n\
         \x20       --subject <name> --roles operator --clearance internal --purposes operations"
    ))
}

/// Adopt a governance ledger written before checkpoints: verified under the pinned
/// audit key, then vouched for by its first signed checkpoint. With
/// `--checkpoint-mirror`, verify the ledger and (re-)seed its checkpoint mirror.
/// [MG-SEC-05]
fn audit_checkpoint(o: &Options) -> Result<String, Failure> {
    if o.get("identity-dir").is_some() {
        return service_ledger_checkpoint(o);
    }
    let state = StateDir::new(o.require("state-dir")?);
    let pin = read_audit_pin(Path::new(o.require("audit-pin")?)).map_err(Failure::Run)?;
    let passphrase = passphrase(o)?;
    let wrap = RootWrap::Passphrase(&passphrase);
    let _lock = StateLock::acquire(&state, "mg-admin audit checkpoint").map_err(Failure::Run)?;
    if let Some(mirror) = o.get("checkpoint-mirror") {
        let entries =
            mirror_audit_ledger(&state, &pin, Path::new(mirror), &wrap).map_err(Failure::Run)?;
        return Ok(format!(
            "the audit ledger verifies under the pinned key; its checkpoint, mirrored to              {mirror}, vouches for {entries} entries"
        ));
    }
    let done = checkpoint_legacy_ledger(&state, &pin, &wrap).map_err(Failure::Run)?;
    Ok(done
        .iter()
        .map(|(name, adoption)| adoption_line(name, adoption))
        .collect::<Vec<_>>()
        .join(
            "
",
        ))
}

/// `audit checkpoint --identity-dir DIR --service NAME --ledger FILE --ledger-id ID`:
/// adopt a mesh service's own ledger, written before ledgers had checkpoints, under
/// its audit keystore (and `--audit-pin`, when given). Run it with the service
/// stopped. [MG-SEC-05]
fn service_ledger_checkpoint(o: &Options) -> Result<String, Failure> {
    let identity_dir = Path::new(o.require("identity-dir")?);
    let service = o.require("service")?;
    let ledger = Path::new(o.require("ledger")?);
    let ledger_id = o.require("ledger-id")?;
    if o.get("state-dir").is_some() || o.get("checkpoint-mirror").is_some() {
        return Err(Failure::Usage(
            "--identity-dir adopts one service's ledger; it takes no --state-dir or              --checkpoint-mirror"
                .into(),
        ));
    }
    let pin = o
        .get("audit-pin")
        .map(|p| read_audit_pin(Path::new(p)))
        .transpose()
        .map_err(Failure::Run)?;
    let passphrase = passphrase(o)?;
    let adoption = checkpoint_service_ledger(
        identity_dir,
        service,
        ledger,
        ledger_id,
        pin.as_ref(),
        &RootWrap::Passphrase(&passphrase),
    )
    .map_err(Failure::Run)?;
    Ok(adoption_line(&format!("{service} ledger"), &adoption))
}

/// `keystore migrate --keystore FILE`: convert a version 1 keystore (root in the
/// clear) or a version 2 keystore (header not authenticated) to version 3 under the
/// keystore passphrase. The new file is written beside the old one as `FILE.v3`,
/// owner-only, read back from disk with the passphrase and checked to unseal the same
/// signing key, then renamed over `FILE`. A version 3 file is only checked, so the
/// command is safe to repeat. [VLD-ANB-MIDGARD-0007]
fn keystore_migrate(o: &Options) -> Result<String, Failure> {
    let path = PathBuf::from(o.require("keystore")?);
    let passphrase = passphrase(o)?;
    let wrap = RootWrap::Passphrase(&passphrase);
    let bytes = std::fs::read(&path)
        .map_err(|e| Failure::Run(format!("cannot read {}: {e}", path.display())))?;
    let from = match format_version(&bytes) {
        Some(KEYSTORE_VERSION) => {
            let keystore = Keystore::decode(&bytes, &wrap)
                .map_err(|e| Failure::Run(format!("{}: {e}", path.display())))?;
            return Ok(format!(
                "{} ({}) is already keystore version {KEYSTORE_VERSION} and unlocks; nothing \
                 changed",
                path.display(),
                keystore.key_id
            ));
        }
        Some(v @ (1 | 2)) => v,
        _ => {
            return Err(Failure::Run(format!(
                "{} is not a keystore this tool can migrate",
                path.display()
            )))
        }
    };
    // The original signing key, unsealed under the root the old file holds.
    let migrated = migrate(&bytes, &wrap, &mut OsRng)
        .map_err(|e| Failure::Run(format!("cannot migrate {}: {e}", path.display())))?;
    let expected = Keystore::decode(&migrated, &wrap)
        .and_then(|k| k.signing_root())
        .map_err(|e| Failure::Run(format!("cannot migrate {}: {e}", path.display())))?;
    let keystore = replace_keystore(
        &path,
        &format!("v{KEYSTORE_VERSION}"),
        &migrated,
        &wrap,
        &expected,
        |_| true,
        o.get("state-dir").map(|state| AdminEvent {
            state: StateDir::new(state),
            command: "mg-admin keystore migrate",
            action: "MIGRATE keystore",
            resource: format!(
                "keystore:{}:v{from}-to-v{KEYSTORE_VERSION}",
                kms::SigningRoot::key_id(&expected)
            ),
        }),
    )?;
    let old_copies = if from == 1 {
        "it holds the root in clear"
    } else {
        "its header is not authenticated, and no reader opens it"
    };
    Ok(format!(
        "migrated {} ({}, {}) from keystore version {from} to keystore version \
         {KEYSTORE_VERSION}; destroy every other copy of the version {from} file (backups, \
         volumes, Secrets): {old_copies}",
        path.display(),
        keystore.key_id,
        keystore.alg.as_str()
    ))
}

/// An event to record in an enclosure's audit ledger once a keystore is staged.
struct AdminEvent {
    state: StateDir,
    command: &'static str,
    action: &'static str,
    resource: String,
}

/// Put `bytes`, a replacement for the keystore at `path`, in its place: written beside
/// it as `FILE.<suffix>`, owner-only, read back from disk under `wrap` and checked to
/// unseal `expected`'s signing key under the same verifying key (and to pass `also`),
/// recorded in the audit ledger when `event` is given, then renamed over `path`. On any
/// failure the staged file is removed and `path` is unchanged. Returns the keystore as
/// read back. [VLD-ANB-MIDGARD-0007]
fn replace_keystore(
    path: &Path,
    suffix: &str,
    bytes: &[u8],
    wrap: &RootWrap,
    expected: &kms::SoftwareSigningRoot,
    also: impl Fn(&Keystore) -> bool,
    event: Option<AdminEvent>,
) -> Result<Keystore, Failure> {
    use kms::SigningRoot as _;
    let mut staged = path.as_os_str().to_os_string();
    staged.push(format!(".{suffix}"));
    let staged = PathBuf::from(staged);
    write_restricted(&staged, bytes).map_err(|e| Failure::Run(e.to_string()))?;
    // Prove the file on disk, not just the bytes in memory, before it takes the name.
    let reread = Keystore::read(&staged, wrap).and_then(|k| {
        let root = k.signing_root()?;
        Ok((k, root))
    });
    let keystore = match reread {
        Ok((keystore, root))
            if keystore.verifying_key == *expected.verifying_key()
                && pqc_agility::ct_eq(
                    root.expose_signing_key().expose(),
                    expected.expose_signing_key().expose(),
                )
                && also(&keystore) =>
        {
            keystore
        }
        _ => {
            let _ = std::fs::remove_file(&staged);
            return Err(Failure::Run(format!(
                "{} does not read back to the original signing key; {} is unchanged",
                staged.display(),
                path.display()
            )));
        }
    };
    if let Some(event) = event {
        let recorded = StateLock::acquire(&event.state, event.command)
            .map_err(Failure::Run)
            .and_then(|_lock| {
                record_admin_event(&event.state, wrap, event.action, &event.resource)
            });
        if let Err(e) = recorded {
            let _ = std::fs::remove_file(&staged);
            return Err(e);
        }
    }
    std::fs::rename(&staged, path)
        .map_err(|e| Failure::Run(format!("cannot replace {}: {e}", path.display())))?;
    Ok(keystore)
}

/// `keystore rotate-root --keystore FILE`: give a version 3 keystore a new custody
/// root, keeping its signing key and verifying key, as `keystore migrate` replaces a
/// file: staged as `FILE.rotated`, read back and checked to unseal the same signing
/// key under a different root, recorded with --state-dir, then renamed over `FILE`.
///
/// A keystore's root seals its own signing key, and for some keystores data too,
/// through `kms::KmsSealer`; that data would never open again under a new root. Which
/// do: the state directory's issuer keystore seals the identity directory, revocations
/// and the administrators' journals, its audit keystore the governance ledger and the
/// object store, and a mesh service's audit keystore its ledger and its own stores
/// (`ServiceIdentity::data_sealer`). Which do not: release, admission and
/// model-signing keystores (mg-sign, mg-register, mg-bundle and mg-model only sign
/// with them), a mesh service's identity keystore (it only signs handshakes; the
/// service's data is under its audit keystore), and an administrator's passkey. So
/// the rule ([`rotation_refusal`]) refuses a file in a state directory's `keys/`, a
/// file named `*.audit.keystore`, and, wherever the file lies, a keystore whose key id
/// is the state directory's issuer or audit key id or a mesh audit key id
/// (`<service>-audit`). [VLD-ANB-MIDGARD-0007]
fn keystore_rotate_root(o: &Options) -> Result<String, Failure> {
    let path = PathBuf::from(o.require("keystore")?);
    let passphrase = passphrase(o)?;
    let wrap = RootWrap::Passphrase(&passphrase);
    let keystore = Keystore::read(&path, &wrap).map_err(|e| Failure::Run(e.to_string()))?;
    if let Some(reason) = rotation_refusal(&path, &keystore) {
        return Err(Failure::Run(format!(
            "{} {reason}, and a new root would leave that data sealed under the old one; \
             re-initialise the enclosure instead",
            path.display()
        )));
    }
    let expected = keystore
        .signing_root()
        .map_err(|e| Failure::Run(format!("{}: {e}", path.display())))?;
    let rotated = keystore
        .rotate_root(&mut OsRng)
        .and_then(|k| k.encode(&wrap, &mut OsRng))
        .map_err(|e| Failure::Run(format!("cannot rotate {}: {e}", path.display())))?;
    let old_root = keystore.root_key_id().to_string();
    let after = replace_keystore(
        &path,
        "rotated",
        &rotated,
        &wrap,
        &expected,
        |k| k.root_key_id() != old_root,
        o.get("state-dir").map(|state| AdminEvent {
            state: StateDir::new(state),
            command: "mg-admin keystore rotate-root",
            action: "ROTATE keystore root",
            resource: format!("keystore:{}:root:{old_root}", keystore.key_id),
        }),
    )?;
    Ok(format!(
        "rotated the custody root of {} ({}, {}) from {old_root} to {}; the signing key and \
         verifying key are unchanged. Destroy every other copy of the old file (backups, \
         volumes, Secrets): it holds the old root",
        path.display(),
        after.key_id,
        after.alg.as_str(),
        after.root_key_id()
    ))
}

/// Why `keystore rotate-root` must not rotate `keystore`, read from `path`: its root
/// seals an enclosure's data. `None` when it seals nothing but its own signing key.
/// See [`keystore_rotate_root`] for which are which. [VLD-ANB-MIDGARD-0007]
fn rotation_refusal(path: &Path, keystore: &Keystore) -> Option<String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    if name
        .as_deref()
        .is_some_and(|n| n.ends_with(".audit.keystore"))
    {
        return Some(
            "is a mesh service's audit keystore, whose root seals its ledger and stores".into(),
        );
    }
    if [IDP_KEY_ID, AUDIT_KEY_ID].contains(&keystore.key_id.as_str()) {
        return Some(format!(
            "holds the state directory's {} key, whose root seals the enclosure's data",
            keystore.key_id
        ));
    }
    if keystore.key_id.ends_with("-audit") {
        return Some(format!(
            "holds a mesh service's audit key ({}), whose root seals its ledger and stores",
            keystore.key_id
        ));
    }
    // A file in a state directory's keys/: the directory beside it has the state
    // directory's other contents.
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let keys = path.parent()?;
    let root = keys.parent()?;
    let state = StateDir::new(root);
    let in_state_dir = keys.file_name().is_some_and(|n| n == "keys")
        && (state.trust().is_dir() || state.directory().exists() || state.ledger().exists());
    in_state_dir.then(|| {
        format!(
            "is in the state directory {}, whose keystores' roots seal the enclosure's data",
            root.display()
        )
    })
}

/// `keystore verify --keystore FILE`: unlock it with the passphrase, unseal its
/// signing key, and say what it holds. [VLD-ANB-MIDGARD-0007]
fn keystore_verify(o: &Options) -> Result<String, Failure> {
    let path = PathBuf::from(o.require("keystore")?);
    let passphrase = passphrase(o)?;
    let wrap = RootWrap::Passphrase(&passphrase);
    let keystore = Keystore::read(&path, &wrap).map_err(|e| Failure::Run(e.to_string()))?;
    keystore
        .signing_root()
        .map_err(|e| Failure::Run(format!("{}: {e}", path.display())))?;
    if let Some(state) = o.get("state-dir") {
        let state = StateDir::new(state);
        let _lock = StateLock::acquire(&state, "mg-admin keystore verify").map_err(Failure::Run)?;
        record_admin_event(
            &state,
            &wrap,
            "VERIFY keystore",
            &format!("keystore:{}", keystore.key_id),
        )?;
    }
    Ok(format!(
        "{}\n  key id: {}\n  algorithm: {}\n  format version: {KEYSTORE_VERSION}\n  unlocks: yes",
        path.display(),
        keystore.key_id,
        keystore.alg.as_str()
    ))
}

fn adoption_line(name: &str, adoption: &Adoption) -> String {
    match adoption {
        Adoption::Absent => format!("the {name} does not exist yet; nothing to do"),
        Adoption::AlreadyCheckpointed => {
            format!("the {name} already has a checkpoint; nothing changed")
        }
        Adoption::Adopted { entries } => {
            format!("the {name} verifies; its checkpoint now vouches for {entries} entries")
        }
    }
}

fn user_add(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let subject = o.require("subject")?;
    if subject.trim() != subject || subject.chars().any(char::is_whitespace) {
        return Err(Failure::Usage(format!(
            "--subject must not contain whitespace, not '{subject}'"
        )));
    }
    let roles: BTreeSet<Role> = list(o.require("roles")?).into_iter().map(Role).collect();
    let purposes: BTreeSet<Purpose> = list(o.require("purposes")?)
        .into_iter()
        .map(Purpose)
        .collect();
    if roles.is_empty() || purposes.is_empty() {
        return Err(Failure::Usage(
            "--roles and --purposes each need at least one value".into(),
        ));
    }
    let clearance_band = o.require("clearance")?;
    let level = level_of(clearance_band).ok_or_else(|| {
        Failure::Usage(format!(
            "--clearance must be open, internal, restricted or confidential, not '{clearance_band}'"
        ))
    })?;
    let compartments: BTreeSet<Compartment> = o
        .get("compartments")
        .map(list)
        .unwrap_or_default()
        .into_iter()
        .map(Compartment)
        .collect();
    let password = read_password()?;
    let passphrase = passphrase(o)?;
    let wrap = RootWrap::Passphrase(&passphrase);

    let _lock = StateLock::acquire(&state, "mg-admin user add").map_err(Failure::Run)?;
    let mut directory = open_directory(&state, &wrap)?;
    if directory.get(subject).is_some() {
        return Err(Failure::Run(format!(
            "'{subject}' is already enrolled; enrolment never replaces a principal"
        )));
    }

    let totp = o.flag("totp").then(|| {
        let mut secret = vec![0u8; TOTP_SECRET_LEN];
        OsRng.fill(&mut secret);
        TotpSecret::new(secret)
    });
    let enrolment_uri = totp
        .as_ref()
        .map(|secret| secret.provisioning_uri(TOTP_ISSUER, subject));

    // Audit first: an enrolment the ledger cannot record does not happen.
    record_admin_event(
        &state,
        &wrap,
        "ENROLL principal",
        &format!("principal:{subject}"),
    )?;
    directory
        .enroll(
            subject,
            Enrollment {
                user: UserRecord {
                    roles,
                    clearance: Clearance {
                        level,
                        compartments,
                    },
                    purposes,
                },
                credentials: Credentials {
                    password_hash: Pbkdf2Hasher::production().hash(password.as_bytes()),
                    totp,
                    ..Credentials::default()
                },
            },
        )
        .map_err(|e| Failure::Run(format!("cannot record the enrolment: {e}")))?;

    let mut message = format!("enrolled '{subject}' (clearance {clearance_band})");
    if let Some(uri) = enrolment_uri {
        message.push_str(
            "\nTOTP second factor — add this to the principal's authenticator app, then \
             discard it; it is not shown again:\n  ",
        );
        message.push_str(&uri);
    }
    Ok(message)
}

fn user_list(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let passphrase = passphrase(o)?;
    let _lock = StateLock::acquire(&state, "mg-admin user list").map_err(Failure::Run)?;
    let directory = open_directory(&state, &RootWrap::Passphrase(&passphrase))?;

    let mut message = format!("{} principal(s)", directory.len());
    for subject in directory.subjects() {
        let Some(entry) = directory.get(subject) else {
            continue;
        };
        let user = &entry.user;
        let roles: Vec<&str> = user.roles.iter().map(|r| r.0.as_str()).collect();
        let purposes: Vec<&str> = user.purposes.iter().map(|p| p.0.as_str()).collect();
        let compartments: Vec<&str> = user
            .clearance
            .compartments
            .iter()
            .map(|c| c.0.as_str())
            .collect();
        let clearance = if compartments.is_empty() {
            band(user.clearance.level).to_string()
        } else {
            format!("{}/{}", band(user.clearance.level), compartments.join("+"))
        };
        message.push_str(&format!(
            "\n  {subject}  roles={}  clearance={clearance}  purposes={}  second-factor={}",
            roles.join(","),
            purposes.join(","),
            if entry.credentials.has_mfa() {
                "yes"
            } else {
                "no"
            },
        ));
    }
    Ok(message)
}

fn import_data(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let file = o.require("file")?;
    let data = std::fs::read_to_string(file)
        .map_err(|e| Failure::Run(format!("cannot read {file}: {e}")))?;

    let _lock = StateLock::acquire(&state, "mg-admin import").map_err(Failure::Run)?;
    let ontology_path = state.ontology();
    let ontology_text = std::fs::read_to_string(&ontology_path)
        .map_err(|e| Failure::Run(format!("cannot read {}: {e}", ontology_path.display())))?;
    let ontology = parse_ontology(&ontology_text)
        .map_err(|e| Failure::Run(format!("{}: {e}", ontology_path.display())))?;
    let import = parse_import(&data, &ontology).map_err(Failure::Run)?;
    if import.objects.is_empty() && import.links.is_empty() {
        return Err(Failure::Run(format!("{file} holds no objects or links")));
    }

    let passphrase = passphrase(o)?;
    let wrap = RootWrap::Passphrase(&passphrase);
    let keystore = Keystore::read(&state.audit_keystore(), &wrap)
        .map_err(|e| Failure::Run(format!("cannot open the audit keystore: {e}")))?;
    let sealer = KmsSealer::new(Kms::new(fips_provider(), keystore.trust_root()));
    let mut store = DurableObjectStore::open(state.store(), Box::new(sealer), STORE_ID)
        .map_err(|e| Failure::Run(format!("cannot open the object store: {e}")))?;
    import
        .check_against(&|id| store.get_object(id).is_some())
        .map_err(Failure::Run)?;

    // Audit first: an import the ledger cannot record does not happen.
    let (objects, links) = (import.objects.len(), import.links.len());
    record_admin_event(
        &state,
        &wrap,
        "IMPORT data",
        &format!("import:{objects}-objects:{links}-links"),
    )?;
    let stopped = |e: fabric::FabricError| {
        Failure::Run(format!(
            "the import stopped part-way: the ledger records it, but not everything was \
             written: {e}"
        ))
    };
    for object in import.objects {
        store.put_object_durable(object).map_err(stopped)?;
    }
    for link in import.links {
        store.put_link_durable(link).map_err(stopped)?;
    }
    Ok(format!(
        "imported {objects} object(s) and {links} link(s) into {}",
        state.root().display()
    ))
}

// ---- helpers -----------------------------------------------------------------------

/// The starter rules `init` writes: role `operator` may Read and Apply, and role
/// `approver` may Read and Approve the human gates a gated action needs. Both may read
/// the audit ledger view (`ReadAudit` on `audit-ledger`), and the operator also sees
/// other principals' login rows in it (`audit-ledger:sessions`). Mandatory access
/// control and the purpose gate still apply before any of them.
/// [VLD-ANB-MIDGARD-0031]
fn starter_policy() -> Result<PolicySet, PolicyError> {
    let rule = |id: &str, role: &str, action: &str| Rule {
        id: RuleId(id.into()),
        role: Role(role.into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    };
    let audit = |id: &str, role: &str, resource: &str| Rule {
        selector: Selector::Resource(ResourceId(resource.into())),
        ..rule(id, role, "ReadAudit")
    };
    PolicySet::new(vec![
        rule("operator-read", "operator", "Read"),
        rule("operator-apply", "operator", "Apply"),
        rule("approver-read", "approver", "Read"),
        rule("approver-approve", "approver", "Approve"),
        audit("operator-audit", "operator", "audit-ledger"),
        audit(
            "operator-audit-sessions",
            "operator",
            "audit-ledger:sessions",
        ),
        audit("approver-audit", "approver", "audit-ledger"),
    ])
}

/// The identity directory, sealed under the issuer keystore's custody root.
fn open_directory(state: &StateDir, wrap: &RootWrap) -> Result<DurableDirectory, Failure> {
    let keystore = Keystore::read(&state.idp_keystore(), wrap).map_err(|e| {
        Failure::Run(format!(
            "cannot open the issuer keystore (has `mg-admin init` been run?): {e}"
        ))
    })?;
    let sealer = KmsSealer::new(Kms::new(fips_provider(), keystore.trust_root()));
    DurableDirectory::open(state.directory(), Box::new(sealer))
        .map_err(|e| Failure::Run(format!("cannot open the identity directory: {e}")))
}

/// Append an administrative event — an enrolment, an import — to the governance audit
/// ledger: the same PQ-signed chain the gateway writes every access decision to, under
/// the same key and ledger id, so it appears in the audit view beside them.
/// [AC-M-4] [RC-29]
fn record_admin_event(
    state: &StateDir,
    wrap: &RootWrap,
    action: &str,
    resource: &str,
) -> Result<u64, Failure> {
    let keystore = Keystore::read(&state.audit_keystore(), wrap)
        .map_err(|e| Failure::Run(format!("cannot open the audit keystore: {e}")))?;
    let sealer = KmsSealer::new(Kms::new(fips_provider(), keystore.trust_root()));
    let verifying_key = keystore.verifying_key.clone();
    let alg = keystore.alg;
    let root = keystore
        .into_signing_root()
        .map_err(|e| Failure::Run(format!("cannot unseal the audit key: {e}")))?;
    // The writer owns its key, and a signing key is deliberately not cloneable.
    let signing_key = SigningKey::new(
        alg,
        SecretBytes::new(root.expose_signing_key().expose().to_vec()),
    );

    let ledger = FileLedger::open(state.ledger(), Box::new(sealer), LedgerId(LEDGER_ID.into()))
        .map_err(|e| Failure::Run(format!("cannot open the audit ledger: {e}")))?;
    let mut writer = AuditWriter::new(
        ledger,
        Box::new(FipsSig::new(alg)),
        signing_key,
        verifying_key,
        LedgerId(LEDGER_ID.into()),
    );

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // A partial final frame a crash left was cut off when the ledger opened: record
    // the repair first. [VLD-ANB-MIDGARD-0040]
    writer
        .record_recovery(&now.to_string())
        .map_err(|e| Failure::Run(format!("cannot record the audit ledger repair: {e}")))?;
    // Shaped like the governance spine's own records, so the audit view reads it the
    // same way.
    let id = RecordId(format!("AUD-{}", writer.entries().len()));
    writer
        .append(AuditRecord {
            id,
            actor: Actor("mg-admin".into()),
            action: AuditAction(action.into()),
            resource: AuditResource(resource.into()),
            ts: Timestamp(now.to_string()),
            context: Default::default(),
        })
        .map_err(|e| Failure::Run(format!("cannot record {action} in the audit ledger: {e}")))
}

/// The keystore passphrase: from `--keystore-passphrase-file`, else the file
/// `MIDGARD_KEYSTORE_PASSPHRASE_FILE` names; with neither, a usage error naming both.
/// [VLD-ANB-MIDGARD-0007]
pub(crate) fn passphrase(o: &Options) -> Result<SecretBytes, Failure> {
    let path = passphrase_file(o.get(PASSPHRASE_FLAG), std::env::var(PASSPHRASE_ENV).ok())
        .map_err(Failure::Usage)?;
    RootWrap::passphrase_from_file(&path).map_err(|e| Failure::Run(e.to_string()))
}

/// A passphrase with its own option (`--identity-passphrase-file`,
/// `--passkey-passphrase-file`), falling back to [`passphrase`].
pub(crate) fn passphrase_or_default(o: &Options, option: &str) -> Result<SecretBytes, Failure> {
    match o.get(option) {
        Some(path) => {
            RootWrap::passphrase_from_file(Path::new(path)).map_err(|e| Failure::Run(e.to_string()))
        }
        None => passphrase(o),
    }
}

/// One password line from standard input, refused from an echoing terminal.
fn read_password() -> Result<String, Failure> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(Failure::Usage(
            "the password is read from standard input, which here is a terminal that would \
             echo it; pipe it in instead"
                .into(),
        ));
    }
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .map_err(|e| Failure::Run(format!("cannot read the password: {e}")))?;
    let password = line.trim_end_matches(['\r', '\n']).to_string();
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(Failure::Usage(format!(
            "the password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    Ok(password)
}

fn write_file(path: &Path, contents: &str) -> Result<(), Failure> {
    std::fs::write(path, contents)
        .map_err(|e| Failure::Run(format!("cannot write {}: {e}", path.display())))
}

/// A comma-separated list, blanks dropped.
fn list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

// ---- argument parsing ----------------------------------------------------------------

/// Passphrase options every command accepts. [VLD-ANB-MIDGARD-0007]
const PASSPHRASE_OPTIONS: [&str; 3] = [
    PASSPHRASE_FLAG,
    "identity-passphrase-file",
    "passkey-passphrase-file",
];

/// Parsed `--key value` options and bare `--flag`s.
struct Options {
    values: Vec<(String, String)>,
    flags: BTreeSet<String>,
}

impl Options {
    fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn require(&self, key: &str) -> Result<&str, Failure> {
        self.get(key)
            .ok_or_else(|| Failure::Usage(format!("--{key} is required")))
    }

    fn flag(&self, key: &str) -> bool {
        self.flags.contains(key)
    }
}

/// Parse options, rejecting anything unrecognised or repeated: a mistyped option is a
/// usage error, never silently ignored.
fn options(argv: &[&str], valued: &[&str], flags: &[&str]) -> Result<Options, Failure> {
    let mut parsed = Options {
        values: Vec::new(),
        flags: BTreeSet::new(),
    };
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i];
        let key = arg
            .strip_prefix("--")
            .ok_or_else(|| Failure::Usage(format!("unexpected argument '{arg}'")))?;
        if flags.contains(&key) {
            if !parsed.flags.insert(key.to_string()) {
                return Err(Failure::Usage(format!("--{key} given twice")));
            }
            i += 1;
            continue;
        }
        if !valued.contains(&key) && !PASSPHRASE_OPTIONS.contains(&key) {
            return Err(Failure::Usage(format!("unknown option '--{key}'")));
        }
        let value = argv
            .get(i + 1)
            .filter(|v| !v.starts_with("--"))
            .ok_or_else(|| Failure::Usage(format!("--{key} requires a value")))?;
        if parsed.get(key).is_some() {
            return Err(Failure::Usage(format!("--{key} given twice")));
        }
        parsed.values.push((key.to_string(), (*value).to_string()));
        i += 2;
    }
    Ok(parsed)
}
