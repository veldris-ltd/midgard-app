//! `heimdall-source` — a feed source: signs an observation with its certified mesh key
//! and submits it to `heimdall-feed` over PQ-mTLS; or, as an analyst, reads the fused
//! picture. [FR-H-3] [NFR-PQ-3] [NFR-SEC-1]
//!
//! ```text
//! heimdall-source --identity-dir DIR --addr HOST:PORT --kind K --entity KEY
//!                 [--attrs k=v,...] [--observed-at T] [--observation-id ID]
//!                 [--service NAME] [--server NAME] [--feed-id FEED] [--purpose P]
//! heimdall-source --identity-dir DIR --addr HOST:PORT --picture [--service NAME]
//!                 [--server NAME] [--purpose P]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME defaults
//! to `feed-source`). The gateway at `--addr` must hold the mesh identity `--server`
//! (default `heimdall-feed`); any other is refused before anything is sent. The observation's feed is NAME — `--feed-id` defaults to it, and
//! the gateway refuses any other. `--observed-at` defaults to now. Exits 1 when the
//! gateway refuses.

use heimdall_feed_gateway::{encode_signed_observation, sign_observation};
use heimdall_feed_server::{outcome, parse_attrs, INGEST, READ_PICTURE};
use heimdall_picture::{FeedId, Observation};
use pqc_agility::backend::fips::FipsSig;
use service_runtime::{client, unix_seconds, Args, ServiceIdentity};

const USAGE: &str = "usage: heimdall-source --identity-dir DIR --addr HOST:PORT \
                     (--kind K --entity KEY [--attrs k=v,...] [--observed-at T] \
                     [--observation-id ID] [--feed-id FEED] | --picture) \
                     [--service NAME] [--server NAME] [--purpose P]";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(
        &argv,
        &[
            "identity-dir",
            "addr",
            "server",
            "service",
            "purpose",
            "kind",
            "entity",
            "attrs",
            "observed-at",
            "observation-id",
            "feed-id",
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
    let server = args.get("server").unwrap_or("heimdall-feed");

    if args.flag("picture") {
        return answer(&client::call(
            &identity,
            addr,
            server,
            purpose,
            READ_PICTURE,
            &[],
        )?);
    }

    let feed = args.get("feed-id").unwrap_or(service);
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
        server,
        purpose,
        INGEST,
        &encode_signed_observation(&signed),
    )?)
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
