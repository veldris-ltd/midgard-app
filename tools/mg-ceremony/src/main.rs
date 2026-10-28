//! mg-ceremony: the release key ceremony, as one guided run. [RT-35] [AC-RF-CA-6a] [NFR-PQ-3]
//!
//! `docs/ops/KEY_CEREMONY.md` is the procedure; this does its mechanical part, so
//! that what the officers check is the output, not their typing. It
//!
//!   1. generates the release signing key (ML-DSA-87) and the Bifrost admission
//!      key (ML-DSA-65), each in a sealed keystore written to a fresh directory
//!      outside the checkout, and reads each back;
//!   2. writes their public halves as `deploy/trust/release-trust.pub` and
//!      `deploy/trust/bifrost-trust.pub`;
//!   3. writes `deploy/trust/allowed_signers` from the SSH public keys of the
//!      people who may sign a release tag, each for the namespace `git`;
//!   4. checks all three the way the release does;
//!   5. writes the ceremony record, `docs/ops/ceremonies/<date>-release-keys.md`,
//!      with the key ids, the SHA-256 of each published file and each SSH key's
//!      fingerprint, and blank lines for the two people to sign;
//!   6. prints the text to publish through a second channel, and what to commit.
//!
//! ```text
//! mg-ceremony                       the guided ceremony: asks one question at a time
//! mg-ceremony run   --custody DIR --signer PRINCIPAL=KEY.pub [--signer ...]
//!                   [--repo DIR] [--year YYYY] [--officer NAME] [--witness NAME]
//! mg-ceremony check [--repo DIR]
//! ```
//!
//! Key generation is `attest::Keystore`, the code `mg-sign keygen` runs; nothing
//! cryptographic is implemented here. It commits nothing, pushes nothing and uses
//! no network. A ceremony is held once per key generation: if any published file
//! already exists it refuses, because rotation appends a key beside the old one
//! (`deploy/trust/README.md`).
//!
//! Exit codes: `0` done, `2` usage error, `1` refused or failed.

mod ceremony;
mod clock;
mod ssh;
mod wizard;

use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;
use std::process::ExitCode;

use clock::Now;
use ssh::Signer;

const USAGE: &str = "\
usage:
  mg-ceremony                       the guided ceremony (or double-click mg-ceremony.exe)
  mg-ceremony run   --custody DIR --signer PRINCIPAL=KEY.pub [--signer ...]
                    [--repo DIR] [--year YYYY] [--officer NAME] [--witness NAME]
  mg-ceremony check [--repo DIR]

  run     hold the ceremony with every answer given as a flag.
  check   check the published files as the release does, and print the values
          to compare with the second channel.

  --repo     the Midgard checkout (default: the current directory)
  --custody  a new, empty directory outside the checkout for the two keystores
  --signer   who may sign a release tag: their email address and SSH .pub file
  --year     the year in the key ids (default: this year, UTC)

The procedure is docs/ops/KEY_CEREMONY.md.
";

/// ML-KEM-1024 and ML-DSA-87 keygen hold large arrays by value; as in mg-sign, a
/// generous stack makes an unoptimised build behave like a release one.
const STACK_SIZE: usize = 64 * 1024 * 1024;

fn main() -> ExitCode {
    std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(dispatch)
        .expect("spawn worker thread")
        .join()
        .unwrap_or(ExitCode::FAILURE)
}

fn dispatch() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        None => guided(),
        Some("run") => run(&args[1..]),
        Some("check") => check(&args[1..]),
        Some("-h" | "--help" | "help") => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some(other) => Err(Failure::Usage(format!("unknown command {other:?}"))),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(m)) => {
            eprintln!("mg-ceremony: {m}\n");
            eprint!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Failure::Fatal(m)) => {
            eprintln!("mg-ceremony: {m}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug)]
enum Failure {
    Usage(String),
    Fatal(String),
}

impl From<String> for Failure {
    fn from(m: String) -> Self {
        Failure::Fatal(m)
    }
}

/// The guided ceremony. Started by a double-click, the console window closes
/// when the program ends, so it waits for Enter before it does.
fn guided() -> Result<(), Failure> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut out = std::io::stdout();
    let env = wizard::Env {
        starts: [
            std::env::current_dir().ok(),
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(PathBuf::from)),
        ]
        .into_iter()
        .flatten()
        .collect(),
        home: home_dir(),
        ssh_keygen: find_ssh_keygen(),
        temp: std::env::temp_dir(),
        now: Now::system(),
    };
    let result = wizard::run(
        &mut wizard::Io {
            input: &mut input,
            out: &mut out,
        },
        &env,
    );
    if let Err(e) = &result {
        println!("\nmg-ceremony: {e}");
    }
    if std::io::stdin().is_terminal() {
        println!("\nPress Enter to close this window.");
        let _ = input.read_line(&mut String::new());
    }
    result.map_err(|_| Failure::Fatal("the ceremony did not complete".into()))
}

