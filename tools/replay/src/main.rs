//! `midgard-replay` — plays a recording back on a loop. [design §2]
use midgard_replay::player::{log, Player, SystemClock};
use midgard_replay::recording::{Event, Recording};
use midgard_replay::sinks::LiveSink;
use service_runtime::Args;
use std::path::{Path, PathBuf};
use std::time::Duration;

const USAGE: &str = "usage: midgard-replay --recording DIR --identity-dir DIR --forge-dir DIR \
                     [--addr HOST:PORT] [--source-bin PATH] [--loops N]";

/// Print `message` (with the usage when `usage`) and exit 2.
fn fail(message: &str, usage: bool) -> ! {
    if usage {
        eprintln!("midgard-replay: {message}\n{USAGE}");
    } else {
        eprintln!("midgard-replay: {message}");
    }
    std::process::exit(2)
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = Args::parse(
        &argv,
        &[
            "recording",
            "identity-dir",
            "forge-dir",
            "addr",
            "source-bin",
            "loops",
        ],
        &[],
    )
    .unwrap_or_else(|e| fail(&e, true));
    let need = |k: &str| {
        args.require(k)
            .map(str::to_string)
            .unwrap_or_else(|e| fail(&e, true))
    };
    let (rec_dir, ids, forge_dir) = (need("recording"), need("identity-dir"), need("forge-dir"));
    let loops = args.get("loops").map(|n| {
        n.parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or_else(|| fail("--loops must be a positive number", true))
    });
    let source_bin = args
        .get("source-bin")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("heimdall-source")))
                .unwrap_or_else(|| "heimdall-source".into())
        });
    if !Path::new(&forge_dir).is_dir() {
        fail(
            &format!("--forge-dir {forge_dir} is not a directory"),
            false,
        );
    }
    let rec = Recording::load(Path::new(&rec_dir)).unwrap_or_else(|e| fail(&e, false));
    let mut missing: Vec<&str> = rec
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Batch { feed, .. } => Some(feed.as_str()),
            _ => None,
        })
        .filter(|f| !Path::new(&ids).join(format!("{f}.cert")).exists())
        .collect();
    missing.sort();
    missing.dedup();
    if !Path::new(&ids).join("mesh.anchor").exists() {
        fail(&format!("missing mesh.anchor in identity dir {ids}"), false);
    }
    if !missing.is_empty() {
        fail(
            &format!("missing identity for feed {}", missing.join(", ")),
            false,
        );
    }
    let sink = LiveSink {
        source_bin,
        identity_dir: ids,
        addr: args.get("addr").unwrap_or("127.0.0.1:7730").to_string(),
        forge_dir: forge_dir.into(),
        timeout: Duration::from_secs(120),
    };
    log(
        "replay started",
        &[
            ("events", rec.events.len().to_string()),
            ("duration", rec.duration.to_string()),
        ],
    );
    Player {
        rec: &rec,
        clock: &SystemClock,
        sink: &sink,
    }
    .run(loops);
}
