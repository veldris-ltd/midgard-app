//! `mg-admin` — the operator tool for an enclosure's state directory.
//!
//! It creates the directory and its custody keys, and enrols the principals who log in
//! to the gateway. Everything it writes stays under `--state-dir`; it never reaches the
//! network, and the only secret it prints is a TOTP enrolment URI the operator asked
//! for. [RC-29] [NFR-SEC-2] [NFR-PRV-1]
//!
//! Usage:
//!   mg-admin init --state-dir <dir>
//!   mg-admin user add --state-dir <dir> --subject <name> --roles <r,..>
//!                     --clearance <band> --purposes <p,..> [--compartments <c,..>] [--totp]
//!   mg-admin user list --state-dir <dir>
//!   mg-admin import --state-dir <dir> --file <objects.jsonl>
//!   mg-admin admin add --state-dir <dir> --admin <name> --role <role> --keystore <path>
//!   mg-admin enrol request|approve|list --gateway <url> --admin <name> --keystore <path> ..
//!   mg-admin mesh issue --state-dir <dir> --service <name> --roles <r,..> --out <dir>
//!                       [--days <n>] [--clearance <band>] [--compartments <c,..>]
//!   mg-admin mesh renew --state-dir <dir> --service <name> --out <dir> [--within <days>]
//!                       [--days <n>] [--clearance <band>] [--compartments <c,..>]
//!   mg-admin mesh revoke --state-dir <dir> --cert <file> --out <dir,..>
//!   mg-admin audit checkpoint --state-dir <dir> --audit-pin <file>
//!
//! The password for `user add` is read from standard input, and only from a pipe or a
//! file: std has no portable way to stop a terminal echoing it, so an interactive
//! terminal is refused rather than let the password appear on screen.

mod admin;
mod http_client;
mod mesh;

use std::collections::BTreeSet;
use std::io::{BufRead, IsTerminal};
use std::path::Path;
use std::process::exit;
use std::time::{SystemTime, UNIX_EPOCH};

use attest::Keystore;
use audit_writer::{
    Action as AuditAction, Actor, AuditRecord, AuditWriter, FileLedger, LedgerId, RecordId,
    Resource as AuditResource, Timestamp,
};
use fabric::{DurableObjectStore, ObjectStore};
use http_gateway::boot::{checkpoint_legacy_ledger, init_audit_ledger, read_audit_pin};
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
    Action, Clearance, Compartment, Effect, PolicyError, PolicySet, Purpose, Role, Rule, RuleId,
    Selector,
};
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{OsRng, Rng, SecretBytes, SigningKey};

const USAGE: &str = "\
mg-admin — create an enclosure's state directory and enrol its principals

USAGE:
  mg-admin init --state-dir <dir>
  mg-admin user add --state-dir <dir> --subject <name> --roles <r,..> \\
                    --clearance <band> --purposes <p,..> [--compartments <c,..>] [--totp]
  mg-admin user list --state-dir <dir>
  mg-admin import --state-dir <dir> --file <objects.jsonl>
  mg-admin admin add --state-dir <dir> --admin <name> --role <role> --keystore <path> \\
                     [--scope <subsystem>] [--days <n>]
  mg-admin enrol request --gateway <url> --admin <name> --keystore <path> \\
                         --subject <name> --roles <r,..> --clearance <band> \\
                         --purposes <p,..> --justification <text> [--compartments <c,..>] [--totp]
  mg-admin enrol approve --gateway <url> --admin <name> --keystore <path> --id <enrolment>
  mg-admin enrol list    --gateway <url> --admin <name> --keystore <path>
  mg-admin mesh issue --state-dir <dir> --service <name> --roles <r,..> --out <dir> \\
                      [--days <n>] [--clearance <band>] [--compartments <c,..>]
  mg-admin mesh renew --state-dir <dir> --service <name> --out <dir> [--within <days>] \\
                      [--days <n>] [--clearance <band>] [--compartments <c,..>]
  mg-admin mesh revoke --state-dir <dir> --cert <file> --out <dir,..>
  mg-admin audit checkpoint --state-dir <dir> --audit-pin <file>

COMMANDS:
  init       create <dir> with fresh issuer and audit keystores, their verifying keys
             under trust/, a starter policy.json (role operator may Read and Apply;
             role approver may Read and Approve human gates) and an empty
             ontology.json. Refuses a directory that already has anything
             in it.
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
             --days (default 365). Recorded in the audit ledger.
  enrol      enrol a principal while the gateway runs, under dual control. `request`
             hashes the password (read from a pipe) locally and asks the gateway at
             --gateway, a loopback URL such as http://127.0.0.1:7710, to enrol them;
             the request waits until a *different* IAM administrator runs `approve`.
             `list` shows what is waiting. Each command signs a fresh challenge with
             the administrator's passkey.
  mesh issue give a service its mesh identity in --out: its own identity and audit-signing
             keystores (secrets, for that service alone), a certificate for --roles signed
             by the enclosure's issuer, mesh.anchor, the trust anchor every service
             verifies its peers against, and mesh.crl, the signed revocation list. The
             certificate is valid for --days (default 30) and grants --clearance (a band
             as for user add, default open) with --compartments. Never overwrites an
             existing service's identity.
  mesh renew give a service a fresh key and certificate in --out, keeping its roles.
             With --within, does nothing until the certificate expires within that many
             days, so it can run from a timer. Restart the service afterwards.
  mesh revoke
             revoke the certificate in --cert: sign the next revocation list and write it
             as mesh.crl into each --out service directory. Running services pick it up
             without a restart.
  audit checkpoint
             once, on upgrade: verify a governance ledger written before ledgers had
             checkpoints under the audit key pinned in --audit-pin, and record its first
             signed checkpoint. The gateway refuses a ledger without one.

