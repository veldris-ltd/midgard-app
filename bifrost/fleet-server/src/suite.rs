//! The suite agent: `bifrost-agent --suite` keeps the native suite on one Mac at the
//! versions the orchestrator desires. [FR-B-2] [FR-B-3] [FR-B-4] [RC-17]
//!
//! # Slots
//!
//! Under the suite root:
//!
//! ```text
//! releases/<version>/<service>   one verified build per service and artefact version
//! releases/<version>/.<service>.sha256   its signed digest, checked before a kept slot
//!                                is reused
//! active/<service>               symlink -> ../releases/<version>/<service>
//! settled/<service>              the last two versions the channel settled on
//! staging/                       downloads before they are verified and moved in
//! swap.journal                   the swap in progress, if any
//! ```
//!
//! A service's version is the one its `active` link names. The native runner starts each
//! service from `active/<service>` (`run-native.sh` with `MIDGARD_SUITE_ROOT` set).
//!
//! # A swap
//!
//! 1. **Install** ([`Slots::install`]): the build was downloaded with
//!    [`client::download_verified`](crate::client::download_verified) against the agent's
//!    own release-signer anchors. It is verified again here, written to `staging/` and
//!    fsynced, read back and its SHA-256 compared with the signed manifest's, and only
//!    then renamed into its slot.
//! 2. **Intent** ([`Slots::begin_swap`]): `swap.journal` records the service, the version
//!    it leaves, the one it moves to and the slot's digest (`step=intent`), fsynced.
//! 3. **Link**: `active/<service>` is repointed by renaming a new symlink over it (atomic),
//!    and the journal moves to `step=linked`.
//! 4. **Restart** through the runner (`<runner> restart <service>`), which waits for the
//!    service's readiness the way the runner's `start` does.
//! 5. **Done** ([`Slots::finish_swap`]): the journal is removed.
//!
//! **Crash recovery** ([`Slots::recover`]), before anything else on start: leftover
//! staging files are removed. If the journal names a swap, its slot is checked against
//! the journalled digest. A good slot finishes the swap (link, restart); a missing or
//! altered one reverts it (the link goes back to the version it left, and the service is
//! restarted if the link had already moved). Either way the journal is then removed.
//!
//! **Pruning** ([`Slots::settle`], [`Slots::prune`]): when the channel has no rollout in
//! progress and every service runs its desired version, those versions are settled. Each
//! service keeps its active slot and the last two settled versions, so the previous
//! release's slot stays until the next release has fully passed; any other slot (a
//! rolled-back release's) is removed.
//!
//! # The loop ([`run`])
//!
//! Every interval (10 s by default) the agent asks the runner for each service's status
//! (`<runner> status <service>`: `ready`, `unready` or `stopped`), checks in with
//! `CheckInServices` (version and healthy = ready, per service) and gets each service's
//! desired version back. Services whose desired version differs from their active one are
//! swapped one at a time, in the order the reply lists them, on a worker thread, so the
//! agent keeps checking in while a service restarts; a service being swapped reports
//! unhealthy until its restart has finished. The orchestrator sequences the rings: the
//! agent applies whatever is desired. An install that fails (an unverifiable download, an
//! untrusted signer) changes nothing and is retried after a minute.
//!
//! When the control plane cannot be reached or refuses, the agent changes nothing and
//! retries with exponential backoff (up to 12 intervals).
//!
//! **Adoption:** on the first run (no `active/` links), the binaries in the adopt-from
//! folder become `--initial-version`, but only if every one's SHA-256 equals the
//! `payloadSha256` of that version's artefact in the registry (downloaded and verified
//! against the agent's anchors). Otherwise the agent refuses and lists the mismatches.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use attest::TrustAnchors;
use pqc_agility::sha256;
use service_runtime::client::Session;
use service_runtime::json::Json;
use service_runtime::ServiceIdentity;

use crate::client::{self, CallError, Downloaded, ServiceReport};

/// The suite's services, as the rings name them (each one's artefact id).
pub const SUITE_SERVICES: [&str; 6] = [
    "forge-worker",
    "midgard-replay",
    "heimdall-feed",
    "heimdall-source",
    "mimir-server",
    "http-gateway",
];

/// How long an install that failed is left before it is tried again.
const INSTALL_RETRY: Duration = Duration::from_secs(60);

/// A service's state as the runner sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Running and ready.
    Ready,
    /// Running, not ready (yet).
    Unready,
    /// Not running.
    Stopped,
}

/// Starts, restarts and reads the suite's services: `run-native.sh` in the suite, a fake
/// in the tests.
pub trait Runner: Send + Sync {
    /// The service's state now.
    fn status(&self, service: &str) -> Health;
    /// Stop the service and start it from its `active` link, waiting for readiness.
    fn restart(&self, service: &str) -> Result<(), String>;
}

/// A runner that is a command: `<cmd…> status <service>` prints `ready`, `unready` or
/// `stopped` (its last line counts); `<cmd…> restart <service>` exits 0 once the service
/// is ready.
pub struct CommandRunner {
    argv: Vec<String>,
}

impl CommandRunner {
    /// `command` is split on whitespace: a program and any leading arguments.
    pub fn new(command: &str) -> Result<Self, String> {
        let argv: Vec<String> = command.split_whitespace().map(str::to_string).collect();
        if argv.is_empty() {
            return Err("--runner is empty".into());
        }
        Ok(Self { argv })
    }

    fn command(&self, verb: &str, service: &str) -> Command {
        let mut c = Command::new(&self.argv[0]);
        c.args(&self.argv[1..]).arg(verb).arg(service);
        c.stdin(std::process::Stdio::null());
        c
    }
}

impl Runner for CommandRunner {
    fn status(&self, service: &str) -> Health {
        let Ok(out) = self.command("status", service).output() else {
            return Health::Stopped;
        };
        let text = String::from_utf8_lossy(&out.stdout);
        match text.lines().rev().map(str::trim).find(|l| !l.is_empty()) {
            Some("ready") if out.status.success() => Health::Ready,
            Some("unready") if out.status.success() => Health::Unready,
            _ => Health::Stopped,
        }
    }