fn run(argv: &[String]) -> Result<(), Failure> {
    let args = Args::parse(
        argv,
        &["repo", "custody", "signer", "year", "officer", "witness"],
    )?;
    let custody = args
        .one("custody")?
        .ok_or_else(|| Failure::Usage("name the custody directory: --custody DIR".into()))?;
    let specs = args.all("signer");
    if specs.is_empty() {
        return Err(Failure::Usage(
            "name at least one tag signer: --signer PRINCIPAL=KEY.pub".into(),
        ));
    }
    let mut signers = Vec::new();
    for spec in specs {
        let (principal, file) = spec
            .split_once('=')
            .filter(|(p, f)| !p.is_empty() && !f.is_empty())
            .ok_or_else(|| Failure::Usage(format!("--signer {spec} is not PRINCIPAL=KEY.pub")))?;
        let text = std::fs::read_to_string(file)
            .map_err(|e| Failure::Fatal(format!("{file} is missing or unreadable: {e}")))?;
        signers.push(Signer::parse(principal, &text).map_err(|e| format!("{file}: {e}"))?);
    }
    let now = Now::system();
    let plan = ceremony::Plan {
        repo: PathBuf::from(args.one("repo")?.unwrap_or(".")),
        custody: PathBuf::from(custody),
        year: args
            .one("year")?
            .map_or_else(|| now.year.to_string(), str::to_string),
        officer: args.one("officer")?.unwrap_or_default().to_string(),
        witness: args.one("witness")?.unwrap_or_default().to_string(),
        signers,
    };
    let commit = ceremony::git_head(&plan.repo);
    let outcome = ceremony::hold(&plan, &now, commit.as_deref())?;
    let mut input = std::io::empty();
    wizard::report(
        &mut wizard::Io {
            input: &mut input,
            out: &mut std::io::stdout(),
        },
        &outcome,
        &now,
        false,
    );
    Ok(())
}

fn check(argv: &[String]) -> Result<(), Failure> {
    let args = Args::parse(argv, &["repo"])?;
    let repo = ceremony::find_repo(&PathBuf::from(args.one("repo")?.unwrap_or(".")))?;
    let published = ceremony::check_published(&repo)?;
    println!("check: the published files pass the release's checks.\n");
    print!(
        "{}",
        ceremony::second_channel_text(&published, &Now::system())
    );
    Ok(())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// `ssh-keygen` on the PATH, or where Windows installs its OpenSSH client.
fn find_ssh_keygen() -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["ssh-keygen.exe"]
    } else {
        &["ssh-keygen"]
    };
    let on_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
            .find(|p| p.is_file())
    });
    on_path.or_else(|| {
        let p = PathBuf::from(std::env::var_os("SystemRoot")?)
            .join("System32")
            .join("OpenSSH")
            .join("ssh-keygen.exe");
        p.is_file().then_some(p)
    })
}

// --- argument parsing --------------------------------------------------------

/// Strict, as in mg-sign: an unknown flag or a flag with no value is a usage
/// error. Only `--signer` may be given more than once.
struct Args {
    flags: Vec<(String, String)>,
}

impl Args {
    fn parse(argv: &[String], known: &[&str]) -> Result<Self, Failure> {
        let mut flags = Vec::new();
        let mut it = argv.iter();
        while let Some(a) = it.next() {
            let name = a
                .strip_prefix("--")
                .ok_or_else(|| Failure::Usage(format!("unexpected argument {a:?}")))?;
            if !known.contains(&name) {
                return Err(Failure::Usage(format!("unknown flag --{name}")));
            }
            let value = it
                .next()
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(|| Failure::Usage(format!("--{name} requires a value")))?;
            flags.push((name.to_string(), value.clone()));
        }
        Ok(Self { flags })
    }

    fn all(&self, name: &str) -> Vec<&str> {
        self.flags
            .iter()
            .filter(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    fn one(&self, name: &str) -> Result<Option<&str>, Failure> {
        match self.all(name)[..] {
            [] => Ok(None),
            [v] => Ok(Some(v)),
            _ => Err(Failure::Usage(format!("--{name} is given more than once"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rejects_unknown_flags_and_missing_values() {
        assert!(Args::parse(&argv(&["--custodi", "x"]), &["custody"]).is_err());
        assert!(Args::parse(&argv(&["--custody"]), &["custody"]).is_err());
        assert!(Args::parse(&argv(&["--custody", "--signer"]), &["custody", "signer"]).is_err());
        assert!(Args::parse(&argv(&["stray"]), &["custody"]).is_err());
    }

    #[test]
    fn only_signer_repeats() {
        let a = Args::parse(
            &argv(&[
                "--signer", "a", "--signer", "b", "--year", "1", "--year", "2",
            ]),
            &["signer", "year"],
        )
        .unwrap();
        assert_eq!(a.all("signer"), ["a", "b"]);
        assert!(a.one("year").is_err());
        assert_eq!(a.one("custody").unwrap(), None);
    }
}