init, user, import and admin add need the state directory's lock, which a running
gateway holds: stop it first. The enrol commands talk to the running gateway instead.
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
        ["init", rest @ ..] => init(&options(rest, &["state-dir"], &[])?),
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
        ["enrol", "request", rest @ ..] => admin::enrol_request(&options(
            rest,
            &[
                "gateway",
                "admin",
                "keystore",
                "subject",
                "roles",
                "clearance",
                "compartments",
                "purposes",
                "justification",
            ],
            &["totp"],
        )?),
        ["enrol", "approve", rest @ ..] => admin::enrol_approve(&options(
            rest,
            &["gateway", "admin", "keystore", "id"],
            &[],
        )?),
        ["enrol", "list", rest @ ..] => {
            admin::enrol_list(&options(rest, &["gateway", "admin", "keystore"], &[])?)
        }
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
            ],
            &[],
        )?),
        ["mesh", "revoke", rest @ ..] => {
            mesh::mesh_revoke(&options(rest, &["state-dir", "cert", "out"], &[])?)
        }
        ["audit", "checkpoint", rest @ ..] => {
            audit_checkpoint(&options(rest, &["state-dir", "audit-pin"], &[])?)
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
    for dir in [state.root().to_path_buf(), state.keys(), state.trust()] {
        std::fs::create_dir_all(&dir)
            .map_err(|e| Failure::Run(format!("cannot create {}: {e}", dir.display())))?;
    }

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
            .write(&path)
            .map_err(|e| Failure::Run(format!("cannot write {}: {e}", path.display())))?;
    }
    write_file(&state.idp_trust_anchor(), &idp.export_verifying_key())?;
    write_file(&state.audit_trust_anchor(), &audit.export_verifying_key())?;
    // The governance ledger exists from the start, vouched for by a signed empty
    // checkpoint, so the gateway can refuse one that later goes missing. [MG-SEC-05]
    init_audit_ledger(&state).map_err(Failure::Run)?;

    let policy = starter_policy()
        .map_err(|e| Failure::Run(format!("cannot build the starter policy: {e}")))?;
    let text = render_policy(&policy)
        .map_err(|e| Failure::Run(format!("cannot render the starter policy: {e}")))?;
    write_file(&state.policy(), &format!("{text}\n"))?;
    write_file(&state.ontology(), STARTER_ONTOLOGY)?;

    Ok(format!(
        "initialised {root}\n\
         \x20 keys/        issuer and audit keystores — protect this directory as a secret\n\
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
/// audit key, then vouched for by its first signed checkpoint. [MG-SEC-05]
fn audit_checkpoint(o: &Options) -> Result<String, Failure> {
    let state = StateDir::new(o.require("state-dir")?);
    let pin = read_audit_pin(Path::new(o.require("audit-pin")?)).map_err(Failure::Run)?;
    let _lock = StateLock::acquire(&state, "mg-admin audit checkpoint").map_err(Failure::Run)?;
    let entries = checkpoint_legacy_ledger(&state, &pin).map_err(Failure::Run)?;
    Ok(format!(
        "the audit ledger verifies under the pinned key; its checkpoint now vouches for \
         {entries} entries"
    ))
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

    let _lock = StateLock::acquire(&state, "mg-admin user add").map_err(Failure::Run)?;
    let mut directory = open_directory(&state)?;
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
    record_admin_event(&state, "ENROLL principal", &format!("principal:{subject}"))?;
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
    let _lock = StateLock::acquire(&state, "mg-admin user list").map_err(Failure::Run)?;
    let directory = open_directory(&state)?;

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

    let keystore = Keystore::read(&state.audit_keystore())
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
/// `approver` may Read and Approve the human gates a gated action needs. Mandatory
/// access control and the purpose gate still apply before any of them.
fn starter_policy() -> Result<PolicySet, PolicyError> {
    let rule = |id: &str, role: &str, action: &str| Rule {
        id: RuleId(id.into()),
        role: Role(role.into()),
        action: Action(action.into()),
        purpose: None,
        selector: Selector::Any,
        effect: Effect::Permit,
    };
    PolicySet::new(vec![
        rule("operator-read", "operator", "Read"),
        rule("operator-apply", "operator", "Apply"),
        rule("approver-read", "approver", "Read"),
        rule("approver-approve", "approver", "Approve"),
    ])
}

/// The identity directory, sealed under the issuer keystore's custody root.
fn open_directory(state: &StateDir) -> Result<DurableDirectory, Failure> {
    let keystore = Keystore::read(&state.idp_keystore()).map_err(|e| {
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
fn record_admin_event(state: &StateDir, action: &str, resource: &str) -> Result<u64, Failure> {
    let keystore = Keystore::read(&state.audit_keystore())
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
    // Shaped like the governance spine's own records, so the audit view reads it the
    // same way.
    writer
        .append(AuditRecord {
            id: RecordId(format!("{action}:{resource}:{now}")),
            actor: Actor("mg-admin".into()),
            action: AuditAction(action.into()),
            resource: AuditResource(resource.into()),
            ts: Timestamp(now.to_string()),
        })
        .map_err(|e| Failure::Run(format!("cannot record {action} in the audit ledger: {e}")))
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
        if !valued.contains(&key) {
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