    fn restart(&self, service: &str) -> Result<(), String> {
        let out = self
            .command("restart", service)
            .output()
            .map_err(|e| format!("cannot run the runner {}: {e}", self.argv[0]))?;
        if out.status.success() {
            return Ok(());
        }
        let mut said = String::from_utf8_lossy(&out.stdout).into_owned();
        said.push_str(&String::from_utf8_lossy(&out.stderr));
        let tail: Vec<&str> = said.lines().rev().take(3).collect();
        Err(format!(
            "restart {service} failed ({}): {}",
            out.status,
            tail.into_iter().rev().collect::<Vec<_>>().join(" / ")
        ))
    }
}

/// A name that is safe as one path component: a service name or a version.
pub fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+'))
}

fn need_safe(what: &str, name: &str) -> Result<(), String> {
    if safe_name(name) {
        Ok(())
    } else {
        Err(format!("{what} {name:?} is not a safe name"))
    }
}

/// A file's SHA-256.
pub fn file_sha256(path: &Path) -> io::Result<[u8; 32]> {
    Ok(sha256(&fs::read(path)?))
}

fn hex(d: &[u8]) -> String {
    attest::hex::hex(d)
}

fn fsync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// Write `bytes` to `path` durably: a temp file beside it, fsynced, renamed, and the
/// directory fsynced.
fn write_durably(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().expect("a file in a directory");
    let tmp = dir.join(format!(
        ".{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
    ));
    {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    fsync_dir(dir)
}

/// The step a journalled swap reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The slot is in place; the link may or may not have moved.
    Intent,
    /// The link has moved; the restart may or may not have happened.
    Linked,
}

/// A swap in progress, as `swap.journal` holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    /// The service.
    pub service: String,
    /// The version it leaves (its active one when the swap began).
    pub from: Option<String>,
    /// The version it moves to.
    pub to: String,
    /// The SHA-256 of the slot it moves to.
    pub sha256: [u8; 32],
    /// How far it got.
    pub step: Step,
}

const JOURNAL_HEADER: &str = "bifrost-suite-swap 1";

impl Journal {
    fn encode(&self) -> String {
        format!(
            "{JOURNAL_HEADER}\nservice={}\nfrom={}\nto={}\nsha256={}\nstep={}\n",
            self.service,
            self.from.as_deref().unwrap_or("-"),
            self.to,
            hex(&self.sha256),
            match self.step {
                Step::Intent => "intent",
                Step::Linked => "linked",
            }
        )
    }

    fn decode(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        if lines.next() != Some(JOURNAL_HEADER) {
            return Err("not a swap journal".into());
        }
        let fields: BTreeMap<&str, &str> = lines.filter_map(|l| l.split_once('=')).collect();
        let get = |k: &str| {
            fields
                .get(k)
                .copied()
                .ok_or_else(|| format!("the swap journal has no {k}"))
        };
        let service = get("service")?.to_string();
        let to = get("to")?.to_string();
        need_safe("service", &service)?;
        need_safe("version", &to)?;
        let from = match get("from")? {
            "-" => None,
            v => {
                need_safe("version", v)?;
                Some(v.to_string())
            }
        };
        let sha256 = attest::hex::unhex(get("sha256")?)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .ok_or("the swap journal's sha256 is not a digest")?;
        let step = match get("step")? {
            "intent" => Step::Intent,
            "linked" => Step::Linked,
            s => return Err(format!("the swap journal's step {s:?} is unknown")),
        };
        Ok(Self {
            service,
            from,
            to,
            sha256,
            step,
        })
    }
}

/// What [`Slots::recover`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// No swap was in progress.
    Clean,
    /// The swap was finished: the service now runs `to`.
    Finished {
        /// The service.
        service: String,
        /// The version it runs.
        to: String,
        /// Whether it was restarted.
        restarted: bool,
    },
    /// The swap was reverted: the service runs `from` again.
    Reverted {
        /// The service.
        service: String,
        /// The version it runs (none if it had none).
        from: Option<String>,
        /// Why.
        reason: String,
        /// Whether it was restarted.
        restarted: bool,
    },
}

/// The slots under one suite root.
#[derive(Debug, Clone)]
pub struct Slots {
    root: PathBuf,
}

