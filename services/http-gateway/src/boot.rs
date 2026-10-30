//! Serving a real enclosure from its state directory: the persisted issuer and audit
//! keys, the durable object store, audit ledger and identity directory, the policy
//! file and the ontology file. Nothing is compiled in and nothing is seeded; a directory
//! `mg-admin init` created boots with no principals until one is enrolled.
//! [RC-09] [RC-10] [RC-29] [AC-M-4]
//!
//! Boot fails closed. A missing or tampered keystore, a journal that does not
//! authenticate, a ledger chain that does not re-verify, an unreadable policy file, or
//! a state directory another process holds all refuse to start rather than serve
//! something partial.
//!
//! The governance ledger is checked under a **pinned** audit key the operator supplies
//! from outside the state directory's keystore: an audit keystore whose key is not the
//! pinned one refuses to boot, and so does a ledger that is missing, has no signed
//! checkpoint, or is shorter than its checkpoint. `mg-admin init` creates the ledger
//! ([`init_audit_ledger`]); the gateway only ever opens it. [MG-SEC-05]

use std::path::Path;

use admin_identity::{DurableAdminPasskeys, DurableIdentityDirectory, RoleCatalog};
use admin_privilege::DurablePrivilege;
use attest::Keystore;
use audit_writer::{
    verify_chain, verify_ledger, AuditWriter, FileLedger, LedgerId, LedgerStore, PinnedKey, Sealer,
};
use fabric::DurableObjectStore;
use governance::Governance;
use iam::{DurableDirectory, Idp, Pbkdf2Hasher};
use kms::{Kms, KmsSealer};
use policy::DefaultPdp;
use pqc_agility::backend::fips::{provider as fips_provider, FipsSig};
use pqc_agility::{SecretBytes, SigningKey, VerifyingKey};

use crate::admin_api::AdminControl;
use crate::enclosure::Enclosure;
use crate::ontology_file::parse_ontology;
use crate::policy_file::parse_policy;
use crate::state_dir::{StateDir, StateLock, IDP_KEY_ID, LEDGER_ID, PRIVILEGE_LEDGER_ID, STORE_ID};
use crate::SecuredGateway;

/// The gateway a state directory serves.
pub type DurableGateway = SecuredGateway<DurableObjectStore, FileLedger>;

/// A gateway opened over a state directory, with the lock that keeps any other
/// gateway or `mg-admin` out of it for as long as this is alive.
pub struct ServedEnclosure {
    /// The gateway, ready to serve.
    pub gateway: DurableGateway,
    /// Principals enrolled in the identity directory at boot.
    pub principals: usize,
    /// Entries recovered from the audit ledger, all re-verified.
    pub audit_entries: usize,
    /// Object types registered from `ontology.json`.
    pub object_types: usize,
    /// Actions registered from `ontology.json`.
    pub actions: usize,
    /// Administrators with a registered passkey, who can enrol principals online.
    pub administrators: usize,
    lock: StateLock,
}

impl ServedEnclosure {
    /// The gateway and the lock, to be kept alive together while serving.
    pub fn into_parts(self) -> (DurableGateway, StateLock) {
        (self.gateway, self.lock)
    }
}

