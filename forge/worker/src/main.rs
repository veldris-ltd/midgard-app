//! `forge-worker` — Forge jobs as a running service. [FR-F-2] [FR-F-3] [FR-F-4] [RC-11]
//! [RC-33] [NFR-SEC-1]
//!
//! ```text
//! forge-worker --identity-dir DIR --state-dir DIR --jobs FILE --gateway HOST:PORT
//!              [--service NAME] [--gateway-service NAME] [--purpose P] [--secrets-dir DIR]
//!              [--egress-routes FILE] [--health-bind ADDR] [--once]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME defaults
//! to `forge-worker`). `--state-dir` holds the worker's sealed dataset store
//! (`datasets/`), its checkpoints (`checkpoints.journal`) and audit ledger
//! (`worker.ledger`), and is locked while it runs. `--jobs` is the jobs file (see the
//! crate docs); the secrets its sources name are files in `--secrets-dir`. With
//! `--egress-routes`, every network source must be one of the egress broker's local
//! ports, or the worker refuses to start. Records are synced to the enclosure's gateway
//! mesh door at `--gateway`, declared for `--purpose` (default `data-integration`). The
//! door must hold the mesh identity `--gateway-service` (default `http-gateway`); any
//! other is refused before records are sent.
//!
//! Every job runs at start and then every `every` seconds, in the order the file
//! declares them; a run that fails is retried after at most ten seconds. A run resumes
//! from the job's checkpoint, and the new checkpoint is recorded once the gateway has
//! accepted the records. Each run is audited and logged as one JSON line. With `--once`,
//! every job runs once and the worker exits, 1 if any failed. `/healthz`, `/readyz` and
//! `/metrics` are served on `--health-bind` when given.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fabric::LedgerStore;
use forge_connector_sdk::{CredentialResolver, NoCredentials};
use forge_dataset_store::DurableDatasetStore;
use forge_worker::{parse_jobs, run_job, Checkpoints, Job, SecretsDir};
use governance::Governance;
use kms::TrustRoot;
use policy::{DefaultPdp, PolicySet};
use service_runtime::{client, spawn_health_port, unix_seconds, Args, DirLock, ServiceIdentity};
use telemetry::{HealthRegistry, Logger, MetricsRegistry, StdoutJsonSink, Toggle};

const USAGE: &str = "usage: forge-worker --identity-dir DIR --state-dir DIR --jobs FILE \
                     --gateway HOST:PORT [--service NAME] [--gateway-service NAME] \
                     [--purpose P] [--secrets-dir DIR] \
                     [--egress-routes FILE] [--health-bind ADDR] [--once]";

/// The id of the worker's audit ledger.
const LEDGER_ID: &str = "forge-worker-audit";
/// The gateway method records are synced with (`http_gateway::mesh_sync::SYNC_RECORDS`).
const SYNC_RECORDS: &str = "SyncRecords";
/// The gateway's outcome byte for an applied sync.
const ACCEPTED: u8 = 0;
/// Seconds before a failed job runs again (or its interval, if that is shorter).
const RETRY_AFTER_FAILURE: u64 = 10;

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(
        &argv,
        &[
            "identity-dir",
            "state-dir",
            "jobs",
            "gateway",
            "gateway-service",
            "service",
            "purpose",
            "health-bind",
            "secrets-dir",
            "egress-routes",
        ],
        &["once"],
    ) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("forge-worker: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(&args) {
        eprintln!("forge-worker: {e}");
        std::process::exit(1);
    }
}

