//! `heimdall-source` — a feed source: signs an observation with its certified mesh key
//! and submits it to `heimdall-feed` over PQ-mTLS; or, as an analyst, reads the fused
//! picture. [FR-H-3] [NFR-PQ-3] [NFR-SEC-1]
//!
//! ```text
//! heimdall-source --identity-dir DIR --addr HOST:PORT --kind K --entity KEY
//!                 [--attrs k=v,...] [--observed-at T] [--observation-id ID]
//!                 [--service NAME] [--feed-id FEED] [--purpose P]
//! heimdall-source --identity-dir DIR --addr HOST:PORT --picture [--service NAME] [--purpose P]
//! heimdall-source --identity-dir DIR --addr HOST:PORT --batch FILE|- [--service NAME]
//!                 [--feed-id FEED] [--purpose P]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME defaults
//! to `feed-source`). The observation's feed is NAME — `--feed-id` defaults to it, and
//! the gateway refuses any other. `--observed-at` defaults to now. Exits 1 when the
//! gateway refuses.
//!
//! `--batch` submits many observations over one PQ-mTLS session, one JSON object per
//! line of FILE (or standard input for `-`):
//!
//! ```text
//! {"entity": "406A9F", "kind": "air-track", "observedAt": 1792169458, "id": "optional",
//!  "attrs": {"callsign": "BAW187", "squawk": "0123", "speed": 446.6, "on_ground": false}}
//! ```
//!
//! Attributes keep their JSON types: a string stays text (so `"0123"` keeps its zero),
//! a whole number is an integer, another number a float, `true`/`false` a boolean.
//! `observedAt` defaults to now and `id` to `FEED-ENTITY-OBSERVEDAT`. It prints one
//! summary line and exits 1 if any observation was refused or a line was unreadable.

use heimdall_feed_gateway::{encode_signed_observation, sign_observation};
use heimdall_feed_server::{outcome, parse_attrs, INGEST, READ_PICTURE};
use heimdall_picture::{Attr, FeedId, Observation};
use pqc_agility::backend::fips::FipsSig;
use service_runtime::json::{parse, Json};
use service_runtime::{client, unix_seconds, Args, ServiceIdentity};

const USAGE: &str = "usage: heimdall-source --identity-dir DIR --addr HOST:PORT \
                     (--kind K --entity KEY [--attrs k=v,...] [--observed-at T] \
                     [--observation-id ID] [--feed-id FEED] | --picture | --batch FILE) \
                     [--service NAME] [--purpose P]";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(
        &argv,
        &[
            "identity-dir",
            "addr",
            "service",
            "purpose",
            "kind",
            "entity",
            "attrs",
            "observed-at",
            "observation-id",
            "feed-id",
            "batch",
        ],
        &["picture"],
    ) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("heimdall-source: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    match run(&args) {
        Ok(report) => println!("{report}"),
        Err(e) => {
            eprintln!("heimdall-source: {e}");
            std::process::exit(1);
        }
    }
}

fn run(args: &Args) -> Result<String, String> {
    let service = args.get("service").unwrap_or("feed-source");
    let identity = ServiceIdentity::load(args.require("identity-dir")?, service)?;
    let addr = args.require("addr")?;
    let purpose = args.get("purpose").unwrap_or("situational-awareness");

    if args.flag("picture") {
        return answer(&client::call(&identity, addr, purpose, READ_PICTURE, &[])?);
    }

    let feed = args.get("feed-id").unwrap_or(service);
    if let Some(path) = args.get("batch") {
        return batch(&identity, addr, purpose, feed, path);
    }
    let entity = args.require("entity")?;
    let observed_at = match args.get("observed-at") {
        Some(t) => t
            .parse::<u64>()
            .map_err(|_| format!("--observed-at must be a whole number, not '{t}'"))?,
        None => unix_seconds(),
    };
    let id = args
        .get("observation-id")
        .map_or_else(|| format!("{feed}-{entity}-{observed_at}"), str::to_string);
    let mut observation = Observation::new(
        id,
        FeedId(feed.to_string()),
        args.require("kind")?,
        entity,
        observed_at,
    );
    for (key, value) in parse_attrs(args.get("attrs").unwrap_or(""))? {
        observation = observation.with_attr(key, value);
    }

    let scheme = FipsSig::new(identity.cert.identity.sig_alg);
    let signed = sign_observation(&scheme, &identity.signing_key, observation)
        .map_err(|e| format!("cannot sign the observation: {e}"))?;
    answer(&client::call(
        &identity,
        addr,
        purpose,
        INGEST,
        &encode_signed_observation(&signed),
    )?)
}