/// Open the enclosure in `state` for serving, allowing browser sessions from
/// `allowed_origins`, with the governance ledger checked under `audit_pin` — the audit
/// verifying key, supplied from outside the state directory's keystore. [MG-SEC-05]
pub fn open_state_dir(
    state: &StateDir,
    allowed_origins: Vec<String>,
    audit_pin: &PinnedKey,
) -> Result<ServedEnclosure, String> {
    let lock = StateLock::acquire(state, "http-gateway")?;

    // The issuer key: tokens minted before a restart verify after it. Its custody
    // root seals the identity directory.
    let idp_keystore = read_keystore(&state.idp_keystore(), "issuer")?;
    let directory_sealer = sealer(&idp_keystore);
    let passkeys_sealer = sealer(&idp_keystore);
    let roles_sealer = sealer(&idp_keystore);
    let (idp_vk, idp_sk) = unseal(idp_keystore, "issuer")?;
    let idp = Idp::from_key(IDP_KEY_ID, idp_vk, idp_sk)
        .map_err(|e| format!("the issuer keystore does not hold a usable IdP key: {e}"))?;

    // The audit key: one ledger across restarts. Its custody root seals the ledger and
    // the object store.
    let audit_keystore = read_keystore(&state.audit_keystore(), "audit")?;
    let ledger_sealer = sealer(&audit_keystore);
    let store_sealer = sealer(&audit_keystore);
    let privilege_ledger_sealer = sealer(&audit_keystore);
    let privilege_state_sealer = sealer(&audit_keystore);
    let pending_sealer = sealer(&audit_keystore);
    let audit_alg = audit_keystore.alg;
    let (audit_vk, audit_sk) = unseal(audit_keystore, "audit")?;
    // The keystore that signs the ledger does not get to say which key checks it.
    // [MG-SEC-05]
    audit_pin.check(&audit_vk).map_err(|_| {
        format!(
            "the audit keystore {} does not hold the pinned audit key; refusing to serve a \
             ledger signed by an unpinned key",
            state.audit_keystore().display()
        )
    })?;

    // The ledger must already exist (`mg-admin init` makes it) and reach its signed
    // checkpoint: a deleted or shortened ledger is tampering, not a fresh start.
    let ledger = FileLedger::open(state.ledger(), ledger_sealer, LedgerId(LEDGER_ID.into()))
        .map_err(|e| format!("the audit ledger does not open: {e}"))?;
    let writer = AuditWriter::new(
        ledger,
        Box::new(FipsSig::new(audit_alg)),
        audit_sk,
        audit_vk,
        LedgerId(LEDGER_ID.into()),
    );
    // Every recovered entry, and the checkpoint vouching for how many there are, must
    // verify under the pinned key before a single request is served on top of them.
    verify_ledger(
        writer.entries(),
        writer.checkpoint(),
        &fips_provider(),
        &LedgerId(LEDGER_ID.into()),
        audit_pin,
    )
    .map_err(|e| format!("the audit ledger does not re-verify: {e}"))?;
    let audit_entries = writer.entries().len();

    let policy_path = state.policy();
    let policy_text = std::fs::read_to_string(&policy_path)
        .map_err(|e| format!("cannot read {}: {e}", policy_path.display()))?;
    let policy =
        parse_policy(&policy_text).map_err(|e| format!("{}: {e}", policy_path.display()))?;
    let governance = Governance::with_audit(DefaultPdp::new(policy), writer);

    let store = DurableObjectStore::open(state.store(), store_sealer, STORE_ID)
        .map_err(|e| format!("the object store does not open: {e}"))?;
    let directory = DurableDirectory::open(state.directory(), directory_sealer)
        .map_err(|e| format!("the identity directory does not open: {e}"))?;
    let principals = directory.len();

    // Administration: passkeys and role bindings sealed under the issuer's root, the
    // dual-control ledger and parked enrolments under the audit root. The privilege
    // service signs with its own copy of the audit key, into its own ledger.
    std::fs::create_dir_all(state.admin())
        .map_err(|e| format!("cannot create {}: {e}", state.admin().display()))?;
    let passkeys =
        DurableAdminPasskeys::open(state.admin_passkeys(), passkeys_sealer, fips_provider())
            .map_err(|e| format!("administrator passkeys do not open: {e}"))?;
    let roles =
        DurableIdentityDirectory::open(state.admin_roles(), roles_sealer, RoleCatalog::standard())
            .map_err(|e| format!("administrator roles do not open: {e}"))?;
    let (privilege_vk, privilege_sk) =
        unseal(read_keystore(&state.audit_keystore(), "audit")?, "audit")?;
    let privilege = DurablePrivilege::open(
        state.admin_privilege(),
        privilege_ledger_sealer,
        privilege_state_sealer,
        Box::new(FipsSig::new(audit_alg)),
        privilege_sk,
        privilege_vk,
        LedgerId(PRIVILEGE_LEDGER_ID.into()),
    )
    .map_err(|e| format!("the dual-control service does not open: {e:?}"))?;
    let admin = AdminControl::open(
        passkeys,
        roles,
        privilege,
        state.admin_pending(),
        pending_sealer,
    )?;
    let administrators = admin.administrators();

    // The type registry is not journalled, so the ontology is registered on every boot;
    // the objects already in the store are typed by it. A directory without an
    // ontology file serves no object types.
    let mut enclosure = Enclosure::with_store(store, governance);
    let ontology_path = state.ontology();
    let ontology = if ontology_path.exists() {
        let text = std::fs::read_to_string(&ontology_path)
            .map_err(|e| format!("cannot read {}: {e}", ontology_path.display()))?;
        let ontology =
            parse_ontology(&text).map_err(|e| format!("{}: {e}", ontology_path.display()))?;
        enclosure
            .load_ontology(&ontology, crate::now_secs())
            .map_err(|e| format!("{}: {e}", ontology_path.display()))?;
        Some(ontology)
    } else {
        None
    };
    let (object_types, actions) = ontology
        .as_ref()
        .map_or((0, 0), |o| (o.object_types.len(), o.actions.len()));

    let issuer_vk = idp.verifying_key().clone();
    let gateway =
        SecuredGateway::with_enclosure(fips_provider(), issuer_vk, enclosure, allowed_origins)
            .with_idp(idp, Pbkdf2Hasher::production())
            .with_directory(directory)
            .with_admin(admin);
    // Records synced over the mesh are checked against the same ontology.
    let gateway = match ontology {
        Some(ontology) => gateway.with_ontology(ontology),
        None => gateway,
    };

    Ok(ServedEnclosure {
        gateway,
        principals,
        audit_entries,
        object_types,
        actions,
        administrators,
        lock,
    })
}

