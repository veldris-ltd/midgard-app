//! The suite agent: `bifrost-agent --suite` keeps the native suite on one Mac at the
//! versions the orchestrator desires. [FR-B-2] [FR-B-3] [FR-B-4] [RC-17]
//!
//! # Slots
//!
//! Under the suite root:
//!
//! ```text
//! releases/<version>/<service>            one verified build per service and version
//! releases/<version>/.<service>.artefact  its signed manifest, SBOM and signature
//! active/<service>                        symlink -> ../releases/<version>/<service>
//! settled/<service>                       the last two versions the channel settled on
//! staging/                                downloads before they are verified
//! swap.journal                            the swap in progress, if any
//! agent.lock                              held (flock) by the one agent on this root
//! agent.ready                             the agent's pid, once recovery and adoption
//!                                         are done
//! ```
//!
//! A service's *linked* version is the one its `active` link names. The runner reports
//! the version a service was actually launched from (`run-native.sh` resolves the link
//! when it launches), and that *running* version is what the agent reports: a service is
//! healthy only when it is ready on its linked version.
//!
//! A kept slot is verified again (its stored manifest, SBOM and signature against the
//! agent's anchors, and the slot's bytes against the manifest) before it is reused or
//! reverted to.
//!
//! # A swap ([`Slots::swap`])
//!
//! 1. **Install** ([`Slots::install`]): the build was downloaded with
//!    [`client::download_verified`](crate::client::download_verified) against the agent's
//!    own anchors. It is verified again, written to `staging/` and fsynced, read back and
//!    compared with the signed digest, and renamed into its slot with its signed record.
//! 2. **Intent**: `swap.journal` records the service, the version it leaves, the one it
//!    moves to, the slot's digest and whether the service was running (`step=intent`).
//! 3. **Link**: `active/<service>` is repointed atomically (`step=linked`).
//! 4. **Restart** (`<runner> restart <service>`), only if the service was running: a
//!    service the user stopped is relinked and left stopped.
//! 5. **Revert** if the restart fails: the journal moves to `step=reverting`, the link
//!    goes back to the version it left (after that slot verifies again) and the service is
//!    restarted on it. If the old slot does not verify, nothing more is changed: the
//!    service stays on whatever runs and the swap is reported as stuck.
//! 6. **Done**: the journal is removed.
//!
//! **Crash recovery** ([`Slots::recover`]), before anything else on start: staging is
//! emptied; an interrupted swap whose slot still verifies is finished (link, restart, and
//! a revert if that restart fails); otherwise, or if a revert was interrupted, it is
//! reverted.
//!
//! **Pruning** ([`Slots::settle`], [`Slots::prune`]): when the channel has no rollout in
//! progress and every service runs its desired version, those versions are settled. Each
//! service keeps its active slot and the last two settled versions; other slots go.
//!
//! # The loop ([`run`])
//!
//! Every interval the agent samples each service's status from the runner (under the lock
//! a swap moves links with), checks in with `CheckInServices` and gets the desired
//! versions back. Services whose linked version differs are swapped one at a time on a
//! worker thread, so check-ins carry on. A service mid-swap reports unhealthy.
//!
//! - A swap that was reverted or stuck is not tried again while that version stays
//!   desired; an install that was refused is tried again after a minute.
//! - **Paused:** when the runner says the suite is paused (`run-native.sh stop`), the
//!   agent reports every service unhealthy and changes nothing.
//! - **Unreachable or refusing control plane:** nothing changes; exponential backoff.
//! - **SIGTERM / SIGINT:** the swap in progress is finished (or reverted), then the agent
//!   exits. The runner runs in its own process group, so a terminal's Ctrl-C does not
//!   kill it halfway.
//!
//! **Adoption:** on the first run, the binaries in the adopt-from folder become
//! `--initial-version` only if every one's SHA-256 equals that version's signed
//! `payloadSha256` (downloaded and verified against the agent's anchors); otherwise the
//! agent refuses and lists the mismatches. Each adopted service that is running is then
//! restarted once from its slot, so what runs is what was adopted.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use attest::TrustAnchors;
use bifrost_artefact::{ArtefactManifest, SignedArtefact};
use pqc_agility::{sha256, SigId, Signature};
use service_runtime::client::Session;
use service_runtime::json::{parse, Json};
use service_runtime::ServiceIdentity;

use crate::b64;
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

/// How long an install that was refused is left before it is tried again.
const INSTALL_RETRY: Duration = Duration::from_secs(60);

/// The exit code with which a runner's `restart` says it left the service stopped (the
/// suite is paused).
pub const LEFT_STOPPED: i32 = 3;

/// The services the agent is in the middle of swapping; links move under its lock.
pub type Busy = Mutex<BTreeSet<String>>;

static STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn on_stop_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// Whether SIGTERM or SIGINT has asked the agent to stop.
pub fn stopping() -> bool {
    STOP.load(Ordering::SeqCst)
}

fn handle_stop_signals() {
    let handler = on_stop_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        libc::signal(libc::SIGTERM, handler);
        libc::signal(libc::SIGINT, handler);
    }
}

/// Sleep up to `d`, returning early when the agent is asked to stop.
fn nap(d: Duration) {
    let until = Instant::now() + d;
    while !stopping() {
        let now = Instant::now();
        if now >= until {
            break;
        }
        std::thread::sleep((until - now).min(Duration::from_millis(100)));
    }
}

/// A service's state as the runner sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Running and ready, launched from this version.
    Ready(String),
    /// Running, not ready (yet, or not on its linked version), launched from this version.
    Unready(String),
    /// Not running.
    Stopped,
    /// The suite is paused: the user stopped it.
    Paused,
}

impl Status {
    /// Whether a process is running.
    pub fn running(&self) -> bool {
        matches!(self, Status::Ready(_) | Status::Unready(_))
    }
}

/// What a restart did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restarted {
    /// Restarted and ready.
    Ready,
    /// Not started: the suite is paused.
    LeftStopped,
}

/// Starts, restarts and reads the suite's services: `run-native.sh` in the suite, a fake
/// in the tests.
pub trait Runner: Send + Sync {
    /// The service's state now.
    fn status(&self, service: &str) -> Status;
    /// Stop the service and start it from its `active` link, waiting for readiness.
    fn restart(&self, service: &str) -> Result<Restarted, String>;
}

/// A runner that is a command:
/// - `<cmd…> status <service>` prints `ready <version>`, `unready <version>`, `stopped`
///   or `paused` (its last line counts; `<version>` is what the running process was
///   launched from, `-` if unknown);
/// - `<cmd…> restart <service>` exits 0 once the service is ready, [`LEFT_STOPPED`] when
///   it was not started because the suite is paused, anything else on a failure.
///
/// It runs in its own process group, so a signal to the agent's group does not reach it.
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
        c.process_group(0);
        c
    }
}