fn run(args: &Args) -> Result<(), String> {
    let service = args.get("service").unwrap_or("forge-worker");
    let state_dir = Path::new(args.require("state-dir")?);
    let jobs_path = Path::new(args.require("jobs")?);
    let gateway = args.require("gateway")?;
    let purpose = args.get("purpose").unwrap_or("data-integration");
    let gateway_service = args.get("gateway-service").unwrap_or("http-gateway");

    let jobs_text = std::fs::read_to_string(jobs_path)
        .map_err(|e| format!("cannot read {}: {e}", jobs_path.display()))?;
    let base = jobs_path.parent().unwrap_or(Path::new("."));
    let jobs = parse_jobs(&jobs_text, base).map_err(|e| format!("{}: {e}", jobs_path.display()))?;
    if let Some(routes_path) = args.get("egress-routes") {
        let routes_text = std::fs::read_to_string(routes_path)
            .map_err(|e| format!("cannot read {routes_path}: {e}"))?;
        let routes = forge_worker::egress::parse_routes(&routes_text)
            .map_err(|e| format!("{routes_path}: {e}"))?;
        forge_worker::egress::check_jobs(&jobs, &routes)?;
    }

    let metrics = Arc::new(MetricsRegistry::new());
    metrics.describe(
        "forge_job_runs_total",
        "Forge job runs, by job and outcome.",
    );
    if let Some(bind) = args.get("health-bind") {
        let mut health = HealthRegistry::new();
        health.register(Box::new(Toggle::new("jobs", true)));
        spawn_health_port(bind, Arc::new(health), metrics.clone())
            .map_err(|e| format!("cannot bind the health port: {e}"))?;
    }

    std::fs::create_dir_all(state_dir)
        .map_err(|e| format!("cannot create {}: {e}", state_dir.display()))?;
    let _lock = DirLock::acquire(state_dir, "worker.lock", service)?;
    let identity = ServiceIdentity::load(args.require("identity-dir")?, service)?;
    let writer = identity.audit_writer(state_dir.join("worker.ledger"), LEDGER_ID)?;
    // The worker decides nothing itself — the gateway authorises every sync — so its
    // governance holds no rules and is used only to record runs.
    let no_rules = PolicySet::new(Vec::new()).map_err(|e| format!("{e:?}"))?;
    let mut audit = Governance::with_audit(DefaultPdp::new(no_rules), writer);
    let mut store = DurableDatasetStore::open(state_dir.join("datasets"), identity.data_kms()?)
        .map_err(|e| format!("the dataset store does not open: {e:?}"))?;
    let mut checkpoints = Checkpoints::open(
        state_dir.join("checkpoints.journal"),
        identity.data_sealer()?,
    )?;
    let credentials: Box<dyn CredentialResolver> = match args.get("secrets-dir") {
        Some(dir) => Box::new(SecretsDir(dir.into())),
        None => Box::new(NoCredentials),
    };

    let worker = Worker {
        identity: &identity,
        gateway,
        gateway_service,
        purpose,
        service,
        metrics: &metrics,
        credentials: credentials.as_ref(),
    };
    let mut synced: Vec<Option<String>> = vec![None; jobs.len()];

    if args.flag("once") {
        let mut failed = 0;
        for (job, last) in jobs.iter().zip(synced.iter_mut()) {
            if !worker.run(job, &mut store, &mut audit, last, &mut checkpoints) {
                failed += 1;
            }
        }
        return if failed == 0 {
            Ok(())
        } else {
            Err(format!("{failed} job(s) failed"))
        };
    }

    let mut due = vec![0u64; jobs.len()];
    loop {
        for (i, job) in jobs.iter().enumerate() {
            if unix_seconds() >= due[i] {
                let ok = worker.run(
                    job,
                    &mut store,
                    &mut audit,
                    &mut synced[i],
                    &mut checkpoints,
                );
                // A failed run — the gateway not reachable yet, or a link to an object
                // another job has not synced yet — is retried soon, not a whole
                // interval later.
                let wait = if ok {
                    job.every
                } else {
                    job.every.min(RETRY_AFTER_FAILURE)
                };
                due[i] = unix_seconds().saturating_add(wait);
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

struct Worker<'a> {
    identity: &'a ServiceIdentity,
    gateway: &'a str,
    gateway_service: &'a str,
    purpose: &'a str,
    service: &'a str,
    metrics: &'a MetricsRegistry,
    credentials: &'a dyn CredentialResolver,
}

impl Worker<'_> {
    /// Run one job from its checkpoint and sync its records — unless there are none, or a
    /// source read in full yields a version already synced; then record the new
    /// checkpoint, only once the gateway has accepted the records. Audit, count and log
    /// the outcome. Returns whether the run succeeded.
    fn run<R: TrustRoot, L: LedgerStore>(
        &self,
        job: &Job,
        store: &mut DurableDatasetStore<R>,
        audit: &mut Governance<L>,
        synced: &mut Option<String>,
        checkpoints: &mut Checkpoints,
    ) -> bool {
        let logger = Logger::new(self.service, &StdoutJsonSink);
        let now = unix_seconds();
        let resume = checkpoints.get(&job.id).map(str::to_string);
        let result = run_job(job, store, self.credentials, resume.as_deref()).and_then(|run| {
            if run.objects == 0 && run.links == 0 {
                return Ok((run, "nothing new", "the source had no new rows".to_string()));
            }
            if run.checkpoint.is_none() && synced.as_deref() == Some(run.version.as_str()) {
                return Ok((run, "unchanged", "already synced".to_string()));
            }
            let response = client::call(
                self.identity,
                self.gateway,
                self.gateway_service,
                self.purpose,
                SYNC_RECORDS,
                run.records.as_bytes(),
            )?;
            let (code, body) = response
                .split_first()
                .ok_or_else(|| "the gateway sent an empty response".to_string())?;
            let message = String::from_utf8_lossy(body).into_owned();
            if *code != ACCEPTED {
                return Err(format!("the gateway refused the sync: {message}"));
            }
            *synced = Some(run.version.clone());
            Ok((run, "synced", message))
        });
        let result = result.and_then(|(run, outcome, message)| {
            if let Some(checkpoint) = &run.checkpoint {
                checkpoints.record(&job.id, checkpoint)?;
            }
            Ok((run, outcome, message))
        });

        match result {
            Ok((run, outcome, message)) => {
                let short = run.version.get(..16).unwrap_or(&run.version);
                let recorded = audit.record_event(
                    self.service,
                    "RUN forge job",
                    &format!("job:{}:{short}:{outcome}", job.id),
                    now,
                );
                self.metrics.inc(
                    "forge_job_runs_total",
                    &[("job", &job.id), ("outcome", outcome)],
                );
                logger.info(
                    "job run",
                    &[
                        ("job", &job.id),
                        ("outcome", outcome),
                        ("version", &run.version),
                        ("changed", &run.changed.to_string()),
                        ("rows", &run.rows.to_string()),
                        ("objects", &run.objects.to_string()),
                        ("links", &run.links.to_string()),
                        ("message", &message),
                    ],
                );
                if let Err(e) = recorded {
                    logger.error(
                        "job run not audited",
                        &[("job", &job.id), ("reason", &format!("{e:?}"))],
                    );
                    return false;
                }
                true
            }
            Err(reason) => {
                let _ = audit.record_event(
                    self.service,
                    "RUN forge job failed",
                    &format!("job:{}", job.id),
                    now,
                );
                self.metrics.inc(
                    "forge_job_runs_total",
                    &[("job", &job.id), ("outcome", "failed")],
                );
                logger.error("job run failed", &[("job", &job.id), ("reason", &reason)]);
                false
            }
        }
    }
}
