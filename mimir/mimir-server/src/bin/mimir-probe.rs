//! `mimir-probe` — one inference call to a running `mimir-server`, over PQ-mTLS, as a
//! certified mesh workload. It proves the service answers the way a product would reach
//! it, and exits non-zero if it does not. [FR-MI-8] [NFR-SEC-1] [RC-33]
//!
//! ```text
//! mimir-probe --identity-dir DIR --addr HOST:PORT --purpose P
//!             [--service NAME] [--prompt TEXT] [--max-tokens N]
//! ```
//!
//! `--identity-dir` holds what `mg-admin mesh issue --service NAME` wrote (NAME
//! defaults to `mimir-probe`); policy must let that identity's roles open the channel
//! and `Infer`.

use std::collections::BTreeSet;
use std::sync::Arc;

use mimir_bridge::{Inference, MimirEndpoint, PqMtlsClient};
use model_server::{InferenceRequest, WorkloadClass};
use policy::ClassificationLevel;
use pq_mtls::Posture;
use service_runtime::{Args, ServiceIdentity};

const USAGE: &str = "usage: mimir-probe --identity-dir DIR --addr HOST:PORT --purpose P \
                     [--service NAME] [--prompt TEXT] [--max-tokens N]";

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match Args::parse(
        &argv,
        &[
            "identity-dir",
            "addr",
            "purpose",
            "service",
            "prompt",
            "max-tokens",
        ],
        &[],
    ) {
        Ok(args) => args,
        Err(e) => {
            eprintln!("mimir-probe: {e}\n{USAGE}");
            std::process::exit(2);
        }
    };
    match run(&args) {
        Ok(report) => println!("{report}"),
        Err(e) => {
            eprintln!("mimir-probe: {e}");
            std::process::exit(1);
        }
    }
}

fn run(args: &Args) -> Result<String, String> {
    let service = args.get("service").unwrap_or("mimir-probe");
    let max_tokens = match args.get("max-tokens") {
        Some(n) => n
            .parse::<u32>()
            .map_err(|_| format!("--max-tokens must be a whole number, not '{n}'"))?,
        None => 16,
    };
    let identity = ServiceIdentity::load(args.require("identity-dir")?, service)?;
    let purpose = args.require("purpose")?.to_string();
    let signing_key = Arc::try_unwrap(identity.signing_key)
        .map_err(|_| "the identity key is shared".to_string())?;

    let client = PqMtlsClient::new(MimirEndpoint {
        addr: args.require("addr")?.to_string(),
        anchor: identity.anchor,
        cert: identity.cert,
        signing_key,
        purpose: purpose.clone(),
        posture: Posture {
            level: ClassificationLevel::UNCLASSIFIED,
            compartments: BTreeSet::new(),
        },
    });
    let completion = client
        .infer(InferenceRequest {
            purpose,
            workload_class: WorkloadClass::Balanced,
            quant_pref: None,
            backend_override: None,
            max_tokens,
            prompt: args.get("prompt").unwrap_or("ping").to_string(),
        })
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "model={} locality={:?} audit_seq={} first_token_ms={} tokens={}\n{}",
        completion.model_id,
        completion.locality,
        completion.audit_seq,
        completion.first_token_ms,
        completion.completion_tokens,
        completion.text
    ))
}