/// Parse a runner's `status` output.
pub fn parse_status(stdout: &str) -> Status {
    let line = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let mut words = line.split_whitespace();
    let version = |w: Option<&str>| w.unwrap_or("-").to_string();
    match words.next() {
        Some("ready") => Status::Ready(version(words.next())),
        Some("unready") => Status::Unready(version(words.next())),
        Some("paused") => Status::Paused,
        _ => Status::Stopped,
    }
}

impl Runner for CommandRunner {
    fn status(&self, service: &str) -> Status {
        match self.command("status", service).output() {
            Ok(out) if out.status.success() => parse_status(&String::from_utf8_lossy(&out.stdout)),
            _ => Status::Stopped,
        }
    }

    fn restart(&self, service: &str) -> Result<Restarted, String> {
        let out = self
            .command("restart", service)
            .output()
            .map_err(|e| format!("cannot run the runner {}: {e}", self.argv[0]))?;
        if out.status.success() {
            return Ok(Restarted::Ready);
        }
        if out.status.code() == Some(LEFT_STOPPED) {
            return Ok(Restarted::LeftStopped);
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

/// A downloaded build's signed record (everything but the payload), as kept beside its
/// slot.
fn encode_record(got: &Downloaded) -> String {
    let a = &got.artefact;
    let m = &a.manifest;
    Json::obj(vec![
        ("signer", Json::s(&got.signer)),
        ("id", Json::s(&m.id)),
        ("version", Json::s(&m.version)),
        ("component", Json::s(&m.component)),
        ("payloadSha256", Json::s(&hex(&m.payload_sha256))),
        ("sbomSha256", Json::s(&hex(&m.sbom_sha256))),
        ("sbom", Json::s(&b64::encode(a.sbom.as_bytes()))),
        ("alg", Json::s(a.signature.alg().as_str())),
        ("signature", Json::s(&hex(a.signature.as_bytes()))),
    ])
    .encode()
}

fn decode_record(text: &str, payload: Vec<u8>) -> Result<Downloaded, String> {
    let j = parse(text).map_err(|e| format!("not JSON: {e}"))?;
    let field = |k: &str| {
        j.str_field(k)
            .map(str::to_string)
            .ok_or_else(|| format!("no {k}"))
    };
    let digest = |k: &str| {
        attest::hex::unhex(&field(k)?)
            .and_then(|b| <[u8; 32]>::try_from(b).ok())
            .ok_or_else(|| format!("{k} is not a digest"))
    };
    let alg = field("alg")?;
    let alg = [SigId::MlDsa44, SigId::MlDsa65, SigId::MlDsa87]
        .into_iter()
        .find(|a| a.as_str() == alg)
        .ok_or_else(|| format!("unknown signature algorithm {alg}"))?;
    let sbom = String::from_utf8(b64::decode(&field("sbom")?)?)
        .map_err(|_| "the SBOM is not UTF-8".to_string())?;
    let signature = attest::hex::unhex(&field("signature")?).ok_or("the signature is not hex")?;
    Ok(Downloaded {
        artefact: SignedArtefact {
            manifest: ArtefactManifest {
                id: field("id")?,
                version: field("version")?,
                component: field("component")?,
                payload_sha256: digest("payloadSha256")?,
                sbom_sha256: digest("sbomSha256")?,
            },
            sbom,
            payload,
            signature: Signature::new(alg, signature),
        },
        signer: field("signer")?,
    })
}

/// Check that `got` is `service`@`version` and verifies under `anchors`.
fn check_build(
    service: &str,
    version: &str,
    got: &Downloaded,
    anchors: &TrustAnchors,
) -> Result<(), String> {
    let m = &got.artefact.manifest;
    if m.id != service || m.version != version {
        return Err(format!(
            "asked for {service}@{version}, the build is {}@{}",
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
        .map_err(|e| format!("{service}@{version} does not verify: {e}"))
}

/// The step a journalled swap reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The slot is in place; the link may or may not have moved.
    Intent,
    /// The link has moved; the restart may or may not have happened.
    Linked,
    /// The restart failed; the link is going back to `from`.
    Reverting,
}

/// A swap in progress, as `swap.journal` holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journal {
    /// The service.
    pub service: String,
    /// The version it leaves (its linked one when the swap began).
    pub from: Option<String>,
    /// The version it moves to.
    pub to: String,
    /// The SHA-256 of the slot it moves to.
    pub sha256: [u8; 32],
    /// Whether the service was running, and so is restarted.
    pub restart: bool,
    /// How far it got.
    pub step: Step,
}

const JOURNAL_HEADER: &str = "bifrost-suite-swap 2";

impl Journal {
    fn encode(&self) -> String {
        format!(
            "{JOURNAL_HEADER}\nservice={}\nfrom={}\nto={}\nsha256={}\nrestart={}\nstep={}\n",
            self.service,
            self.from.as_deref().unwrap_or("-"),
            self.to,
            hex(&self.sha256),
            if self.restart { "yes" } else { "no" },
            match self.step {
                Step::Intent => "intent",
                Step::Linked => "linked",
                Step::Reverting => "reverting",
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
        let restart = match get("restart")? {
            "yes" => true,
            "no" => false,
            r => return Err(format!("the swap journal's restart {r:?} is unknown")),
        };
        let step = match get("step")? {
            "intent" => Step::Intent,
            "linked" => Step::Linked,
            "reverting" => Step::Reverting,
            s => return Err(format!("the swap journal's step {s:?} is unknown")),
        };
        Ok(Self {
            service,
            from,
            to,
            sha256,
            restart,
            step,
        })
    }
}

/// How a swap ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapOutcome {
    /// `to` is linked: restarted and ready (`restarted`), or not running and left so.
    Done {
        /// Whether the service was restarted.
        restarted: bool,
    },
    /// `to` did not start; the link is back on `from`.
    Reverted {
        /// Why.
        reason: String,
        /// The restart on `from`: `Ok(true)` ready, `Ok(false)` left stopped (it was not
        /// running, or the suite is paused), or why it failed too.
        restored: Result<bool, String>,
    },
    /// `to` did not start and `from` could not be restored (missing or no longer
    /// verifying); nothing more was changed.
    Stuck {
        /// Why.
        reason: String,
    },
}

/// What [`Slots::recover`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// No swap was in progress.
    Clean,
    /// An interrupted swap of `service` to `to` was finished or reverted.
    Recovered {
        /// The service.
        service: String,
        /// The version the swap was to.
        to: String,
        /// How it ended.
        outcome: SwapOutcome,
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

    fn record_path(&self, version: &str, service: &str) -> PathBuf {
        self.root
            .join("releases")
            .join(version)
            .join(format!(".{service}.artefact"))
    }

    /// Verify `service`'s kept slot for `version` again: its stored signed record against
    /// `anchors`, and its bytes against the record. Returns the slot's SHA-256.
    pub fn verified(
        &self,
        version: &str,
        service: &str,
        anchors: &TrustAnchors,
    ) -> Result<[u8; 32], String> {
        need_safe("service", service)?;
        need_safe("version", version)?;
        let slot = self.slot(version, service);
        match fs::symlink_metadata(&slot) {
            Ok(m) if m.is_file() => {}
            Ok(_) => return Err(format!("{} is not a file", slot.display())),
            Err(e) => return Err(format!("{}: {e}", slot.display())),
        }
        let record = fs::read_to_string(self.record_path(version, service))
            .map_err(|e| format!("{service}@{version} has no signed record: {e}"))?;
        let payload = fs::read(&slot).map_err(|e| format!("{}: {e}", slot.display()))?;
        let got = decode_record(&record, payload)
            .map_err(|e| format!("{service}@{version}'s signed record: {e}"))?;
        check_build(service, version, &got, anchors)?;
        Ok(got.artefact.manifest.payload_sha256)
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
    /// first; its signed record is kept beside it. Returns the slot's SHA-256. Nothing is
    /// changed on a refusal.
    pub fn install(
        &self,
        service: &str,
        version: &str,
        got: &Downloaded,
        anchors: &TrustAnchors,
    ) -> Result<[u8; 32], String> {
        need_safe("service", service)?;
        need_safe("version", version)?;
        check_build(service, version, got, anchors)?;
        let digest = got.artefact.manifest.payload_sha256;

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
        write_durably(
            &self.record_path(version, service),
            encode_record(got).as_bytes(),
        )
        .map_err(|e| fail(e, "keep its signed record"))?;
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
    /// `sha256`; `restart` if it is running), then repoint its link and journal that.
    pub fn begin_swap(
        &self,
        service: &str,
        to: &str,
        sha256: [u8; 32],
        restart: bool,
    ) -> Result<Journal, String> {
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
            restart,
            step: Step::Intent,
        };
        self.write_journal(&j)?;
        self.link(service, to)?;
        j.step = Step::Linked;
        self.write_journal(&j)?;
        Ok(j)
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

    /// With the link on `to`: restart the service if it was running, and revert if that
    /// fails.
    fn complete(
        &self,
        j: Journal,
        runner: &dyn Runner,
        anchors: &TrustAnchors,
        busy: &Busy,
    ) -> SwapOutcome {
        if !j.restart {
            return SwapOutcome::Done { restarted: false };
        }
        match runner.restart(&j.service) {
            Ok(Restarted::Ready) => SwapOutcome::Done { restarted: true },
            Ok(Restarted::LeftStopped) => SwapOutcome::Done { restarted: false },
            Err(e) => {
                let reason = format!("{} did not start: {e}", j.to);
                self.revert(j, &reason, runner, anchors, busy)
            }
        }
    }

    /// Put the link back on `from`, once its slot verifies again, and restart the service
    /// on it if it was running. A `from` that is missing or does not verify is not used:
    /// the swap is stuck, and nothing more is changed.
    fn revert(
        &self,
        mut j: Journal,
        reason: &str,
        runner: &dyn Runner,
        anchors: &TrustAnchors,
        busy: &Busy,
    ) -> SwapOutcome {
        let Some(from) = j.from.clone() else {
            return SwapOutcome::Stuck {
                reason: format!("{reason}; it had no version before to go back to"),
            };
        };
        if let Err(e) = self.verified(&from, &j.service, anchors) {
            return SwapOutcome::Stuck {
                reason: format!("{reason}; cannot go back to {from}: {e}"),
            };
        }
        {
            let _moving = busy.lock().expect("busy");
            j.step = Step::Reverting;
            if let Err(e) = self
                .write_journal(&j)
                .and_then(|()| self.link(&j.service, &from))
            {
                return SwapOutcome::Stuck {
                    reason: format!("{reason}; cannot go back to {from}: {e}"),
                };
            }
        }
        let restored = if j.restart {
            runner.restart(&j.service).map(|r| r == Restarted::Ready)
        } else {
            Ok(false)
        };
        SwapOutcome::Reverted {
            reason: reason.to_string(),
            restored,
        }
    }

    /// The whole swap of `service` to its installed slot for `to`: journal, link (under
    /// `busy`'s lock, with the service marked busy until it ends), restart if `restart`,
    /// revert if the restart fails, and remove the journal.
    #[allow(clippy::too_many_arguments)]
    pub fn swap(
        &self,
        service: &str,
        to: &str,
        sha256: [u8; 32],
        restart: bool,
        runner: &dyn Runner,
        anchors: &TrustAnchors,
        busy: &Busy,
    ) -> Result<SwapOutcome, String> {
        let j = {
            let mut b = busy.lock().expect("busy");
            b.insert(service.to_string());
            match self.begin_swap(service, to, sha256, restart) {
                Ok(j) => j,
                Err(e) => {
                    b.remove(service);
                    return Err(e);
                }
            }
        };
        let outcome = self.complete(j, runner, anchors, busy);
        let finished = self.finish_swap();
        busy.lock().expect("busy").remove(service);
        finished.map(|()| outcome)
    }

    /// Finish or revert a swap a crash interrupted, before anything else: see the module
    /// docs. Leftover staging files are removed.
    pub fn recover(
        &self,
        runner: &dyn Runner,
        anchors: &TrustAnchors,
        busy: &Busy,
    ) -> Result<Recovery, String> {
        let staging = self.root.join("staging");
        if let Ok(entries) = fs::read_dir(&staging) {
            for e in entries.flatten() {
                let _ = fs::remove_file(e.path());
            }
        }
        let Some(mut j) = self.journal()? else {
            return Ok(Recovery::Clean);
        };
        let (service, to) = (j.service.clone(), j.to.clone());
        let outcome = if j.step == Step::Reverting {
            self.revert(j, "a revert was interrupted", runner, anchors, busy)
        } else {
            let slot_ok = file_sha256(&self.slot(&j.to, &j.service)).ok() == Some(j.sha256)
                && self.verified(&j.to, &j.service, anchors).is_ok();
            if slot_ok {
                self.link(&j.service, &j.to)?;
                j.step = Step::Linked;
                self.write_journal(&j)?;
                self.complete(j, runner, anchors, busy)
            } else {
                // The link never moved: going back needs no restart.
                if j.step == Step::Intent && self.active_version(&j.service) == j.from {
                    j.restart = false;
                }
                let reason = format!(
                    "{} is missing or no longer verifies",
                    self.slot(&j.to, &j.service).display()
                );
                let _ = fs::remove_file(self.slot(&j.to, &j.service));
                self.revert(j, &reason, runner, anchors, busy)
            }
        };
        self.finish_swap()?;
        Ok(Recovery::Recovered {
            service,
            to,
            outcome,
        })
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
    /// then any release folder left empty. Only real folders with safe names are looked
    /// at, and only regular files removed. Returns the versions removed.
    pub fn prune(&self, service: &str) -> Vec<String> {
        let mut keep: BTreeSet<String> = self.settled(service).into_iter().collect();
        keep.extend(self.active_version(service));
        let mut removed = Vec::new();
        if !safe_name(service) {
            return removed;
        }
        let Ok(entries) = fs::read_dir(self.root.join("releases")) else {
            return removed;
        };
        for e in entries.flatten() {
            let Some(version) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let real_dir = fs::symlink_metadata(e.path()).is_ok_and(|m| m.is_dir());
            if !safe_name(&version) || !real_dir {
                continue;
            }
            let slot = e.path().join(service);
            let is_file = fs::symlink_metadata(&slot).is_ok_and(|m| m.is_file());
            if !keep.contains(&version) && is_file && fs::remove_file(&slot).is_ok() {
                let _ = fs::remove_file(self.record_path(&version, service));
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

/// Whether a call failed in transport (worth retrying), rather than with a verdict.
pub fn is_transport(e: &CallError) -> bool {
    e.code.is_none()
        && (e.message.starts_with("cannot connect")
            || e.message.starts_with("cannot send the request")
            || e.message.contains("closed the channel")
            || e.message.contains("refused the PQ-mTLS channel"))
}

/// What the agent reports for each service, sampled under `busy`'s lock: the version
/// the running process was launched from (the linked one when nothing runs), healthy only
/// when it is ready on its linked version and not mid-swap. Also whether the suite is
/// paused.
pub fn sample(
    slots: &Slots,
    runner: &dyn Runner,
    services: &[String],
    busy: &Busy,
) -> (Vec<ServiceReport>, bool) {
    let b = busy.lock().expect("busy");
    let mut paused = false;
    let reports = services
        .iter()
        .map(|s| {
            let linked = slots.active_version(s);
            let linked_or_none = linked.clone().unwrap_or_else(|| "none".to_string());
            let running = |v: &str| {
                if v == "-" || v.is_empty() {
                    linked_or_none.clone()
                } else {
                    v.to_string()
                }
            };
            let (version, healthy) = match runner.status(s) {
                Status::Ready(v) => {
                    let healthy = !b.contains(s) && linked.as_deref() == Some(v.as_str());
                    (running(&v), healthy)
                }
                Status::Unready(v) => (running(&v), false),
                Status::Stopped => (linked_or_none, false),
                Status::Paused => {
                    paused = true;
                    (linked_or_none, false)
                }
            };
            ServiceReport {
                name: s.clone(),
                version,
                healthy,
            }
        })
        .collect();
    (reports, paused)
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

/// Hold the suite root's single-instance lock for as long as the file is open.
fn lock_root(root: &Path) -> Result<File, String> {
    let path = root.join("agent.lock");
    let f = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)
        .map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    // SAFETY: flock on a descriptor this function owns.
    let got = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if got != 0 {
        return Err(format!(
            "another bifrost-agent is running on {} (it holds {})",
            root.display(),
            path.display()
        ));
    }
    Ok(f)
}

/// Adopt the binaries in `adopt_from` for `services` (those with no active link) as
/// `version` if every one matches the registry; otherwise refuse with the mismatches.
/// Transport errors are `Ok(false)` (retry); any other error is a refusal.
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
            Err(e) if is_transport(&e) => {
                log("warn", &format!("adoption: {service}@{version}: {e}"));
                return Ok(false);
            }
            Err(e) => {
                let why = if e.code.is_none() {
                    e.message.clone()
                } else {
                    e.to_string()
                };
                registry.push((service.clone(), Err(why)));
            }
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
    // What runs must be what was adopted: restart each running service from its slot.
    for service in services {
        if stopping() {
            break;
        }
        match cfg.runner.status(service) {
            Status::Paused => {
                log(
                    "info",
                    "the suite is paused; adopted services are not restarted",
                );
                break;
            }
            Status::Stopped => log(
                "info",
                &format!("{service} is not running; adopted, left stopped"),
            ),
            _ => match cfg.runner.restart(service) {
                Ok(Restarted::Ready) => log(
                    "info",
                    &format!("{service}: restarted from its {version} slot"),
                ),
                Ok(Restarted::LeftStopped) => log(
                    "info",
                    &format!("{service}: left stopped (the suite is paused)"),
                ),
                Err(e) => log("error", &format!("adoption restart: {e}")),
            },
        }
    }
    Ok(true)
}

/// Versions the agent will not try again for now.
#[derive(Default)]
struct Failed {
    /// Refused installs, tried again after [`INSTALL_RETRY`].
    installs: BTreeMap<(String, String), Instant>,
    /// Swaps that were reverted or stuck, not tried again while still desired.
    swaps: BTreeSet<(String, String)>,
}

fn log_outcome(service: &str, from: Option<&str>, to: &str, outcome: &SwapOutcome) {
    let from = from.unwrap_or("none");
    match outcome {
        SwapOutcome::Done { restarted: true } => {
            log("info", &format!("{service}: {to} ready (was {from})"))
        }
        SwapOutcome::Done { restarted: false } => log(
            "info",
            &format!("{service}: linked {to} (was {from}); not running, left stopped"),
        ),
        SwapOutcome::Reverted { reason, restored } => log(
            "error",
            &format!(
                "{service}: restart of {to} failed ({reason}); reverted, back on {from} ({})",
                match restored {
                    Ok(true) => "ready".to_string(),
                    Ok(false) => "left stopped".to_string(),
                    Err(e) => format!("its restart failed too: {e}"),
                }
            ),
        ),
        SwapOutcome::Stuck { reason } => log(
            "error",
            &format!("{service}: swap to {to} stuck: {reason}; left as it runs"),
        ),
    }
}

/// The swaps a worker carries out, one service at a time; it stops between services when
/// the agent is asked to stop.
fn swap_all(
    cfg: &AgentConfig,
    busy: &Busy,
    failed: &Mutex<Failed>,
    changes: Vec<(String, String)>,
) {
    let mut session: Option<Session> = None;
    for (service, version) in changes {
        if stopping() {
            break;
        }
        let sha = match cfg.slots.verified(&version, &service, &cfg.anchors) {
            // A slot kept from before (a rollback), verified again.
            Ok(d) => Ok(d),
            Err(_) => (|| -> Result<[u8; 32], String> {
                if session.is_none() {
                    session = Some(open_session(cfg).map_err(|e| e.message)?);
                }
                let s = session.as_mut().expect("opened");
                let got = client::download_verified(s, &service, &version, &cfg.anchors).map_err(
                    |e| {
                        if e.code.is_none() {
                            session = None;
                            e.message
                        } else {
                            e.to_string()
                        }
                    },
                )?;
                cfg.slots.install(&service, &version, &got, &cfg.anchors)
            })(),
        };
        let sha = match sha {
            Ok(d) => d,
            Err(e) => {
                log("error", &format!("refused {service}@{version}: {e}"));
                failed
                    .lock()
                    .expect("failed")
                    .installs
                    .insert((service.clone(), version.clone()), Instant::now());
                continue;
            }
        };
        let status = cfg.runner.status(&service);
        if status == Status::Paused {
            log("info", "the suite is paused; swaps wait");
            break;
        }
        let from = cfg.slots.active_version(&service);
        log(
            "info",
            &format!(
                "{service}: {} -> {version}{}",
                from.as_deref().unwrap_or("none"),
                if status.running() {
                    "; restarting"
                } else {
                    " (not running: link only)"
                }
            ),
        );
        match cfg.slots.swap(
            &service,
            &version,
            sha,
            status.running(),
            cfg.runner.as_ref(),
            &cfg.anchors,
            busy,
        ) {
            Ok(outcome) => {
                log_outcome(&service, from.as_deref(), &version, &outcome);
                if !matches!(outcome, SwapOutcome::Done { .. }) {
                    failed
                        .lock()
                        .expect("failed")
                        .swaps
                        .insert((service.clone(), version.clone()));
                }
            }
            Err(e) => {
                log("error", &format!("{service}: swap to {version}: {e}"));
                failed
                    .lock()
                    .expect("failed")
                    .swaps
                    .insert((service.clone(), version.clone()));
            }
        }
    }
}

/// The marker `run-local.sh agent start` waits for.
fn ready_marker(slots: &Slots) -> PathBuf {
    slots.root().join("agent.ready")
}

/// Run the suite agent until it is stopped (SIGTERM / SIGINT, after finishing the swap in
/// progress): take the suite root's lock, recover, adopt if needed, then check in every
/// interval and apply what is desired. Returns an error only on a refusal that needs an
/// operator (another agent on the root, a bad journal, a refused adoption).
pub fn run(cfg: AgentConfig) -> Result<(), String> {
    for s in &cfg.services {
        need_safe("service", s)?;
    }
    if cfg.anchors.iter().next().is_none() {
        return Err("no release signers are anchored; pass --trust-dir".into());
    }
    let _lock = lock_root(cfg.slots.root())?;
    let marker = ready_marker(&cfg.slots);
    let _ = fs::remove_file(&marker);
    handle_stop_signals();
    let busy: Arc<Busy> = Arc::default();
    let failed: Arc<Mutex<Failed>> = Arc::default();

    match cfg
        .slots
        .recover(cfg.runner.as_ref(), &cfg.anchors, &busy)?
    {
        Recovery::Clean => {}
        Recovery::Recovered {
            service,
            to,
            outcome,
        } => {
            log(
                "warn",
                &format!("recovered an interrupted swap of {service} to {to}"),
            );
            log_outcome(&service, None, &to, &outcome);
            if !matches!(outcome, SwapOutcome::Done { .. }) {
                failed.lock().expect("failed").swaps.insert((service, to));
            }
        }
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
            if stopping() {
                log("info", "stopped before adoption finished");
                return Ok(());
            }
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
            nap(wait);
        }
        failures = 0;
    }
    write_durably(&marker, format!("{}\n", std::process::id()).as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", marker.display()))?;

    let mut worker: Option<JoinHandle<()>> = None;
    let mut last_seen = String::new();
    let mut was_paused = false;
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
    while !stopping() {
        if worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let _ = worker.take().expect("finished").join();
        }
        let (reports, paused) = sample(&cfg.slots, cfg.runner.as_ref(), &cfg.services, &busy);
        if paused != was_paused {
            log(
                "info",
                if paused {
                    "the suite is paused (run-native.sh stop): reporting it unhealthy, changing nothing"
                } else {
                    "the suite is no longer paused"
                },
            );
            was_paused = paused;
        }
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
                nap(wait);
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

        if worker.is_none() && !paused {
            let now = Instant::now();
            let mut f = failed.lock().expect("failed");
            f.installs
                .retain(|_, at| now.duration_since(*at) < INSTALL_RETRY);
            // A failed swap is forgotten once its version is no longer desired.
            f.swaps
                .retain(|(s, v)| desired.desired.iter().any(|(n, d)| n == s && d == v));
            let changes: Vec<(String, String)> = desired
                .desired
                .iter()
                .filter(|(s, _)| cfg.services.contains(s))
                .filter(|(s, v)| {
                    safe_name(v) && cfg.slots.active_version(s).as_deref() != Some(v.as_str())
                })
                .filter(|(s, v)| {
                    let key = (s.clone(), v.clone());
                    !f.installs.contains_key(&key) && !f.swaps.contains(&key)
                })
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
        nap(cfg.interval);
    }
    if let Some(w) = worker.take() {
        log("info", "stopping: finishing the swap in progress");
        let _ = w.join();
    }
    let _ = fs::remove_file(&marker);
    log("info", "stopped");
    Ok(())
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

    /// Records each restart as service@linked-version; a restart on a version in `fail`
    /// fails; a restart while `paused` leaves the service stopped.
    struct Fake {
        root: PathBuf,
        fail: Vec<&'static str>,
        paused: bool,
        restarts: Mutex<Vec<String>>,
    }
    impl Fake {
        fn new(slots: &Slots) -> Self {
            Self {
                root: slots.root().to_path_buf(),
                fail: vec![],
                paused: false,
                restarts: Mutex::default(),
            }
        }
        fn failing(slots: &Slots, fail: &[&'static str]) -> Self {
            Self {
                fail: fail.to_vec(),
                ..Self::new(slots)
            }
        }
        fn restarts(&self) -> Vec<String> {
            self.restarts.lock().unwrap().clone()
        }
    }
    impl Runner for Fake {
        fn status(&self, _: &str) -> Status {
            Status::Ready("-".into())
        }
        fn restart(&self, service: &str) -> Result<Restarted, String> {
            if self.paused {
                return Ok(Restarted::LeftStopped);
            }
            let v = Slots {
                root: self.root.clone(),
            }
            .active_version(service)
            .unwrap_or_default();
            self.restarts.lock().unwrap().push(format!("{service}@{v}"));
            if self.fail.contains(&v.as_str()) {
                Err(format!("{service} {v} not ready"))
            } else {
                Ok(Restarted::Ready)
            }
        }
    }

    /// A suite root with forge-worker installed and active at 0.1.0 and installed (not
    /// active) at 0.2.0.
    fn two_slots(tag: &str) -> (Slots, Keys, [u8; 32], [u8; 32], PathBuf) {
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
        (slots, k, d1, d2, root)
    }

    fn active(slots: &Slots) -> Option<String> {
        slots.active_version("forge-worker")
    }

    #[test]
    fn install_puts_a_verified_build_in_its_slot_and_a_swap_repoints_active() {
        let (slots, k, d1, d2, root) = two_slots("install");
        let busy = Busy::default();
        let slot = slots.slot("0.2.0", "forge-worker");
        assert_eq!(file_sha256(&slot).unwrap(), d2);
        assert_eq!(
            fs::metadata(&slot).unwrap().permissions().mode() & 0o777,
            0o755
        );
        assert!(fs::read_dir(root.join("staging")).unwrap().next().is_none());
        assert_eq!(active(&slots).as_deref(), Some("0.1.0"));
        let link = fs::read_link(root.join("active/forge-worker")).unwrap();
        assert_eq!(link, PathBuf::from("../releases/0.1.0/forge-worker"));
        assert_eq!(file_sha256(&root.join("active/forge-worker")).unwrap(), d1);

        let runner = Fake::new(&slots);
        let out = slots
            .swap(
                "forge-worker",
                "0.2.0",
                d2,
                true,
                &runner,
                &k.anchors,
                &busy,
            )
            .unwrap();
        assert_eq!(out, SwapOutcome::Done { restarted: true });
        assert_eq!(active(&slots).as_deref(), Some("0.2.0"));
        assert_eq!(runner.restarts(), ["forge-worker@0.2.0"]);
        assert!(slots.journal().unwrap().is_none());
        assert!(busy.lock().unwrap().is_empty());
        // The kept previous slot verifies again from its signed record.
        assert_eq!(slots.verified("0.1.0", "forge-worker", &k.anchors), Ok(d1));
        slots
            .swap(
                "forge-worker",
                "0.1.0",
                d1,
                true,
                &runner,
                &k.anchors,
                &busy,
            )
            .unwrap();
        assert_eq!(active(&slots).as_deref(), Some("0.1.0"));

        // A kept slot altered on disk, a record altered, or another signer: not reused.
        fs::write(slots.slot("0.2.0", "forge-worker"), b"altered").unwrap();
        let e = slots
            .verified("0.2.0", "forge-worker", &k.anchors)
            .unwrap_err();
        assert!(e.contains("does not verify"), "{e}");
        let record = slots.record_path("0.1.0", "forge-worker");
        let text = fs::read_to_string(&record).unwrap();
        fs::write(&record, text.replace("\"0.1.0\"", "\"0.1.1\"")).unwrap();
        assert!(slots.verified("0.1.0", "forge-worker", &k.anchors).is_err());
        fs::write(&record, &text).unwrap();
        assert!(slots
            .verified("0.1.0", "forge-worker", &keys().anchors)
            .is_err());
        assert!(slots.verified("0.9.0", "forge-worker", &k.anchors).is_err());
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
        assert!(e.contains("the build is mimir-server@0.2.0"), "{e}");
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

    #[test]
    fn a_failed_restart_reverts_to_the_old_version_and_restarts_it() {
        let (slots, k, _, d2, root) = two_slots("revert");
        let busy = Busy::default();
        let runner = Fake::failing(&slots, &["0.2.0"]);
        let out = slots
            .swap(
                "forge-worker",
                "0.2.0",
                d2,
                true,
                &runner,
                &k.anchors,
                &busy,
            )
            .unwrap();
        match out {
            SwapOutcome::Reverted { reason, restored } => {
                assert!(reason.contains("0.2.0 did not start"), "{reason}");
                assert_eq!(restored, Ok(true));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(active(&slots).as_deref(), Some("0.1.0"));
        assert_eq!(
            runner.restarts(),
            ["forge-worker@0.2.0", "forge-worker@0.1.0"]
        );
        assert!(slots.journal().unwrap().is_none());
        assert!(busy.lock().unwrap().is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_restart_with_no_good_slot_to_go_back_to_is_stuck_and_changes_nothing_more() {
        let (slots, k, _, d2, root) = two_slots("stuck");
        fs::write(slots.slot("0.1.0", "forge-worker"), b"altered").unwrap();
        let runner = Fake::failing(&slots, &["0.2.0"]);
        let out = slots
            .swap(
                "forge-worker",
                "0.2.0",
                d2,
                true,
                &runner,
                &k.anchors,
                &Busy::default(),
            )
            .unwrap();
        match out {
            SwapOutcome::Stuck { reason } => {
                assert!(reason.contains("cannot go back to 0.1.0"), "{reason}")
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(active(&slots).as_deref(), Some("0.2.0"));
        assert_eq!(runner.restarts(), ["forge-worker@0.2.0"]);
        assert!(slots.journal().unwrap().is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_stopped_service_or_a_paused_suite_is_relinked_and_not_started() {
        let (slots, k, d1, d2, root) = two_slots("stopped");
        let runner = Fake::new(&slots);
        let out = slots
            .swap(
                "forge-worker",
                "0.2.0",
                d2,
                false,
                &runner,
                &k.anchors,
                &Busy::default(),
            )
            .unwrap();
        assert_eq!(out, SwapOutcome::Done { restarted: false });
        assert_eq!(active(&slots).as_deref(), Some("0.2.0"));
        assert!(runner.restarts().is_empty());
        let mut paused = Fake::new(&slots);
        paused.paused = true;
        let out = slots
            .swap(
                "forge-worker",
                "0.1.0",
                d1,
                true,
                &paused,
                &k.anchors,
                &Busy::default(),
            )
            .unwrap();
        assert_eq!(out, SwapOutcome::Done { restarted: false });
        let _ = fs::remove_dir_all(&root);
    }

    /// Crash points of a swap of forge-worker from 0.1.0 to 0.2.0, then recovery.
    #[test]
    fn recovery_finishes_or_reverts_a_swap_interrupted_at_each_step() {
        let busy = Busy::default();
        let recovered = |outcome| Recovery::Recovered {
            service: "forge-worker".into(),
            to: "0.2.0".into(),
            outcome,
        };
        let finished = recovered(SwapOutcome::Done { restarted: true });

        // 1. A crash while staging: the partial download is removed, nothing else moves.
        let (slots, k, _, _, root) = two_slots("crash-stage");
        fs::write(root.join("staging/forge-worker@0.3.0.partial"), b"half").unwrap();
        let runner = Fake::new(&slots);
        assert_eq!(
            slots.recover(&runner, &k.anchors, &busy).unwrap(),
            Recovery::Clean
        );
        assert!(fs::read_dir(root.join("staging")).unwrap().next().is_none());
        assert_eq!(active(&slots).as_deref(), Some("0.1.0"));
        assert!(runner.restarts().is_empty());
        let _ = fs::remove_dir_all(&root);

        // 2. A crash after the slot moved in, before the journal: nothing to do.
        let (slots, k, _, _, root) = two_slots("crash-slot");
        let runner = Fake::new(&slots);
        assert_eq!(
            slots.recover(&runner, &k.anchors, &busy).unwrap(),
            Recovery::Clean
        );
        assert!(runner.restarts().is_empty());
        let _ = fs::remove_dir_all(&root);

        let journal = |d2: [u8; 32], step: Step| Journal {
            service: "forge-worker".into(),
            from: Some("0.1.0".into()),
            to: "0.2.0".into(),
            sha256: d2,
            restart: true,
            step,
        };

        // 3. A crash after the intent, before the link moved: finished.
        let (slots, k, _, d2, root) = two_slots("crash-intent");
        slots.write_journal(&journal(d2, Step::Intent)).unwrap();
        let runner = Fake::new(&slots);
        assert_eq!(slots.recover(&runner, &k.anchors, &busy).unwrap(), finished);
        assert_eq!(active(&slots).as_deref(), Some("0.2.0"));
        assert_eq!(runner.restarts(), ["forge-worker@0.2.0"]);
        assert!(slots.journal().unwrap().is_none());
        let _ = fs::remove_dir_all(&root);

        // 4. A crash after the link moved, before the journal said so: finished.
        let (slots, k, _, d2, root) = two_slots("crash-link");
        slots.write_journal(&journal(d2, Step::Intent)).unwrap();
        slots.link("forge-worker", "0.2.0").unwrap();
        let runner = Fake::new(&slots);
        assert_eq!(slots.recover(&runner, &k.anchors, &busy).unwrap(), finished);
        assert_eq!(runner.restarts(), ["forge-worker@0.2.0"]);
        let _ = fs::remove_dir_all(&root);

        // 5. A crash after the link was journalled, before or during the restart; and
        // 6. after the restart, before the journal was removed: restarted again.
        let (slots, k, _, d2, root) = two_slots("crash-restart");
        slots.write_journal(&journal(d2, Step::Intent)).unwrap();
        slots.link("forge-worker", "0.2.0").unwrap();
        slots.write_journal(&journal(d2, Step::Linked)).unwrap();
        let runner = Fake::new(&slots);
        assert_eq!(slots.recover(&runner, &k.anchors, &busy).unwrap(), finished);
        assert_eq!(active(&slots).as_deref(), Some("0.2.0"));
        assert_eq!(runner.restarts(), ["forge-worker@0.2.0"]);
        assert!(slots.journal().unwrap().is_none());
        assert_eq!(
            slots.recover(&runner, &k.anchors, &busy).unwrap(),
            Recovery::Clean
        );
        let _ = fs::remove_dir_all(&root);

        // 7. Finishing, the restart fails: reverted to 0.1.0 and restarted on it.
        let (slots, k, _, d2, root) = two_slots("crash-fail");
        slots.link("forge-worker", "0.2.0").unwrap();
        slots.write_journal(&journal(d2, Step::Linked)).unwrap();
        let runner = Fake::failing(&slots, &["0.2.0"]);
        match slots.recover(&runner, &k.anchors, &busy).unwrap() {
            Recovery::Recovered {
                outcome: SwapOutcome::Reverted { restored, .. },
                ..
            } => assert_eq!(restored, Ok(true)),
            other => panic!("{other:?}"),
        }
        assert_eq!(active(&slots).as_deref(), Some("0.1.0"));
        assert_eq!(
            runner.restarts(),
            ["forge-worker@0.2.0", "forge-worker@0.1.0"]
        );
        let _ = fs::remove_dir_all(&root);

        // 8. A crash while reverting: the revert is finished.
        let (slots, k, _, d2, root) = two_slots("crash-reverting");
        slots.link("forge-worker", "0.2.0").unwrap();
        slots.write_journal(&journal(d2, Step::Reverting)).unwrap();
        let runner = Fake::new(&slots);
        match slots.recover(&runner, &k.anchors, &busy).unwrap() {
            Recovery::Recovered {
                outcome: SwapOutcome::Reverted { restored, .. },
                ..
            } => assert_eq!(restored, Ok(true)),
            other => panic!("{other:?}"),
        }
        assert_eq!(active(&slots).as_deref(), Some("0.1.0"));
        assert_eq!(runner.restarts(), ["forge-worker@0.1.0"]);
        let _ = fs::remove_dir_all(&root);

        // 9. The slot was altered after the intent: reverted, no restart (the link had
        // not moved).
        let (slots, k, _, d2, root) = two_slots("revert-intent");
        slots.write_journal(&journal(d2, Step::Intent)).unwrap();
        fs::write(slots.slot("0.2.0", "forge-worker"), b"altered").unwrap();
        let runner = Fake::new(&slots);
        match slots.recover(&runner, &k.anchors, &busy).unwrap() {
            Recovery::Recovered {
                outcome: SwapOutcome::Reverted { reason, restored },
                ..
            } => {
                assert_eq!(restored, Ok(false));
                assert!(reason.contains("no longer verifies"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(active(&slots).as_deref(), Some("0.1.0"));
        assert!(!slots.slot("0.2.0", "forge-worker").exists());
        assert!(runner.restarts().is_empty());
        let _ = fs::remove_dir_all(&root);

        // 10. The slot vanished after the link moved: reverted and restarted on 0.1.0.
        let (slots, k, _, d2, root) = two_slots("revert-linked");
        slots.write_journal(&journal(d2, Step::Linked)).unwrap();
        slots.link("forge-worker", "0.2.0").unwrap();
        fs::remove_file(slots.slot("0.2.0", "forge-worker")).unwrap();
        let runner = Fake::new(&slots);
        match slots.recover(&runner, &k.anchors, &busy).unwrap() {
            Recovery::Recovered {
                outcome: SwapOutcome::Reverted { restored, .. },
                ..
            } => assert_eq!(restored, Ok(true)),
            other => panic!("{other:?}"),
        }
        assert_eq!(active(&slots).as_deref(), Some("0.1.0"));
        assert_eq!(runner.restarts(), ["forge-worker@0.1.0"]);
        let _ = fs::remove_dir_all(&root);

        // 11. Both slots bad: stuck, the link left alone.
        let (slots, k, _, d2, root) = two_slots("recover-stuck");
        slots.write_journal(&journal(d2, Step::Linked)).unwrap();
        slots.link("forge-worker", "0.2.0").unwrap();
        fs::write(slots.slot("0.2.0", "forge-worker"), b"altered").unwrap();
        fs::write(slots.slot("0.1.0", "forge-worker"), b"altered").unwrap();
        let runner = Fake::new(&slots);
        match slots.recover(&runner, &k.anchors, &busy).unwrap() {
            Recovery::Recovered {
                outcome: SwapOutcome::Stuck { .. },
                ..
            } => {}
            other => panic!("{other:?}"),
        }
        assert!(runner.restarts().is_empty());
        assert!(slots.journal().unwrap().is_none());
        let _ = fs::remove_dir_all(&root);

        // A journal that does not parse stops the start.
        let (slots, k, _, _, root) = two_slots("bad-journal");
        fs::write(root.join("swap.journal"), "garbage").unwrap();
        assert!(slots
            .recover(&Fake::new(&slots), &k.anchors, &busy)
            .is_err());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_second_swap_is_refused_while_one_is_journalled() {
        let (slots, _, _, d2, root) = two_slots("busy");
        slots.begin_swap("forge-worker", "0.2.0", d2, true).unwrap();
        let e = slots
            .begin_swap("forge-worker", "0.1.0", d2, true)
            .unwrap_err();
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
        // A symlinked folder and an oddly named one are left alone.
        let outside = scratch("prune-outside");
        fs::write(outside.join("forge-worker"), b"not ours").unwrap();
        symlink(&outside, root.join("releases/9.9.9")).unwrap();
        fs::create_dir_all(root.join("releases/.odd")).unwrap();
        fs::write(root.join("releases/.odd/forge-worker"), b"x").unwrap();
        // 0.4.0 is active (mid-rollout), 0.3.0 was a rolled-back release.
        slots.link("forge-worker", "0.4.0").unwrap();
        assert_eq!(slots.prune("forge-worker"), ["0.3.0"]);
        assert!(slots.slot("0.1.0", "forge-worker").exists());
        assert!(slots.slot("0.2.0", "forge-worker").exists());
        assert!(slots.slot("0.4.0", "forge-worker").exists());
        assert!(!root.join("releases/0.3.0").exists());
        assert!(outside.join("forge-worker").exists());
        assert!(root.join("releases/.odd/forge-worker").exists());
        // Another service's slot in a release folder keeps the folder.
        slots.settle("forge-worker", "0.4.0").unwrap();
        assert_eq!(slots.prune("forge-worker"), ["0.1.0"]);
        assert!(slots.slot("0.1.0", "http-gateway").exists());
        assert!(!slots.record_path("0.1.0", "forge-worker").exists());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&outside);
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
            restart: false,
            step: Step::Reverting,
        };
        assert_eq!(Journal::decode(&j.encode()).unwrap(), j);
        assert!(Journal::decode(&j.encode().replace("0.2.0+b1", "../../x")).is_err());
        for bad in ["", ".", "..", ".hidden", "a/b", "a b"] {
            assert!(!safe_name(bad), "{bad}");
        }
        assert!(safe_name("0.1.0-rc.1"));
    }

    /// A runner whose status and restart answers are set per test.
    struct Scripted(BTreeMap<String, Status>);
    impl Runner for Scripted {
        fn status(&self, s: &str) -> Status {
            self.0.get(s).cloned().unwrap_or(Status::Stopped)
        }
        fn restart(&self, _: &str) -> Result<Restarted, String> {
            Ok(Restarted::Ready)
        }
    }

    #[test]
    fn reports_carry_the_running_version_and_are_healthy_only_on_the_linked_one() {
        let (slots, _, _, _, root) = two_slots("sample");
        slots.link("forge-worker", "0.2.0").unwrap();
        let services = vec!["forge-worker".to_string()];
        let busy = Busy::default();
        let report = |st: Status, busy: &Busy| {
            let runner = Scripted([("forge-worker".to_string(), st)].into());
            let (r, paused) = sample(&slots, &runner, &services, busy);
            (r[0].version.clone(), r[0].healthy, paused)
        };
        // Linked 0.2.0 but the old process still runs 0.1.0: reported as 0.1.0, unhealthy.
        assert_eq!(
            report(Status::Ready("0.1.0".into()), &busy),
            ("0.1.0".into(), false, false)
        );
        assert_eq!(
            report(Status::Ready("0.2.0".into()), &busy),
            ("0.2.0".into(), true, false)
        );
        assert_eq!(
            report(Status::Unready("0.2.0".into()), &busy),
            ("0.2.0".into(), false, false)
        );
        assert_eq!(
            report(Status::Stopped, &busy),
            ("0.2.0".into(), false, false)
        );
        assert_eq!(report(Status::Paused, &busy), ("0.2.0".into(), false, true));
        busy.lock().unwrap().insert("forge-worker".into());
        assert_eq!(
            report(Status::Ready("0.2.0".into()), &busy),
            ("0.2.0".into(), false, false)
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_command_runner_reads_status_lines_and_restart_exit_codes() {
        assert_eq!(
            parse_status("noise\nready 0.2.0\n"),
            Status::Ready("0.2.0".into())
        );
        assert_eq!(parse_status("unready -"), Status::Unready("-".into()));
        assert_eq!(parse_status("paused"), Status::Paused);
        assert_eq!(parse_status("stopped"), Status::Stopped);
        assert_eq!(parse_status("what"), Status::Stopped);

        let dir = scratch("runner");
        let script = dir.join("runner.sh");
        fs::write(
            &script,
            "#!/bin/sh\ncase $1-$2 in\n status-a) echo 'ready 1.0' ;;\n restart-a) exit 0 ;;\n \
             restart-b) exit 3 ;;\n restart-c) echo 'c failed'; exit 1 ;;\n *) exit 1 ;;\nesac\n",
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        let r = CommandRunner::new(&script.display().to_string()).unwrap();
        assert_eq!(r.status("a"), Status::Ready("1.0".into()));
        assert_eq!(r.status("b"), Status::Stopped);
        assert_eq!(r.restart("a"), Ok(Restarted::Ready));
        assert_eq!(r.restart("b"), Ok(Restarted::LeftStopped));
        let e = r.restart("c").unwrap_err();
        assert!(e.contains("c failed"), "{e}");
        assert!(CommandRunner::new("  ").is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn transport_failures_are_told_apart_from_verdicts() {
        let e = |code, m: &str| CallError {
            code,
            message: m.into(),
        };
        assert!(is_transport(&e(
            None,
            "cannot connect to 127.0.0.1:1: refused"
        )));
        assert!(is_transport(&e(
            None,
            "127.0.0.1:1 closed the channel: Eof"
        )));
        assert!(!is_transport(&e(
            None,
            "forge-worker@0.1.0 does not verify here: x"
        )));
        assert!(!is_transport(&e(None, "the reply is not JSON")));
        assert!(!is_transport(&e(
            Some(crate::outcome::NOT_FOUND),
            "cannot connect"
        )));
    }

    #[test]
    fn one_agent_per_suite_root() {
        let root = scratch("lock");
        let held = lock_root(&root).unwrap();
        let e = lock_root(&root).unwrap_err();
        assert!(e.contains("another bifrost-agent is running"), "{e}");
        drop(held);
        assert!(lock_root(&root).is_ok());
        let _ = fs::remove_dir_all(&root);
    }
}