/// Create the governance audit ledger in a new state directory and vouch for it with a
/// signed, empty checkpoint, so that from then on a deleted or emptied ledger refuses
/// to boot. Refuses if a ledger is already there. `mg-admin init` calls this once.
/// [MG-SEC-05]
pub fn init_audit_ledger(state: &StateDir) -> Result<(), String> {
    let audit_keystore = read_keystore(&state.audit_keystore(), "audit")?;
    let ledger_sealer = sealer(&audit_keystore);
    let audit_alg = audit_keystore.alg;
    let (audit_vk, audit_sk) = unseal(audit_keystore, "audit")?;
    let ledger = FileLedger::create(state.ledger(), ledger_sealer, LedgerId(LEDGER_ID.into()))
        .map_err(|e| format!("cannot create the audit ledger: {e}"))?;
    AuditWriter::new(
        ledger,
        Box::new(FipsSig::new(audit_alg)),
        audit_sk,
        audit_vk,
        LedgerId(LEDGER_ID.into()),
    )
    .seal_checkpoint()
    .map_err(|e| format!("cannot checkpoint the new audit ledger: {e}"))
}

/// Adopt a governance ledger written before checkpoints existed: verify its whole
/// chain under the pinned key, then record its first signed checkpoint. Refuses an
/// audit keystore that does not hold the pinned key, a ledger that does not verify,
/// and a ledger that already has a checkpoint. Returns the number of entries vouched
/// for. `mg-admin audit checkpoint` calls this once, on upgrade. [MG-SEC-05]
pub fn checkpoint_legacy_ledger(state: &StateDir, audit_pin: &PinnedKey) -> Result<usize, String> {
    let audit_keystore = read_keystore(&state.audit_keystore(), "audit")?;
    let ledger_sealer = sealer(&audit_keystore);
    let audit_alg = audit_keystore.alg;
    let (audit_vk, audit_sk) = unseal(audit_keystore, "audit")?;
    audit_pin
        .check(&audit_vk)
        .map_err(|_| "the audit keystore does not hold the pinned audit key".to_string())?;
    let ledger =
        FileLedger::open_uncheckpointed(state.ledger(), ledger_sealer, LedgerId(LEDGER_ID.into()))
            .map_err(|e| format!("cannot adopt the audit ledger: {e}"))?;
    verify_chain(
        ledger.entries(),
        &fips_provider(),
        &LedgerId(LEDGER_ID.into()),
        &|alg| audit_pin.resolve(alg),
    )
    .map_err(|e| format!("the audit ledger does not verify under the pinned key: {e}"))?;
    let mut writer = AuditWriter::new(
        ledger,
        Box::new(FipsSig::new(audit_alg)),
        audit_sk,
        audit_vk,
        LedgerId(LEDGER_ID.into()),
    );
    writer
        .seal_checkpoint()
        .map_err(|e| format!("cannot checkpoint the audit ledger: {e}"))?;
    Ok(writer.entries().len())
}

/// Read the pinned audit key: one `<alg> <key-id> <hex>` line, as `mg-admin init`
/// writes to `trust/audit.pub`. Keep the copy the gateway is given somewhere the
/// state directory's writer cannot reach. [MG-SEC-05]
pub fn read_audit_pin(path: &Path) -> Result<PinnedKey, String> {
    PinnedKey::read(path).map_err(|e| e.to_string())
}

fn read_keystore(path: &Path, what: &str) -> Result<Keystore, String> {
    Keystore::read(path)
        .map_err(|e| format!("cannot open the {what} keystore {}: {e}", path.display()))
}

/// A sealer under a keystore's custody root.
fn sealer(keystore: &Keystore) -> Box<dyn Sealer> {
    Box::new(KmsSealer::new(Kms::new(
        fips_provider(),
        keystore.trust_root(),
    )))
}

/// A keystore's verifying key and its unsealed signing key. The key is copied out of
/// the custodian once, into the one component that signs with it.
fn unseal(keystore: Keystore, what: &str) -> Result<(VerifyingKey, SigningKey), String> {
    let verifying_key = keystore.verifying_key.clone();
    let alg = keystore.alg;
    let root = keystore
        .into_signing_root()
        .map_err(|e| format!("cannot unseal the {what} key: {e}"))?;
    let signing_key = SigningKey::new(
        alg,
        SecretBytes::new(root.expose_signing_key().expose().to_vec()),
    );
    Ok((verifying_key, signing_key))
}