impl Slots {
    /// The slots under `root`, creating its folders.
    pub fn open(root: &Path) -> io::Result<Self> {
        for d in ["releases", "active", "settled", "staging"] {
            fs::create_dir_all(root.join(d))?;
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// The suite root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Where `service`'s build at `version` lives.
    pub fn slot(&self, version: &str, service: &str) -> PathBuf {
        self.root.join("releases").join(version).join(service)
    }

    fn digest_path(&self, version: &str, service: &str) -> PathBuf {
        self.root
            .join("releases")
            .join(version)
            .join(format!(".{service}.sha256"))
    }

    /// The SHA-256 of `service`'s installed slot for `version`, if it is there and still
    /// has the signed digest recorded when it was installed.
    pub fn installed(&self, version: &str, service: &str) -> Option<[u8; 32]> {
        if !safe_name(version) || !safe_name(service) {
            return None;
        }
        let slot = self.slot(version, service);
        if !fs::symlink_metadata(&slot).ok()?.is_file() {
            return None;
        }
        let recorded = fs::read_to_string(self.digest_path(version, service)).ok()?;
        let recorded = attest::hex::unhex(recorded.trim())?;
        let have = file_sha256(&slot).ok()?;
        (recorded == have).then_some(have)
    }

    fn link_path(&self, service: &str) -> PathBuf {
        self.root.join("active").join(service)
    }

    fn journal_path(&self) -> PathBuf {
        self.root.join("swap.journal")
    }

    /// The version `active/<service>` names, if it is a well-formed link.
    pub fn active_version(&self, service: &str) -> Option<String> {
        let target = fs::read_link(self.link_path(service)).ok()?;
        let parts: Vec<&str> = target.iter().filter_map(|p| p.to_str()).collect();
        match parts.as_slice() {
            ["..", "releases", v, s] if *s == service && safe_name(v) => Some(v.to_string()),
            _ => None,
        }
    }

    /// Point `active/<service>` at its slot for `version`: a new symlink renamed over the
    /// old one, then the folder fsynced.
    pub fn link(&self, service: &str, version: &str) -> Result<(), String> {
        need_safe("service", service)?;
        need_safe("version", version)?;
        let dir = self.root.join("active");
        let tmp = dir.join(format!(".{service}.link"));
        let _ = fs::remove_file(&tmp);
        let target = PathBuf::from("..")
            .join("releases")
            .join(version)
            .join(service);
        symlink(&target, &tmp).map_err(|e| format!("cannot link {}: {e}", tmp.display()))?;
        fs::rename(&tmp, self.link_path(service))
            .map_err(|e| format!("cannot repoint active/{service}: {e}"))?;
        fsync_dir(&dir).map_err(|e| format!("cannot sync {}: {e}", dir.display()))
    }

    /// Verify `got` (downloaded as `service`@`version`) again against `anchors` and move it
    /// into its slot by way of `staging/`, read back and compared with the signed digest
    /// first. Returns the slot's SHA-256. Nothing is changed on a refusal.
    pub fn install(
        &self,
        service: &str,
        version: &str,
        got: &Downloaded,
        anchors: &TrustAnchors,
    ) -> Result<[u8; 32], String> {
        need_safe("service", service)?;
        need_safe("version", version)?;
        let m = &got.artefact.manifest;
        if m.id != service || m.version != version {
            return Err(format!(
                "asked for {service}@{version}, the download is {}@{}",
                m.id, m.version
            ));
        }
        let key = anchors.get(&got.signer).ok_or_else(|| {
            format!(
                "{service}@{version}: signer {:?} is not one this agent trusts",
                got.signer
            )
        })?;
        bifrost_artefact::verify(key, &got.artefact)
            .map_err(|e| format!("{service}@{version} does not verify: {e}"))?;
        let digest = m.payload_sha256;

        let staging = self.root.join("staging");
        let stage = staging.join(format!("{service}@{version}.partial"));
        let fail = |e: io::Error, what: &str| {
            let _ = fs::remove_file(&stage);
            format!("{service}@{version}: cannot {what}: {e}")
        };
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o700)
                .open(&stage)
                .map_err(|e| fail(e, "stage it"))?;
            f.write_all(&got.artefact.payload)
                .and_then(|_| f.sync_all())
                .map_err(|e| fail(e, "write it to staging"))?;
        }
        let staged = file_sha256(&stage).map_err(|e| fail(e, "read it back"))?;
        if staged != digest {
            let _ = fs::remove_file(&stage);
            return Err(format!(
                "{service}@{version}: the staged copy's SHA-256 {} is not the signed {}",
                hex(&staged),
                hex(&digest)
            ));
        }
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o755))
            .map_err(|e| fail(e, "make it executable"))?;
        let dir = self.root.join("releases").join(version);
        fs::create_dir_all(&dir).map_err(|e| fail(e, "make its slot"))?;
        // The signed digest beside the slot, so a kept slot can be checked before reuse.
        write_durably(&self.digest_path(version, service), hex(&digest).as_bytes())
            .map_err(|e| fail(e, "record its digest"))?;
        fs::rename(&stage, dir.join(service)).map_err(|e| fail(e, "move it into its slot"))?;
        fsync_dir(&dir)
            .and_then(|_| fsync_dir(&self.root.join("releases")))
            .and_then(|_| fsync_dir(&staging))
            .map_err(|e| format!("{service}@{version}: cannot sync its slot: {e}"))?;
        Ok(digest)
    }

    fn write_journal(&self, j: &Journal) -> Result<(), String> {
        write_durably(&self.journal_path(), j.encode().as_bytes())
            .map_err(|e| format!("cannot write the swap journal: {e}"))
    }

    /// The swap in progress, if any.
    pub fn journal(&self) -> Result<Option<Journal>, String> {
        match fs::read_to_string(self.journal_path()) {
            Ok(text) => Journal::decode(&text)
                .map(Some)
                .map_err(|e| format!("{}: {e}", self.journal_path().display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("cannot read the swap journal: {e}")),
        }
    }

    /// Journal the intent to move `service` to its installed slot for `to` (digest
    /// `sha256`), then repoint its link and journal that. The caller restarts the service
    /// and then calls [`finish_swap`](Self::finish_swap).
    pub fn begin_swap(&self, service: &str, to: &str, sha256: [u8; 32]) -> Result<(), String> {
        if let Some(j) = self.journal()? {
            return Err(format!(
                "a swap of {} to {} is still journalled",
                j.service, j.to
            ));
        }
        let mut j = Journal {
            service: service.to_string(),
            from: self.active_version(service),
            to: to.to_string(),
            sha256,
            step: Step::Intent,
        };
        self.write_journal(&j)?;
        self.link(service, to)?;
        j.step = Step::Linked;
        self.write_journal(&j)
    }

    /// The swap is done: remove the journal.
    pub fn finish_swap(&self) -> Result<(), String> {
        match fs::remove_file(self.journal_path()) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot remove the swap journal: {e}")),
        }
        fsync_dir(&self.root).map_err(|e| format!("cannot sync {}: {e}", self.root.display()))
    }

    /// Move `service` to its installed slot for `to` and restart it: the whole swap.
    pub fn swap(
        &self,
        service: &str,
        to: &str,
        sha256: [u8; 32],
        runner: &dyn Runner,
    ) -> Result<(), String> {
        self.begin_swap(service, to, sha256)?;
        let restarted = runner.restart(service);
        self.finish_swap()?;
        restarted
    }

    /// Finish or revert a swap a crash interrupted, before anything else: see the module
    /// docs. Leftover staging files are removed.
    pub fn recover(&self, runner: &dyn Runner) -> Result<Recovery, String> {
        let staging = self.root.join("staging");
        if let Ok(entries) = fs::read_dir(&staging) {
            for e in entries.flatten() {
                let _ = fs::remove_file(e.path());
            }
        }
        let Some(j) = self.journal()? else {
            return Ok(Recovery::Clean);
        };
        let slot = self.slot(&j.to, &j.service);
        let before = self.active_version(&j.service);
        let slot_ok = file_sha256(&slot).ok() == Some(j.sha256);
        let outcome = if slot_ok {
            self.link(&j.service, &j.to)?;
            // The link may have moved before the crash and the process not been restarted,
            // or the restart may not have finished: restart either way.
            let restarted = runner.restart(&j.service);
            Recovery::Finished {
                service: j.service.clone(),
                to: j.to.clone(),
                restarted: restarted.is_ok(),
            }
        } else {
            let reason = if slot.exists() {
                format!("{} no longer has the journalled digest", slot.display())
            } else {
                format!("{} is missing", slot.display())
            };
            let _ = fs::remove_file(&slot);
            match &j.from {
                Some(from) => self.link(&j.service, from)?,
                None => {
                    let _ = fs::remove_file(self.link_path(&j.service));
                }
            }
            let moved = before.as_deref() != j.from.as_deref() || j.step == Step::Linked;
            let restarted = moved && j.from.is_some() && runner.restart(&j.service).is_ok();
            Recovery::Reverted {
                service: j.service.clone(),
                from: j.from.clone(),
                reason,
                restarted,
            }
        };
        self.finish_swap()?;
        Ok(outcome)
    }

    fn settled_path(&self, service: &str) -> PathBuf {
        self.root.join("settled").join(service)
    }

    /// The versions `service` last settled on, newest first (at most two).
    pub fn settled(&self, service: &str) -> Vec<String> {
        fs::read_to_string(self.settled_path(service))
            .map(|t| {
                t.lines()
                    .map(str::trim)
                    .filter(|l| safe_name(l))
                    .map(str::to_string)
                    .take(2)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Record that the channel settled with `service` on `version`.
    pub fn settle(&self, service: &str, version: &str) -> Result<(), String> {
        let mut list = self.settled(service);
        if list.first().map(String::as_str) == Some(version) {
            return Ok(());
        }
        list.retain(|v| v != version);
        list.insert(0, version.to_string());
        list.truncate(2);
        let mut text = list.join("\n");
        text.push('\n');
        write_durably(&self.settled_path(service), text.as_bytes())
            .map_err(|e| format!("cannot record {service}'s settled versions: {e}"))
    }

    /// Remove `service`'s slots other than its active one and its last two settled ones;
    /// then any release folder left empty. Returns the versions removed.
    pub fn prune(&self, service: &str) -> Vec<String> {
        let mut keep: BTreeSet<String> = self.settled(service).into_iter().collect();
        keep.extend(self.active_version(service));
        let mut removed = Vec::new();
        let Ok(entries) = fs::read_dir(self.root.join("releases")) else {
            return removed;
        };
        for e in entries.flatten() {
            let Some(version) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let slot = e.path().join(service);
            if !keep.contains(&version) && slot.exists() && fs::remove_file(&slot).is_ok() {
                let _ = fs::remove_file(self.digest_path(&version, service));
                removed.push(version);
            }
            // Only an empty folder is removed.
            let _ = fs::remove_dir(e.path());
        }
        removed.sort();
        removed
    }
}

/// Which local builds do not match the registry's for adoption: `local` is the folder
/// of binaries, `registry` each service's signed `payloadSha256` (or why there is none).
/// Empty means every one matches.
pub fn adoption_mismatches(
    local: &Path,
    version: &str,
    registry: &[(String, Result<[u8; 32], String>)],
) -> Vec<String> {
    let mut out = Vec::new();
    for (service, signed) in registry {
        let path = local.join(service);
        let have = match file_sha256(&path) {
            Ok(d) => d,
            Err(e) => {
                out.push(format!("{service}: cannot read {}: {e}", path.display()));
                continue;
            }
        };
        match signed {
            Err(e) => out.push(format!("{service}: no verified {service}@{version}: {e}")),
            Ok(want) if *want != have => out.push(format!(
                "{service}: {} has SHA-256 {}, the registry's {service}@{version} has {}",
                path.display(),
                hex(&have),
                hex(want)
            )),
            Ok(_) => {}
        }
    }
    out
}

/// What the suite agent needs.
pub struct AgentConfig {
    /// The agent's mesh identity.
    pub identity: Arc<ServiceIdentity>,
    /// The control plane's mesh address.
    pub addr: String,
    /// The PQ-mTLS purpose.
    pub purpose: String,
    /// The installation it checks in as.
    pub install_id: String,
    /// The channel it follows.
    pub channel: String,
    /// The suite root.
    pub slots: Slots,
    /// The runner.
    pub runner: Arc<dyn Runner>,
    /// The release signers the agent trusts.
    pub anchors: TrustAnchors,
    /// The services it manages, in report order.
    pub services: Vec<String>,
    /// The version to adopt the binaries in `adopt_from` as, on the first run.
    pub initial_version: Option<String>,
    /// Where the binaries to adopt are.
    pub adopt_from: PathBuf,
    /// The check-in interval.
    pub interval: Duration,
}

/// One line of the agent's log, as JSON.
pub fn log(level: &str, message: &str) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!(
        "{}",
        Json::obj(vec![
            ("ts", Json::Int(ts as i64)),
            ("level", Json::s(level)),
            ("service", Json::s("bifrost-agent")),
            ("message", Json::s(message)),
        ])
        .encode()
    );
}

fn open_session(cfg: &AgentConfig) -> Result<Session, CallError> {
    Session::open(&cfg.identity, &cfg.addr, &cfg.purpose).map_err(|message| CallError {
        code: None,
        message,
    })
}

/// Adopt the binaries in `adopt_from` for `services` (those with no active link) as
/// `version` if every one matches the registry; otherwise refuse with the mismatches.
/// Transport errors are `Ok(false)` (retry).
fn adopt(
    cfg: &AgentConfig,
    session: &mut Session,
    services: &[String],
    version: &str,
) -> Result<bool, String> {
    let mut registry = Vec::new();
    let mut downloads = BTreeMap::new();
    for service in services {
        match client::download_verified(session, service, version, &cfg.anchors) {
            Ok(got) => {
                registry.push((service.clone(), Ok(got.artefact.manifest.payload_sha256)));
                downloads.insert(service.clone(), got);
            }
            // A transport failure, not a verdict on the build: retry later.
            Err(e)
                if e.code.is_none()
                    && !e.message.contains("does not verify here")
                    && !e.message.contains("is not one this caller trusts") =>
            {
                log("warn", &format!("adoption: {service}@{version}: {e}"));
                return Ok(false);
            }
            Err(e) => registry.push((service.clone(), Err(e.to_string()))),
        }
    }
    let mismatches = adoption_mismatches(&cfg.adopt_from, version, &registry);
    if !mismatches.is_empty() {
        return Err(format!(
            "refusing to adopt {} as {version}: {}",
            cfg.adopt_from.display(),
            mismatches.join("; ")
        ));
    }
    for service in services {
        let got = &downloads[service];
        cfg.slots.install(service, version, got, &cfg.anchors)?;
        cfg.slots.link(service, version)?;
        cfg.slots.settle(service, version)?;
    }
    log(
        "info",
        &format!(
            "adopted {} as {version}: {}",
            cfg.adopt_from.display(),
            services.join(", ")
        ),
    );
    Ok(true)
}

/// The swaps a worker carries out, one service at a time.
fn swap_all(
    cfg: &AgentConfig,
    busy: &Mutex<BTreeSet<String>>,
    failed: &Mutex<BTreeMap<(String, String), Instant>>,
    changes: Vec<(String, String)>,
) {
    let mut session: Option<Session> = None;
    for (service, version) in changes {
        let result = (|| -> Result<(), String> {
            let sha = match cfg.slots.installed(&version, &service) {
                // A slot kept from before (a rollback), unchanged since it was verified.
                Some(d) => d,
                None => {
                    if session.is_none() {
                        session = Some(open_session(cfg).map_err(|e| e.to_string())?);
                    }
                    let s = session.as_mut().expect("opened");
                    let got = client::download_verified(s, &service, &version, &cfg.anchors)
                        .map_err(|e| {
                            if e.code.is_none() {
                                session = None;
                            }
                            let why = if e.code.is_none() {
                                e.message.clone()
                            } else {
                                e.to_string()
                            };
                            format!("refused {service}@{version}: {why}")
                        })?;
                    cfg.slots.install(&service, &version, &got, &cfg.anchors)?
                }
            };
            let from = cfg.slots.active_version(&service);
            {
                // Linked under the lock the reporter reads with, so a moved link is never
                // reported healthy before its restart.
                let mut b = busy.lock().expect("busy");
                b.insert(service.clone());
                if let Err(e) = cfg.slots.begin_swap(&service, &version, sha) {
                    b.remove(&service);
                    return Err(e);
                }
            }
            log(
                "info",
                &format!(
                    "{service}: {} -> {version}; restarting",
                    from.as_deref().unwrap_or("none")
                ),
            );
            let restarted = cfg.runner.restart(&service);
            let finished = cfg.slots.finish_swap();
            busy.lock().expect("busy").remove(&service);
            finished?;
            restarted.map(|()| log("info", &format!("{service}: {version} ready")))
        })();
        if let Err(e) = result {
            log("error", &e);
            if cfg.slots.active_version(&service).as_deref() != Some(version.as_str()) {
                failed
                    .lock()
                    .expect("failed")
                    .insert((service.clone(), version.clone()), Instant::now());
            }
        }
    }
}

/// Run the suite agent until the process ends: recover, adopt if needed, then check in
/// every interval and apply what is desired. Returns only on a refusal that needs an
/// operator (a bad journal, a refused adoption).
pub fn run(cfg: AgentConfig) -> Result<(), String> {
    for s in &cfg.services {
        need_safe("service", s)?;
    }
    if cfg.anchors.iter().next().is_none() {
        return Err("no release signers are anchored; pass --trust-dir".into());
    }
    match cfg.slots.recover(cfg.runner.as_ref())? {
        Recovery::Clean => {}
        r => log("warn", &format!("recovered an interrupted swap: {r:?}")),
    }

    let cfg = Arc::new(cfg);
    let mut session: Option<Session> = None;
    let mut failures: u32 = 0;
    let cap = cfg.interval * 12;
    let backoff = |n: u32| (cfg.interval * 2u32.saturating_pow(n.min(10))).min(cap);

    let unadopted: Vec<String> = cfg
        .services
        .iter()
        .filter(|s| cfg.slots.active_version(s).is_none())
        .cloned()
        .collect();
    if !unadopted.is_empty() {
        let version = cfg.initial_version.clone().ok_or_else(|| {
            format!(
                "{} has no active build for {}; pass --initial-version to adopt {}",
                cfg.slots.root().display(),
                unadopted.join(", "),
                cfg.adopt_from.display()
            )
        })?;
        need_safe("version", &version)?;
        loop {
            let adopted = match open_session(&cfg) {
                Ok(mut s) => adopt(&cfg, &mut s, &unadopted, &version)?,
                Err(_) => false,
            };
            if adopted {
                break;
            }
            failures += 1;
            let wait = backoff(failures);
            log(
                "warn",
                &format!(
                    "control plane {} unreachable; adoption waits, retrying in {} s",
                    cfg.addr,
                    wait.as_secs()
                ),
            );
            std::thread::sleep(wait);
        }
        failures = 0;
    }

    let busy: Arc<Mutex<BTreeSet<String>>> = Arc::default();
    let failed: Arc<Mutex<BTreeMap<(String, String), Instant>>> = Arc::default();
    let mut worker: Option<JoinHandle<()>> = None;
    let mut last_seen = String::new();
    log(
        "info",
        &format!(
            "suite agent {} on {} via {}: {}",
            cfg.install_id,
            cfg.channel,
            cfg.addr,
            cfg.services
                .iter()
                .map(|s| format!(
                    "{s}={}",
                    cfg.slots
                        .active_version(s)
                        .unwrap_or_else(|| "none".to_string())
                ))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );
    loop {
        if worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let _ = worker.take().expect("finished").join();
        }
        let health: BTreeMap<&String, Health> = cfg
            .services
            .iter()
            .map(|s| (s, cfg.runner.status(s)))
            .collect();
        let reports: Vec<ServiceReport> = {
            let b = busy.lock().expect("busy");
            cfg.services
                .iter()
                .map(|s| ServiceReport {
                    name: s.clone(),
                    version: cfg
                        .slots
                        .active_version(s)
                        .unwrap_or_else(|| "none".to_string()),
                    healthy: !b.contains(s) && health[s] == Health::Ready,
                })
                .collect()
        };
        let reply = match session.as_mut() {
            Some(s) => client::check_in_services(s, &cfg.install_id, &cfg.channel, &reports),
            None => open_session(&cfg).and_then(|mut s| {
                let r = client::check_in_services(&mut s, &cfg.install_id, &cfg.channel, &reports);
                session = Some(s);
                r
            }),
        };
        let desired = match reply {
            Ok(d) => {
                if failures > 0 {
                    log("info", "control plane reachable again");
                }
                failures = 0;
                d
            }
            Err(e) => {
                session = None;
                failures += 1;
                let wait = backoff(failures);
                log(
                    "warn",
                    &format!(
                        "check-in failed ({e}); changing nothing, retrying in {} s",
                        wait.as_secs()
                    ),
                );
                std::thread::sleep(wait);
                continue;
            }
        };
        let seen = format!(
            "release={} run={} target={} desired={}",
            desired.release.as_deref().unwrap_or("-"),
            desired.run.as_deref().unwrap_or("-"),
            desired.target.as_deref().unwrap_or("-"),
            desired
                .desired
                .iter()
                .map(|(s, v)| format!("{s}={v}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        if seen != last_seen {
            log("info", &format!("checked in: {seen}"));
            last_seen = seen;
        }

        if worker.is_none() {
            let now = Instant::now();
            let mut f = failed.lock().expect("failed");
            f.retain(|_, at| now.duration_since(*at) < INSTALL_RETRY);
            let changes: Vec<(String, String)> = desired
                .desired
                .iter()
                .filter(|(s, _)| cfg.services.contains(s))
                .filter(|(s, v)| {
                    safe_name(v) && cfg.slots.active_version(s).as_deref() != Some(v.as_str())
                })
                .filter(|(s, v)| !f.contains_key(&(s.clone(), v.clone())))
                .cloned()
                .collect();
            drop(f);
            let converged = cfg.services.iter().all(|s| {
                desired
                    .desired
                    .iter()
                    .find(|(n, _)| n == s)
                    .is_some_and(|(_, v)| cfg.slots.active_version(s).as_deref() == Some(v))
            });
            if !changes.is_empty() {
                let (c, b, fl) = (Arc::clone(&cfg), Arc::clone(&busy), Arc::clone(&failed));
                worker = Some(std::thread::spawn(move || swap_all(&c, &b, &fl, changes)));
            } else if converged && desired.run.is_none() && reports.iter().all(|r| r.healthy) {
                for s in &cfg.services {
                    if let Some(v) = cfg.slots.active_version(s) {
                        if let Err(e) = cfg.slots.settle(s, &v) {
                            log("error", &e);
                            continue;
                        }
                        let removed = cfg.slots.prune(s);
                        if !removed.is_empty() {
                            log("info", &format!("{s}: pruned slots {}", removed.join(", ")));
                        }
                    }
                }
            }
        }
        std::thread::sleep(cfg.interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bifrost_artefact::{build_and_sign, generate_sbom, SbomComponent};
    use pqc_agility::backend::fips::FipsSig;
    use pqc_agility::{OsRng, SignatureScheme, SigningKey};

    fn scratch(tag: &str) -> PathBuf {
        let uniq = Box::into_raw(Box::new(0u8)) as usize;
        let dir =
            std::env::temp_dir().join(format!("bifrost-suite-{tag}-{}-{uniq}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct Keys {
        sk: SigningKey,
        anchors: TrustAnchors,
    }

    fn keys() -> Keys {
        let (vk, sk) = FipsSig::new(bifrost_artefact::BIFROST_SIG_ALG)
            .keypair(&mut OsRng)
            .unwrap();
        let mut anchors = TrustAnchors::new();
        anchors.insert("release-1", vk);
        Keys { sk, anchors }
    }

    fn build(k: &Keys, id: &str, version: &str) -> Downloaded {
        let sbom = generate_sbom(
            id,
            version,
            "service",
            vec![SbomComponent::new("pqc-agility", "0.1.0")],
        );
        let payload = format!("#!/bin/sh\n# {id} {version}\n").into_bytes();
        Downloaded {
            artefact: build_and_sign(id, version, "service", payload, sbom, &k.sk),
            signer: "release-1".into(),
        }
    }

    /// Records restarts; services are always ready.
    #[derive(Default)]
    struct Fake {
        restarts: Mutex<Vec<String>>,
    }
    impl Runner for Fake {
        fn status(&self, _: &str) -> Health {
            Health::Ready
        }
        fn restart(&self, service: &str) -> Result<(), String> {
            self.restarts.lock().unwrap().push(service.to_string());
            Ok(())
        }
    }
    impl Fake {
        fn restarts(&self) -> Vec<String> {
            self.restarts.lock().unwrap().clone()
        }
    }

    /// A suite root with forge-worker installed and active at 0.1.0 and installed (not
    /// active) at 0.2.0.
    fn two_slots(tag: &str) -> (Slots, [u8; 32], [u8; 32], PathBuf) {
        let root = scratch(tag);
        let slots = Slots::open(&root).unwrap();
        let k = keys();
        let d1 = slots
            .install(
                "forge-worker",
                "0.1.0",
                &build(&k, "forge-worker", "0.1.0"),
                &k.anchors,
            )
            .unwrap();
        slots.link("forge-worker", "0.1.0").unwrap();
        let d2 = slots
            .install(
                "forge-worker",
                "0.2.0",
                &build(&k, "forge-worker", "0.2.0"),
                &k.anchors,
            )
            .unwrap();
        (slots, d1, d2, root)
    }

    #[test]
    fn install_puts_a_verified_build_in_its_slot_and_a_swap_repoints_active() {
        let (slots, d1, d2, root) = two_slots("install");
        let slot = slots.slot("0.2.0", "forge-worker");
        assert_eq!(file_sha256(&slot).unwrap(), d2);
        assert_eq!(
            fs::metadata(&slot).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(fs::read_dir(root.join("staging")).unwrap().next().is_none());
        assert_eq!(
            slots.active_version("forge-worker").as_deref(),
            Some("0.1.0")
        );
        let link = fs::read_link(root.join("active/forge-worker")).unwrap();
        assert_eq!(link, PathBuf::from("../releases/0.1.0/forge-worker"));
        assert_eq!(file_sha256(&root.join("active/forge-worker")).unwrap(), d1);

        let runner = Fake::default();
        slots.swap("forge-worker", "0.2.0", d2, &runner).unwrap();
        assert_eq!(
            slots.active_version("forge-worker").as_deref(),
            Some("0.2.0")
        );
        assert_eq!(file_sha256(&root.join("active/forge-worker")).unwrap(), d2);
        assert_eq!(runner.restarts(), ["forge-worker"]);
        assert!(slots.journal().unwrap().is_none());
        // The previous slot is kept.
        assert!(slots.slot("0.1.0", "forge-worker").exists());
        // Back again, from the kept slot, which still has its recorded digest.
        assert_eq!(slots.installed("0.1.0", "forge-worker"), Some(d1));
        slots.swap("forge-worker", "0.1.0", d1, &runner).unwrap();
        // A kept slot altered on disk is not reused.
        fs::write(slots.slot("0.2.0", "forge-worker"), b"altered").unwrap();
        assert_eq!(slots.installed("0.2.0", "forge-worker"), None);
        assert_eq!(slots.installed("0.9.0", "forge-worker"), None);
        assert_eq!(
            slots.active_version("forge-worker").as_deref(),
            Some("0.1.0")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn install_refuses_a_tampered_an_untrusted_or_a_misnamed_download() {
        let root = scratch("refuse");
        let slots = Slots::open(&root).unwrap();
        let k = keys();
        let mut tampered = build(&k, "forge-worker", "0.2.0");
        tampered.artefact.payload[3] ^= 1;
        let e = slots
            .install("forge-worker", "0.2.0", &tampered, &k.anchors)
            .unwrap_err();
        assert!(e.contains("does not verify"), "{e}");

        let other = keys();
        let foreign = build(&other, "forge-worker", "0.2.0");
        let e = slots
            .install("forge-worker", "0.2.0", &foreign, &k.anchors)
            .unwrap_err();
        assert!(e.contains("does not verify"), "{e}");
        let mut unknown = build(&k, "forge-worker", "0.2.0");
        unknown.signer = "someone-else".into();
        let e = slots
            .install("forge-worker", "0.2.0", &unknown, &k.anchors)
            .unwrap_err();
        assert!(e.contains("not one this agent trusts"), "{e}");

        let e = slots
            .install(
                "forge-worker",
                "0.2.0",
                &build(&k, "mimir-server", "0.2.0"),
                &k.anchors,
            )
            .unwrap_err();
        assert!(e.contains("the download is mimir-server@0.2.0"), "{e}");
        let e = slots
            .install(
                "forge-worker",
                "../x",
                &build(&k, "forge-worker", "../x"),
                &k.anchors,
            )
            .unwrap_err();
        assert!(e.contains("not a safe name"), "{e}");

        assert!(!slots.slot("0.2.0", "forge-worker").exists());
        assert!(fs::read_dir(root.join("staging")).unwrap().next().is_none());
        assert!(slots.active_version("forge-worker").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    /// Crash points of a swap of forge-worker from 0.1.0 to 0.2.0, then recovery.
    #[test]
    fn recovery_finishes_or_reverts_a_swap_interrupted_at_each_step() {
        // 1. A crash while staging: the partial download is removed, nothing else moves.
        let (slots, _, _, root) = two_slots("crash-stage");
        fs::write(root.join("staging/forge-worker@0.3.0.partial"), b"half").unwrap();
        let runner = Fake::default();
        assert_eq!(slots.recover(&runner).unwrap(), Recovery::Clean);
        assert!(fs::read_dir(root.join("staging")).unwrap().next().is_none());
        assert_eq!(
            slots.active_version("forge-worker").as_deref(),
            Some("0.1.0")
        );
        assert!(runner.restarts().is_empty());
        let _ = fs::remove_dir_all(&root);

        // 2. A crash after the slot moved in, before the journal: nothing to do.
        let (slots, _, _, root) = two_slots("crash-slot");
        let runner = Fake::default();
        assert_eq!(slots.recover(&runner).unwrap(), Recovery::Clean);
        assert_eq!(
            slots.active_version("forge-worker").as_deref(),
            Some("0.1.0")
        );
        assert!(runner.restarts().is_empty());
        let _ = fs::remove_dir_all(&root);

        let intent = |d2: [u8; 32], step: Step| Journal {
            service: "forge-worker".into(),
            from: Some("0.1.0".into()),
            to: "0.2.0".into(),
            sha256: d2,
            step,
        };
        let finished = Recovery::Finished {
            service: "forge-worker".into(),
            to: "0.2.0".into(),
            restarted: true,
        };

        // 3. A crash after the intent, before the link moved: finished.
        let (slots, _, d2, root) = two_slots("crash-intent");
        slots.write_journal(&intent(d2, Step::Intent)).unwrap();
        let runner = Fake::default();
        assert_eq!(slots.recover(&runner).unwrap(), finished);
        assert_eq!(
            slots.active_version("forge-worker").as_deref(),
            Some("0.2.0")
        );
        assert_eq!(runner.restarts(), ["forge-worker"]);
        assert!(slots.journal().unwrap().is_none());
        let _ = fs::remove_dir_all(&root);

        // 4. A crash after the link moved, before the journal said so: finished.
        let (slots, _, d2, root) = two_slots("crash-link");
        slots.write_journal(&intent(d2, Step::Intent)).unwrap();
        slots.link("forge-worker", "0.2.0").unwrap();
        let runner = Fake::default();
        assert_eq!(slots.recover(&runner).unwrap(), finished);
        assert_eq!(runner.restarts(), ["forge-worker"]);
        let _ = fs::remove_dir_all(&root);

        // 5. A crash after the link was journalled, before or during the restart; and
        // 6. after the restart, before the journal was removed: restarted again.
        let (slots, _, d2, root) = two_slots("crash-restart");
        slots.write_journal(&intent(d2, Step::Intent)).unwrap();
        slots.link("forge-worker", "0.2.0").unwrap();
        slots.write_journal(&intent(d2, Step::Linked)).unwrap();
        let runner = Fake::default();
        assert_eq!(slots.recover(&runner).unwrap(), finished);
        assert_eq!(
            slots.active_version("forge-worker").as_deref(),
            Some("0.2.0")
        );
        assert_eq!(runner.restarts(), ["forge-worker"]);
        assert!(slots.journal().unwrap().is_none());
        // Recovering again is a no-op.
        assert_eq!(slots.recover(&runner).unwrap(), Recovery::Clean);
        let _ = fs::remove_dir_all(&root);

        // 7. The slot was altered after the intent: reverted, no restart (the link had
        // not moved).
        let (slots, _, d2, root) = two_slots("revert-intent");
        slots.write_journal(&intent(d2, Step::Intent)).unwrap();
        fs::write(slots.slot("0.2.0", "forge-worker"), b"altered").unwrap();
        let runner = Fake::default();
        match slots.recover(&runner).unwrap() {
            Recovery::Reverted {
                from,
                restarted,
                reason,
                ..
            } => {
                assert_eq!(from.as_deref(), Some("0.1.0"));
                assert!(!restarted);
                assert!(
                    reason.contains("no longer has the journalled digest"),
                    "{reason}"
                );
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            slots.active_version("forge-worker").as_deref(),
            Some("0.1.0")
        );
        assert!(!slots.slot("0.2.0", "forge-worker").exists());
        assert!(runner.restarts().is_empty());
        let _ = fs::remove_dir_all(&root);

        // 8. The slot vanished after the link moved: reverted and restarted on 0.1.0.
        let (slots, _, d2, root) = two_slots("revert-linked");
        slots.write_journal(&intent(d2, Step::Linked)).unwrap();
        slots.link("forge-worker", "0.2.0").unwrap();
        fs::remove_file(slots.slot("0.2.0", "forge-worker")).unwrap();
        let runner = Fake::default();
        match slots.recover(&runner).unwrap() {
            Recovery::Reverted { restarted, .. } => assert!(restarted),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            slots.active_version("forge-worker").as_deref(),
            Some("0.1.0")
        );
        assert_eq!(runner.restarts(), ["forge-worker"]);
        let _ = fs::remove_dir_all(&root);

        // A journal that does not parse stops the start.
        let (slots, _, _, root) = two_slots("bad-journal");
        fs::write(root.join("swap.journal"), "garbage").unwrap();
        assert!(slots.recover(&Fake::default()).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_swap_is_refused_while_one_is_journalled() {
        let (slots, _, d2, root) = two_slots("busy");
        slots.begin_swap("forge-worker", "0.2.0", d2).unwrap();
        let e = slots.begin_swap("forge-worker", "0.1.0", d2).unwrap_err();
        assert!(e.contains("still journalled"), "{e}");
        slots.finish_swap().unwrap();
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn pruning_keeps_the_active_slot_and_the_last_two_settled() {
        let root = scratch("prune");
        let slots = Slots::open(&root).unwrap();
        let k = keys();
        for v in ["0.1.0", "0.2.0", "0.3.0", "0.4.0"] {
            slots
                .install("forge-worker", v, &build(&k, "forge-worker", v), &k.anchors)
                .unwrap();
        }
        slots
            .install(
                "http-gateway",
                "0.1.0",
                &build(&k, "http-gateway", "0.1.0"),
                &k.anchors,
            )
            .unwrap();
        slots.settle("forge-worker", "0.1.0").unwrap();
        slots.settle("forge-worker", "0.2.0").unwrap();
        slots.settle("forge-worker", "0.2.0").unwrap();
        assert_eq!(slots.settled("forge-worker"), ["0.2.0", "0.1.0"]);
        // 0.4.0 is active (mid-rollout), 0.3.0 was a rolled-back release.
        slots.link("forge-worker", "0.4.0").unwrap();
        assert_eq!(slots.prune("forge-worker"), ["0.3.0"]);
        assert!(slots.slot("0.1.0", "forge-worker").exists());
        assert!(slots.slot("0.2.0", "forge-worker").exists());
        assert!(slots.slot("0.4.0", "forge-worker").exists());
        assert!(!root.join("releases/0.3.0").exists());
        // Another service's slot in a release folder keeps the folder.
        slots.settle("forge-worker", "0.4.0").unwrap();
        assert_eq!(slots.prune("forge-worker"), ["0.1.0"]);
        assert!(slots.slot("0.1.0", "http-gateway").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn adoption_lists_every_mismatch() {
        let dir = scratch("adopt");
        fs::write(dir.join("forge-worker"), b"forge").unwrap();
        fs::write(dir.join("http-gateway"), b"gateway, rebuilt").unwrap();
        fs::write(dir.join("heimdall-feed"), b"feed").unwrap();
        let registry = vec![
            ("forge-worker".to_string(), Ok(sha256(b"forge"))),
            ("http-gateway".to_string(), Ok(sha256(b"gateway"))),
            ("mimir-server".to_string(), Ok(sha256(b"mimir"))),
            (
                "heimdall-feed".to_string(),
                Err("not found: heimdall-feed@0.1.0 is not in the registry".to_string()),
            ),
        ];
        let got = adoption_mismatches(&dir, "0.1.0", &registry);
        assert_eq!(got.len(), 3, "{got:?}");
        assert!(got[0].starts_with("http-gateway: "), "{got:?}");
        assert!(
            got[0].contains("the registry's http-gateway@0.1.0 has"),
            "{got:?}"
        );
        assert!(got[1].starts_with("mimir-server: cannot read"), "{got:?}");
        assert!(
            got[2].contains("no verified heimdall-feed@0.1.0"),
            "{got:?}"
        );
        assert!(adoption_mismatches(&dir, "0.1.0", &registry[..1]).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn journal_round_trips_and_names_are_checked() {
        let j = Journal {
            service: "heimdall-feed".into(),
            from: None,
            to: "0.2.0+b1".into(),
            sha256: [7; 32],
            step: Step::Linked,
        };
        assert_eq!(Journal::decode(&j.encode()).unwrap(), j);
        assert!(Journal::decode(&j.encode().replace("0.2.0+b1", "../../x")).is_err());
        for bad in ["", ".", "..", ".hidden", "a/b", "a b"] {
            assert!(!safe_name(bad), "{bad}");
        }
        assert!(safe_name("0.1.0-rc.1"));
    }
}
