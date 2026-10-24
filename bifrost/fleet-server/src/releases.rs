//! The orchestrator's state at rest, its rings file, and its JSON. [FR-B-2] [RC-12]
//!
//! ## At rest
//! The whole [`Rollouts`] state (releases, channels, runs with each ring's phase and
//! timestamps) is small, so it is kept as one snapshot, `<state>/releases.sealed`,
//! sealed with the service's data sealer (AES-256-GCM under an ML-KEM-1024-wrapped key,
//! as the fleet journal and the registry) under a fixed AAD. Every change is written
//! before it takes effect in memory: the new state is encoded, sealed, written to a
//! temporary file, `fsync`ed, renamed over the old one, and the directory `fsync`ed. A
//! crash leaves the old snapshot or the new one, never a mix. On open a snapshot that
//! does not unseal or decode fails the start closed. A run that was in progress resumes
//! where it was: its rings, deadlines and soak start are in the snapshot, while the
//! agents' reports are not kept, so a ring that was soaking waits for fresh reports
//! (see `bifrost_rollout::runs`).
//!
//! A snapshot replaced by an older one that was sealed by the same service would open:
//! the seal proves origin and integrity, not freshness. That is the same exposure as a
//! truncated fleet journal, and needs write access to the service's state directory.
//!
//! ## Rings
//! `<state>/rings.json` (optional; without it [`RingConfig::suite_default`]):
//!
//! ```json
//! { "version": 1, "soakSecs": 60, "readyTimeoutSecs": 180, "reportTtlSecs": 30,
//!   "rings": [ { "name": "canary", "services": ["forge-worker"] },
//!              { "name": "last", "services": ["http-gateway"], "soakSecs": 120 } ] }
//! ```
//!
//! The top-level times are defaults for every ring, each optional; a ring may override
//! them. A run keeps the rings it started with, so a change applies to the next run.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use audit_writer::Sealer;
use bifrost_rollout::runs::{
    ChannelState, Release, RingConfig, RingPhase, RingRun, RingSpec, RolloutRun, Rollouts, RunKind,
    RunState, DEFAULT_READY_TIMEOUT_SECS, DEFAULT_REPORT_TTL_SECS, DEFAULT_SOAK_SECS,
};
use service_runtime::json::{parse, Json};
use service_runtime::strict_json::{array, no_repeated_keys, object, only_keys, strings, text};

const SNAPSHOT_AAD: &[u8] = b"midgard-bifrost-releases-v1";
const FORMAT: i64 = 1;

/// The orchestrator's state, sealed at rest.
pub struct ReleaseStore {
    state: Rollouts,
    path: PathBuf,
    sealer: Box<dyn Sealer>,
}