/// How many refusals a batch reports line by line before it only counts them.
const REPORTED_REFUSALS: usize = 20;

/// Sign and submit every observation in the JSON Lines `path` over one session.
fn batch(
    identity: &ServiceIdentity,
    addr: &str,
    purpose: &str,
    feed: &str,
    path: &str,
) -> Result<String, String> {
    let text = if path == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
            .map_err(|e| format!("cannot read standard input: {e}"))?;
        text
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("cannot read {path}: {e}"))?
    };
    let scheme = FipsSig::new(identity.cert.identity.sig_alg);
    let mut session = client::Session::open(identity, addr, purpose)?;
    let (mut accepted, mut refused, mut unreadable) = (0usize, 0usize, 0usize);
    let started = std::time::Instant::now();
    for (n, line) in text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        let observation = match observation_from_json(line, feed) {
            Ok(o) => o,
            Err(e) => {
                unreadable += 1;
                eprintln!("heimdall-source: line {}: {e}", n + 1);
                continue;
            }
        };
        let signed = sign_observation(&scheme, &identity.signing_key, observation)
            .map_err(|e| format!("cannot sign the observation on line {}: {e}", n + 1))?;
        let payload = encode_signed_observation(&signed);
        // A service may close a long session; reopen once and resend that line.
        let response = match session.call(INGEST, &payload) {
            Ok(r) => r,
            Err(_) => {
                session = client::Session::open(identity, addr, purpose)?;
                session.call(INGEST, &payload)?
            }
        };
        match answer(&response) {
            Ok(_) => accepted += 1,
            Err(e) => {
                refused += 1;
                if refused <= REPORTED_REFUSALS {
                    eprintln!("heimdall-source: line {}: {e}", n + 1);
                }
            }
        }
    }
    let summary = format!(
        "batch: {accepted} accepted, {refused} refused, {unreadable} unreadable in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    if refused + unreadable > 0 {
        Err(summary)
    } else {
        Ok(summary)
    }
}

/// One batch line as an observation of `feed`.
fn observation_from_json(line: &str, feed: &str) -> Result<Observation, String> {
    let doc = parse(line)?;
    let Json::Obj(pairs) = &doc else {
        return Err("a line must be a JSON object".into());
    };
    for (key, _) in pairs {
        if !["entity", "kind", "observedAt", "id", "attrs"].contains(&key.as_str()) {
            return Err(format!("unknown key \"{key}\""));
        }
    }
    let entity = doc
        .str_field("entity")
        .filter(|s| !s.is_empty())
        .ok_or("\"entity\" must be a non-empty string")?;
    let kind = doc
        .str_field("kind")
        .filter(|s| !s.is_empty())
        .ok_or("\"kind\" must be a non-empty string")?;
    let observed_at = match doc.get("observedAt") {
        None => unix_seconds(),
        Some(Json::Int(t)) if *t >= 0 => *t as u64,
        Some(_) => return Err("\"observedAt\" must be a whole number of seconds".into()),
    };
    let id = match doc.get("id") {
        None => format!("{feed}-{entity}-{observed_at}"),
        Some(Json::Str(s)) if !s.is_empty() => s.clone(),
        Some(_) => return Err("\"id\" must be a non-empty string".into()),
    };
    let mut observation = Observation::new(id, FeedId(feed.to_string()), kind, entity, observed_at);
    match doc.get("attrs") {
        None => {}
        Some(Json::Obj(attrs)) => {
            for (key, value) in attrs {
                let attr = match value {
                    Json::Str(s) => Attr::Text(s.clone()),
                    Json::Int(i) => Attr::Int(*i),
                    Json::Num(f) => Attr::Float(*f),
                    Json::Bool(b) => Attr::Bool(*b),
                    Json::Null => continue,
                    _ => {
                        return Err(format!(
                            "attribute \"{key}\" must be a string, number or boolean"
                        ))
                    }
                };
                observation = observation.with_attr(key.clone(), attr);
            }
        }
        Some(_) => return Err("\"attrs\" must be an object".into()),
    }
    Ok(observation)
}

/// The gateway's message on success; its refusal as an error.
fn answer(response: &[u8]) -> Result<String, String> {
    let (code, body) = response
        .split_first()
        .ok_or_else(|| "the feed gateway sent an empty response".to_string())?;
    let message = String::from_utf8_lossy(body).into_owned();
    match *code {
        outcome::ACCEPTED => Ok(message),
        outcome::DENIED => Err(format!("denied: {message}")),
        outcome::MALFORMED => Err(format!("malformed: {message}")),
        _ => Err(format!("failed: {message}")),
    }
}