impl ReleaseStore {
    /// Open the snapshot at `path`, or start empty when there is none. A snapshot that
    /// does not unseal or decode is an error.
    pub fn open(path: impl AsRef<Path>, sealer: Box<dyn Sealer>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let state = match fs::read(&path) {
            Ok(sealed) => {
                let plain = sealer
                    .unseal(SNAPSHOT_AAD, &sealed)
                    .map_err(|e| format!("{} does not unseal: {e}", path.display()))?;
                let text = String::from_utf8(plain)
                    .map_err(|_| format!("{} is not UTF-8 inside", path.display()))?;
                decode(&text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Rollouts::new(),
            Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
        };
        Ok(Self {
            state,
            path,
            sealer,
        })
    }

    /// The state as last committed.
    pub fn state(&self) -> &Rollouts {
        &self.state
    }

    /// Write `next` durably, then make it the state. On an error nothing changed.
    pub fn commit(&mut self, next: Rollouts) -> Result<(), String> {
        if next == self.state {
            return Ok(());
        }
        let sealed = self
            .sealer
            .seal(SNAPSHOT_AAD, encode(&next).as_bytes())
            .map_err(|e| format!("cannot seal the release state: {e}"))?;
        write_atomic(&self.path, &sealed)?;
        self.state = next;
        Ok(())
    }

    /// The snapshot's path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let tmp = path.with_extension("sealed.tmp");
    let fail = |what: &str, e: std::io::Error| format!("cannot {what} {}: {e}", tmp.display());
    let mut opts = OpenOptions::new();
    opts.create(true).write(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp).map_err(|e| fail("create", e))?;
    file.write_all(bytes).map_err(|e| fail("write", e))?;
    file.sync_all().map_err(|e| fail("sync", e))?;
    drop(file);
    fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))?;
    if let Some(dir) = path.parent() {
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
}

// --- the rings file ---------------------------------------------------------------

/// Parse a rings file strictly (see the module docs) and validate it.
pub fn parse_rings(text_in: &str) -> Result<RingConfig, String> {
    let doc = parse(text_in).map_err(|e| format!("not JSON: {e}"))?;
    only_keys(
        object(&doc, "the file")?,
        "the file",
        &[
            "version",
            "soakSecs",
            "readyTimeoutSecs",
            "reportTtlSecs",
            "rings",
        ],
    )?;
    match doc.get("version") {
        Some(Json::Int(1)) => {}
        Some(other) => return Err(format!("unsupported version {}", other.encode())),
        None => return Err("missing \"version\"".into()),
    }
    let soak = secs(&doc, "the file", "soakSecs", DEFAULT_SOAK_SECS)?;
    let ready = secs(
        &doc,
        "the file",
        "readyTimeoutSecs",
        DEFAULT_READY_TIMEOUT_SECS,
    )?;
    let ttl = secs(&doc, "the file", "reportTtlSecs", DEFAULT_REPORT_TTL_SECS)?;
    let rings = array(&doc, "the file", "rings")?
        .iter()
        .enumerate()
        .map(|(i, ring)| {
            let at = format!("rings[{i}]");
            only_keys(
                object(ring, &at)?,
                &at,
                &["name", "services", "soakSecs", "readyTimeoutSecs"],
            )?;
            Ok(RingSpec {
                name: text(ring, &at, "name")?.to_string(),
                services: strings(ring, &at, "services")?,
                soak_secs: secs(ring, &at, "soakSecs", soak)?,
                ready_timeout_secs: secs(ring, &at, "readyTimeoutSecs", ready)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let config = RingConfig {
        rings,
        report_ttl_secs: ttl,
    };
    config.validate()?;
    Ok(config)
}

fn secs(item: &Json, at: &str, key: &str, default: u64) -> Result<u64, String> {
    match item.get(key) {
        None => Ok(default),
        Some(Json::Int(n)) if (0..=86_400).contains(n) => Ok(*n as u64),
        Some(other) => Err(format!(
            "{at}: \"{key}\" must be whole seconds from 0 to 86400, not {}",
            other.encode()
        )),
    }
}

/// The rings as JSON, in the file's shape.
pub fn rings_json(config: &RingConfig) -> Json {
    Json::obj(vec![
        ("reportTtlSecs", int(config.report_ttl_secs)),
        (
            "rings",
            Json::Arr(config.rings.iter().map(ring_spec_json).collect()),
        ),
    ])
}

fn ring_spec_json(r: &RingSpec) -> Json {
    Json::obj(vec![
        ("name", Json::s(&r.name)),
        (
            "services",
            Json::Arr(r.services.iter().map(|s| Json::s(s)).collect()),
        ),
        ("soakSecs", int(r.soak_secs)),
        ("readyTimeoutSecs", int(r.ready_timeout_secs)),
    ])
}

// --- JSON: the wire and the snapshot ---------------------------------------------

fn int(n: u64) -> Json {
    Json::Int(i64::try_from(n).unwrap_or(i64::MAX))
}

fn opt_int(n: Option<u64>) -> Json {
    n.map_or(Json::Null, int)
}

fn opt_str(s: Option<&str>) -> Json {
    s.map_or(Json::Null, Json::s)
}

/// A release as JSON: `{"version", "artefacts": {service: version}, "createdBy",
/// "createdAt"}`.
pub fn release_json(r: &Release) -> Json {
    Json::obj(vec![
        ("version", Json::s(&r.version)),
        (
            "artefacts",
            Json::Obj(
                r.artefacts
                    .iter()
                    .map(|(s, v)| (s.clone(), Json::s(v)))
                    .collect(),
            ),
        ),
        ("createdBy", Json::s(&r.created_by)),
        ("createdAt", int(r.created_at)),
    ])
}

/// A run as JSON, every ring with its phase and times.
pub fn run_json(run: &RolloutRun) -> Json {
    let rings = run
        .rings
        .iter()
        .map(|g| {
            let Json::Obj(mut fields) = ring_spec_json(&g.spec) else {
                unreachable!("an object")
            };
            fields.extend(
                [
                    ("phase", Json::s(g.phase.as_str())),
                    ("startedAt", opt_int(g.started_at)),
                    ("deadline", opt_int(g.deadline)),
                    ("soakSince", opt_int(g.soak_since)),
                    ("endedAt", opt_int(g.ended_at)),
                    ("reason", opt_str(g.reason.as_deref())),
                    (
                        "readySeen",
                        Json::Arr(g.ready_seen.iter().map(|s| Json::s(s)).collect()),
                    ),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_string(), v)),
            );
            Json::Obj(fields)
        })
        .collect();
    Json::obj(vec![
        ("id", Json::s(&run.id)),
        ("kind", Json::s(run.kind.as_str())),
        ("channel", Json::s(&run.channel)),
        ("release", Json::s(&run.release)),
        ("from", Json::s(&run.from)),
        ("state", Json::s(run.state.as_str())),
        ("startedAt", int(run.started_at)),
        ("startedBy", Json::s(&run.started_by)),
        ("endedAt", opt_int(run.ended_at)),
        ("endedBy", opt_str(run.ended_by.as_deref())),
        ("reason", opt_str(run.reason.as_deref())),
        ("reportTtlSecs", int(run.report_ttl_secs)),
        ("rings", Json::Arr(rings)),
    ])
}

fn channel_json(c: &ChannelState) -> Json {
    Json::obj(vec![
        ("name", Json::s(&c.name)),
        ("current", Json::s(&c.current)),
        ("previous", opt_str(c.previous.as_deref())),
    ])
}

/// The snapshot's plaintext.
pub fn encode(state: &Rollouts) -> String {
    Json::obj(vec![
        ("format", Json::Int(FORMAT)),
        ("nextRun", int(state.next_run)),
        (
            "releases",
            Json::Arr(state.releases.values().map(release_json).collect()),
        ),
        (
            "channels",
            Json::Arr(state.channels.values().map(channel_json).collect()),
        ),
        ("runs", Json::Arr(state.runs.iter().map(run_json).collect())),
    ])
    .encode()
}

/// Read a snapshot's plaintext back.
pub fn decode(text_in: &str) -> Result<Rollouts, String> {
    let doc = parse(text_in).map_err(|e| format!("not JSON: {e}"))?;
    if doc.get("format") != Some(&Json::Int(FORMAT)) {
        return Err("unknown snapshot format".into());
    }
    let mut state = Rollouts {
        next_run: num(&doc, "nextRun")?,
        ..Rollouts::default()
    };
    for r in array(&doc, "the snapshot", "releases")? {
        let artefacts = object(
            r.get("artefacts").ok_or("a release has no artefacts")?,
            "artefacts",
        )?;
        no_repeated_keys(artefacts, "artefacts")?;
        let release = Release {
            version: text(r, "a release", "version")?.to_string(),
            artefacts: artefacts
                .iter()
                .map(|(s, v)| {
                    v.as_str()
                        .map(|v| (s.clone(), v.to_string()))
                        .ok_or_else(|| format!("artefact {s} has no version"))
                })
                .collect::<Result<_, String>>()?,
            created_by: text(r, "a release", "createdBy")?.to_string(),
            created_at: num(r, "createdAt")?,
        };
        state.releases.insert(release.version.clone(), release);
    }
    for c in array(&doc, "the snapshot", "channels")? {
        let channel = ChannelState {
            name: text(c, "a channel", "name")?.to_string(),
            current: text(c, "a channel", "current")?.to_string(),
            previous: opt_text(c, "previous")?,
        };
        state.channels.insert(channel.name.clone(), channel);
    }
    for r in array(&doc, "the snapshot", "runs")? {
        let at = "a run";
        let mut rings = Vec::new();
        for g in array(r, at, "rings")? {
            let mut ring = RingRun {
                spec: RingSpec {
                    name: text(g, "a ring", "name")?.to_string(),
                    services: strings(g, "a ring", "services")?,
                    soak_secs: num(g, "soakSecs")?,
                    ready_timeout_secs: num(g, "readyTimeoutSecs")?,
                },
                phase: RingPhase::parse(text(g, "a ring", "phase")?)
                    .ok_or("a ring has an unknown phase")?,
                started_at: opt_num(g, "startedAt")?,
                deadline: opt_num(g, "deadline")?,
                soak_since: opt_num(g, "soakSince")?,
                ended_at: opt_num(g, "endedAt")?,
                reason: opt_text(g, "reason")?,
                ready_seen: Default::default(),
            };
            ring.ready_seen = strings(g, "a ring", "readySeen")?.into_iter().collect();
            rings.push(ring);
        }
        state.runs.push(RolloutRun {
            id: text(r, at, "id")?.to_string(),
            kind: RunKind::parse(text(r, at, "kind")?).ok_or("a run has an unknown kind")?,
            channel: text(r, at, "channel")?.to_string(),
            release: text(r, at, "release")?.to_string(),
            from: text(r, at, "from")?.to_string(),
            rings,
            report_ttl_secs: num(r, "reportTtlSecs")?,
            state: RunState::parse(text(r, at, "state")?).ok_or("a run has an unknown state")?,
            started_at: num(r, "startedAt")?,
            started_by: text(r, at, "startedBy")?.to_string(),
            ended_at: opt_num(r, "endedAt")?,
            ended_by: opt_text(r, "endedBy")?,
            reason: opt_text(r, "reason")?,
        });
    }
    Ok(state)
}

fn num(item: &Json, key: &str) -> Result<u64, String> {
    match item.get(key) {
        Some(Json::Int(n)) if *n >= 0 => Ok(*n as u64),
        _ => Err(format!("\"{key}\" must be a whole number")),
    }
}

fn opt_num(item: &Json, key: &str) -> Result<Option<u64>, String> {
    match item.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(_) => num(item, key).map(Some),
    }
}

fn opt_text(item: &Json, key: &str) -> Result<Option<String>, String> {
    match item.get(key) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Str(s)) => Ok(Some(s.clone())),
        Some(_) => Err(format!("\"{key}\" must be a string or null")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audit_writer::AuditError;
    use bifrost_rollout::runs::{InstallReport, ServiceStatus};

    /// A sealer for tests: XOR with the AAD's length and a checksum, so a wrong AAD or
    /// a flipped byte fails. Not a cipher; the real one is the KMS sealer.
    struct TestSealer;
    impl Sealer for TestSealer {
        fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, AuditError> {
            let mut out: Vec<u8> = plaintext.iter().map(|b| b ^ aad.len() as u8).collect();
            let sum: u32 = plaintext.iter().map(|&b| b as u32).sum::<u32>() ^ aad.len() as u32;
            out.extend_from_slice(&sum.to_le_bytes());
            Ok(out)
        }
        fn unseal(&self, aad: &[u8], sealed: &[u8]) -> Result<Vec<u8>, AuditError> {
            let (body, sum) = sealed.split_at(sealed.len().saturating_sub(4));
            let plain: Vec<u8> = body.iter().map(|b| b ^ aad.len() as u8).collect();
            let want: u32 = plain.iter().map(|&b| b as u32).sum::<u32>() ^ aad.len() as u32;
            if sum != want.to_le_bytes() {
                return Err(AuditError::Store("bad seal".into()));
            }
            Ok(plain)
        }
    }

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bifrost-releases-{tag}-{}-{}",
            std::process::id(),
            Box::into_raw(Box::new(0u8)) as usize
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn release(v: &str, config: &RingConfig) -> Release {
        Release {
            version: v.into(),
            artefacts: config
                .services()
                .into_iter()
                .map(|s| (s.to_string(), v.to_string()))
                .collect(),
            created_by: "rm".into(),
            created_at: 7,
        }
    }

    #[test]
    fn a_run_mid_ring_survives_a_reopen_exactly() {
        let dir = scratch("reopen");
        let path = dir.join("releases.sealed");
        let config = RingConfig::suite_default();
        let mut store = ReleaseStore::open(&path, Box::new(TestSealer)).unwrap();
        let mut next = store.state().clone();
        next.seed_channel("stable", "0.1.0").unwrap();
        next.create_release(release("0.1.0", &config), &config)
            .unwrap();
        next.create_release(release("0.2.0", &config), &config)
            .unwrap();
        next.promote("stable", "0.2.0", &config, "rm", 100).unwrap();
        let report = InstallReport {
            install_id: "mac-suite".into(),
            channel: "stable".into(),
            at: 110,
            services: [(
                "forge-worker".to_string(),
                ServiceStatus {
                    version: "0.2.0".into(),
                    healthy: true,
                },
            )]
            .into_iter()
            .collect(),
        };
        next.tick(110, &[report]);
        store.commit(next.clone()).unwrap();

        let reopened = ReleaseStore::open(&path, Box::new(TestSealer)).unwrap();
        assert_eq!(reopened.state(), &next);
        let ring = &reopened.state().runs[0].rings[0];
        assert_eq!(ring.phase, RingPhase::Soaking);
        assert_eq!(ring.soak_since, Some(110));
        assert!(ring.ready_seen.contains("mac-suite/forge-worker"));

        // A flipped byte at rest stops the open.
        let mut bytes = fs::read(&path).unwrap();
        bytes[10] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert!(ReleaseStore::open(&path, Box::new(TestSealer)).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rings_file_is_read_strictly() {
        let c = parse_rings(
            r#"{"version":1,"soakSecs":5,"rings":[
                {"name":"a","services":["x"]},
                {"name":"b","services":["y","z"],"readyTimeoutSecs":9}]}"#,
        )
        .unwrap();
        assert_eq!(c.rings[0].soak_secs, 5);
        assert_eq!(c.rings[0].ready_timeout_secs, DEFAULT_READY_TIMEOUT_SECS);
        assert_eq!(c.rings[1].ready_timeout_secs, 9);
        assert_eq!(c.report_ttl_secs, DEFAULT_REPORT_TTL_SECS);
        for bad in [
            r#"{"version":2,"rings":[{"name":"a","services":["x"]}]}"#,
            r#"{"version":1,"rings":[]}"#,
            r#"{"version":1,"rings":[{"name":"a","services":[]}]}"#,
            r#"{"version":1,"rings":[{"name":"a","services":["x"]},{"name":"b","services":["x"]}]}"#,
            r#"{"version":1,"rings":[{"name":"a","services":["x"],"extra":1}]}"#,
            r#"{"version":1,"soakSecs":-1,"rings":[{"name":"a","services":["x"]}]}"#,
            r#"{"version":1,"rings":[{"name":"a","services":["x"],"readyTimeoutSecs":0}]}"#,
        ] {
            assert!(parse_rings(bad).is_err(), "{bad}");
        }
        // The shipped file is the default.
        let shipped = include_str!("../../../deploy/compose/fleet-rings.json");
        assert_eq!(parse_rings(shipped).unwrap(), RingConfig::suite_default());
    }
}
