//! The guided ceremony: what `mg-ceremony` does when it is started with no
//! arguments, for instance by double-clicking `mg-ceremony.exe`.
//!
//! It asks one question at a time, checks each answer as it is given, shows
//! everything before a key is generated, and needs the word GENERATE to go on.
//! A rehearsal runs the same questions against a throwaway checkout and deletes
//! everything afterwards, so the officers can practise before the real thing.
//! Output is plain ASCII, so it reads the same in any Windows console.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use crate::ceremony::{self, shown, Outcome, Plan, ALLOWED_SIGNERS, TRUST};
use crate::clock::Now;
use crate::ssh::{check_principal, Signer};

/// A public ed25519 key for rehearsals only. Nobody holds a key that matters for
/// it: it lets a rehearsal run with no SSH key to hand, and is never accepted in
/// a real ceremony.
const PRACTICE_KEY: &str =
    "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINyw+6K7GLdHy8Z8lAxVMz5rVCCiWQhotLU9107+qSZW practice";

/// What the wizard needs from the machine it runs on.
pub struct Env {
    /// Where to look for a Midgard checkout: the current directory and the
    /// directory the program is in.
    pub starts: Vec<PathBuf>,
    /// The user's home directory, whose `.ssh` folder is offered for keys.
    pub home: Option<PathBuf>,
    /// `ssh-keygen`, if installed, to create a signing key on the spot.
    pub ssh_keygen: Option<PathBuf>,
    /// Where a rehearsal puts its throwaway checkout.
    pub temp: PathBuf,
    pub now: Now,
}

/// The console: answers come in, questions and results go out.
pub struct Io<'a> {
    pub input: &'a mut dyn BufRead,
    pub out: &'a mut dyn Write,
}

impl Io<'_> {
    fn say(&mut self, text: &str) {
        let _ = writeln!(self.out, "{text}");
    }

    fn ask(&mut self, question: &str) -> Result<String, String> {
        let _ = write!(self.out, "{question} ");
        let _ = self.out.flush();
        let mut line = String::new();
        match self.input.read_line(&mut line) {
            Ok(0) => Err("the input ended before the ceremony did; nothing was generated".into()),
            Ok(_) => Ok(line.trim().to_string()),
            Err(e) => Err(format!("cannot read the answer: {e}")),
        }
    }

    fn yes(&mut self, question: &str, default: bool) -> Result<bool, String> {
        let hint = if default { "[Y/n]" } else { "[y/N]" };
        loop {
            match self
                .ask(&format!("{question} {hint}"))?
                .to_ascii_lowercase()
                .as_str()
            {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => self.say("  Please answer y or n."),
            }
        }
    }

    /// The next numbered step, counted from the questions this flow asks.
    fn step(&mut self, steps: &mut Steps, title: &str) {
        steps.done += 1;
        self.heading(&format!("Step {} of {}: {title}", steps.done, steps.of));
    }

    fn heading(&mut self, text: &str) {
        self.say("");
        self.say(text);
        self.say(&"-".repeat(text.len()));
    }
}

/// Step numbering: a rehearsal asks three questions, the ceremony five.
struct Steps {
    done: usize,
    of: usize,
}

/// The guided ceremony, from the first menu to the last result.
pub fn run(io: &mut Io, env: &Env) -> Result<(), String> {
    io.say("Midgard release key ceremony");
    io.say("============================");
    io.say("");
    io.say("This generates the two Midgard release keys, publishes their public halves in");
    io.say("the repository, lists who may sign a release tag, and writes the ceremony");
    io.say("record. It commits nothing, pushes nothing and uses no network.");
    io.say("Follow docs/ops/KEY_CEREMONY.md alongside it.");
    loop {
        io.say("");
        io.say("What would you like to do?");
        io.say("  1  Rehearse: a practice run with throwaway keys. Nothing is kept.");
        io.say("  2  Hold the ceremony: generate the real keys.");
        io.say("  3  Check the published files, for example after the ceremony.");
        io.say("  q  Quit.");
        match io
            .ask("Choose 1, 2, 3 or q:")?
            .to_ascii_lowercase()
            .as_str()
        {
            "1" => return rehearse(io, env),
            "2" => return hold(io, env),
            "3" => return check(io, env),
            "q" | "quit" => {
                io.say("Nothing was generated.");
                return Ok(());
            }
            _ => io.say("  Please type 1, 2, 3 or q."),
        }
    }
}

fn rehearse(io: &mut Io, env: &Env) -> Result<(), String> {
    let dir = env.temp.join(format!(
        "mg-ceremony-rehearsal-{}-{}",
        std::process::id(),
        env.now.compact_time()
    ));
    let repo = dir.join("checkout");
    std::fs::create_dir_all(repo.join(TRUST))
        .map_err(|e| format!("cannot create the rehearsal checkout: {e}"))?;
    std::fs::write(repo.join(TRUST).join("README.md"), "Rehearsal checkout.\n")
        .map_err(|e| format!("cannot create the rehearsal checkout: {e}"))?;
    io.say("");
    io.say("REHEARSAL. The questions are the real ones; the keys and files go to a");
    io.say(&format!(
        "throwaway folder ({}) that is deleted at the end.",
        shown(&dir)
    ));
    let steps = &mut Steps { done: 0, of: 3 };
    let result = (|| {
        let (officer, witness) = people(io, steps)?;
        let signers = signers(io, env, true, steps)?;
        let plan = Plan {
            repo: repo.clone(),
            custody: dir.join("custody"),
            year: env.now.year.to_string(),
            officer,
            witness,
            signers,
        };
        if !confirm(io, &plan, true, steps)? {
            return Ok(());
        }
        let outcome = ceremony::hold(&plan, &env.now, None)?;
        report(io, &outcome, &env.now, true);
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    io.say("");
    io.say("The rehearsal folder has been deleted. Run again and choose 2 for the real ceremony.");
    result
}

fn hold(io: &mut Io, env: &Env) -> Result<(), String> {
    let steps = &mut Steps { done: 0, of: 5 };
    let repo = repo(io, env, steps)?;
    if let Err(e) = ceremony::check_not_held(&repo) {
        io.say("");
        io.say(&format!("Stopping: {e}."));
        io.say("If the ceremony has been held, choose 3 to check the published files.");
        return Ok(());
    }
    let (officer, witness) = people(io, steps)?;
    let custody = custody(io, &repo, steps)?;
    let signers = signers(io, env, false, steps)?;
    let plan = Plan {
        repo: repo.clone(),
        custody,
        year: env.now.year.to_string(),
        officer,
        witness,
        signers,
    };
    if !confirm(io, &plan, false, steps)? {
        return Ok(());
    }
    let commit = ceremony::git_head(&repo);
    let outcome = ceremony::hold(&plan, &env.now, commit.as_deref())?;
    report(io, &outcome, &env.now, false);
    Ok(())
}

fn check(io: &mut Io, env: &Env) -> Result<(), String> {
    let repo = repo(io, env, &mut Steps { done: 0, of: 1 })?;
    match ceremony::check_published(&repo) {
        Ok(p) => {
            io.heading("The published files pass the release's checks");
            io.say(&ceremony::second_channel_text(&p, &env.now));
            io.say("Compare these values with the ones published through the second channel.");
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Step 1: the checkout.
fn repo(io: &mut Io, env: &Env, steps: &mut Steps) -> Result<PathBuf, String> {
    io.step(steps, "the Midgard checkout");
    if let Some(found) = ceremony::discover_repo(&env.starts) {
        io.say(&format!("Found a Midgard checkout at {}", shown(&found)));
        if io.yes("Use it?", true)? {
            return Ok(found);
        }
    }
    loop {
        let a = io.ask(
            "Type the folder of your Midgard checkout (it contains deploy\\trust or deploy/trust):",
        )?;
        match ceremony::find_repo(Path::new(&a)) {
            Ok(r) => return Ok(r),
            Err(e) => io.say(&format!("  {e}. Try again.")),
        }
    }
}

/// Step 2: the two people. Dual control: they must be different people.
fn people(io: &mut Io, steps: &mut Steps) -> Result<(String, String), String> {
    io.step(steps, "who is present");
    io.say("Two people hold the ceremony: the duty Security Officer, and a witness.");
    let officer = named(io, "Security Officer's full name:")?;
    loop {
        let witness = named(io, "Witness's full name:")?;
        if witness.eq_ignore_ascii_case(&officer) {
            io.say("  The witness must be a different person from the Security Officer.");
        } else {
            return Ok((officer, witness));
        }
    }
}

fn named(io: &mut Io, question: &str) -> Result<String, String> {
    loop {
        let a = io.ask(question)?;
        if a.is_empty() {
            io.say("  A name is needed for the record.");
        } else if a.contains('|') {
            io.say("  Please leave out the '|' character; the record is a table.");
        } else {
            return Ok(a);
        }
    }
}

/// Step 3: where the keystores go.
fn custody(io: &mut Io, repo: &Path, steps: &mut Steps) -> Result<PathBuf, String> {
    io.step(steps, "where the private keys go");
    io.say("The two keystore files ARE the keys: whoever holds them can sign as Midgard.");
    io.say("Put them in a NEW, empty folder on encrypted removable media, never inside");
    io.say("the checkout. For example:");
    io.say("  Windows:  E:\\midgard-keys-2026         (a BitLocker To Go USB drive)");
    io.say("  Linux:    /media/you/KEYS/midgard-keys-2026   (a LUKS USB drive)");
    loop {
        let a = io.ask("Type the new folder:")?;
        match ceremony::check_custody(repo, Path::new(&a)) {
            Ok(p) => return Ok(p),
            Err(e) => io.say(&format!("  {e}. Try again.")),
        }
    }
}

/// Step 4: who may sign a release tag.
fn signers(
    io: &mut Io,
    env: &Env,
    rehearsal: bool,
    steps: &mut Steps,
) -> Result<Vec<Signer>, String> {
    io.step(steps, "who may sign a release tag");
    io.say("Each person who may cut a release tag gives their SSH signing key: the .pub");
    io.say("file only, never the private key. Add at least one person.");
    let mut out: Vec<Signer> = Vec::new();
    loop {
        io.say("");
        let principal = loop {
            let a = io.ask(&format!(
                "Signer {}: email address they tag with (git config user.email):",
                out.len() + 1
            ))?;
            match check_principal(&a) {
                Err(e) => io.say(&format!("  {e}.")),
                Ok(()) if out.iter().any(|s| s.principal == a) => {
                    io.say("  That person is already listed.")
                }
                Ok(()) => break a,
            }
        };
        let signer = key_for(io, env, &principal, rehearsal)?;
        io.say(&format!("  {}  {}", signer.principal, signer.fingerprint));
        io.say("  Ask them to confirm this fingerprint matches: ssh-keygen -lf <their key>.pub");
        out.push(signer);
        if !io.yes("Add another tag signer?", false)? {
            return Ok(out);
        }
    }
}

fn key_for(io: &mut Io, env: &Env, principal: &str, rehearsal: bool) -> Result<Signer, String> {
    let found = public_keys(env.home.as_deref());
    loop {
        io.say("  Which SSH public key?");
        for (i, k) in found.iter().enumerate() {
            io.say(&format!("    {}  {}", i + 1, shown(k)));
        }
        io.say("    p  type the path of a .pub file (for example on a USB stick)");
        if env.ssh_keygen.is_some() {
            io.say("    n  create a new signing key for this person now, with ssh-keygen");
        }
        if rehearsal {
            io.say("    t  use a practice key (rehearsals only)");
        }
        let a = io.ask("  Choose:")?;
        let path = match a.to_ascii_lowercase().as_str() {
            "t" if rehearsal => return Signer::parse(principal, PRACTICE_KEY),
            "p" => PathBuf::from(io.ask("  Path of the .pub file:")?),
            "n" if env.ssh_keygen.is_some() => match new_key(io, env, principal) {
                Ok(p) => p,
                Err(e) => {
                    io.say(&format!("  {e}"));
                    continue;
                }
            },
            other => match other
                .parse::<usize>()
                .ok()
                .and_then(|n| found.get(n.wrapping_sub(1)))
            {
                Some(p) => p.clone(),
                None => {
                    io.say("  Please choose one of the options listed.");
                    continue;
                }
            },
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match Signer::parse(principal, &text) {
                Ok(s) => return Ok(s),
                Err(e) => io.say(&format!("  {}: {e}.", shown(&path))),
            },
            Err(e) => io.say(&format!("  cannot read {}: {e}.", shown(&path))),
        }
    }
}

/// The `.pub` files in `~/.ssh`, sorted.
fn public_keys(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(dir) = home.map(|h| h.join(".ssh")) else {
        return Vec::new();
    };
    let mut keys: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pub"))
        .collect();
    keys.sort();
    keys
}

/// Run `ssh-keygen` for a new ed25519 signing key. ssh-keygen asks for the
/// passphrase itself, so it never passes through this program.
fn new_key(io: &mut Io, env: &Env, principal: &str) -> Result<PathBuf, String> {
    let keygen = env
        .ssh_keygen
        .as_ref()
        .ok_or("ssh-keygen is not installed")?;
    let home = env
        .home
        .as_ref()
        .ok_or("no home directory to put the key in")?;
    let dir = home.join(".ssh");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", shown(&dir)))?;
    let base = dir.join("id_ed25519_midgard_signing");
    if base.exists() {
        return Err(format!(
            "{} already exists; choose its .pub file from the list instead",
            shown(&base)
        ));
    }
    io.say("  ssh-keygen will ask for a passphrase. Choose a strong one and keep it in KeePass.");
    let status = std::process::Command::new(keygen)
        .args(["-t", "ed25519", "-C", principal, "-f"])
        .arg(&base)
        .status()
        .map_err(|e| format!("cannot run ssh-keygen: {e}"))?;
    if !status.success() {
        return Err("ssh-keygen did not create the key".into());
    }
    io.say(&format!(
        "  Created {} (private, stays with this person) and its .pub.",
        shown(&base)
    ));
    io.say("  Point git at it: git config --global gpg.format ssh");
    io.say(&format!(
        "                  git config --global user.signingkey \"{}.pub\"",
        shown(&base)
    ));
    let mut public = base.into_os_string();
    public.push(".pub");
    Ok(PathBuf::from(public))
}

/// Step 5: everything, once more, and the word that starts generation.
fn confirm(io: &mut Io, plan: &Plan, rehearsal: bool, steps: &mut Steps) -> Result<bool, String> {
    io.step(steps, "check everything, then generate");
    if !rehearsal {
        io.say(&format!("  Checkout:          {}", shown(&plan.repo)));
        io.say(&format!("  Keystores go to:   {}", shown(&plan.custody)));
    }
    io.say(&format!("  Security Officer:  {}", plan.officer));
    io.say(&format!("  Witness:           {}", plan.witness));
    io.say(&format!(
        "  Key ids:           midgard-release-{0} (ML-DSA-87), midgard-bifrost-{0} (ML-DSA-65)",
        plan.year
    ));
    io.say("  Tag signers:");
    for s in &plan.signers {
        io.say(&format!("    {}  {}", s.principal, s.fingerprint));
    }
    io.say("");
    if let Err(e) = ceremony::check_plan(plan) {
        io.say(&format!("Stopping before anything is generated: {e}."));
        return Ok(false);
    }
    let a = io.ask("Type GENERATE to generate the keys, or anything else to stop:")?;
    if a.eq_ignore_ascii_case("generate") {
        Ok(true)
    } else {
        io.say("Stopped. Nothing was generated.");
        Ok(false)
    }
}

/// The result, and what to do next.
pub fn report(io: &mut Io, o: &Outcome, now: &Now, rehearsal: bool) {
    io.heading(if rehearsal {
        "Rehearsal complete"
    } else {
        "Keys generated. Nothing has been committed or pushed."
    });
    io.say("Keystores (the private keys; move them to their custodian):");
    io.say(&format!(
        "  {}   ML-DSA-87  {}",
        shown(&o.release_keystore),
        o.release_id
    ));
    io.say(&format!(
        "  {}   ML-DSA-65  {}",
        shown(&o.bifrost_keystore),
        o.bifrost_id
    ));
    io.say("");
    // Relative to the checkout, and written the way git prints paths.
    let record = o.record.to_string_lossy().replace('\\', "/");
    io.say(&format!("Ceremony record: {record}"));
    io.say("");
    io.say("Publish this through the second channel (copy it as it is):");
    io.say("");
    for line in ceremony::second_channel_text(&o.published, now).lines() {
        io.say(&format!("    {line}"));
    }
    if rehearsal {
        return;
    }
    io.say("");
    io.say("Next, in this order (docs/ops/KEY_CEREMONY.md, part C):");
    io.say("  1. Send the text above through the second channel; write where and when in the");
    io.say("     record's 'Second channel' section.");
    io.say("  2. Both people compare the values and fill in the record's sign-off table.");
    io.say("  3. Commit the published files and the record in one signed commit:");
    io.say(&format!(
        "       git add {TRUST}/release-trust.pub {TRUST}/bifrost-trust.pub {TRUST}/{ALLOWED_SIGNERS} {record}"
    ));
    io.say(&format!(
        "       git commit -S -m \"ops: release key ceremony, {}\"",
        now.long_date()
    ));
    io.say("  4. Register each tag signer's SSH key on the forge as a signing key.");
    io.say("  5. Move the keystores to their custodian, set the runner secrets to their paths");
    io.say("     and run: bash tools/ci/release-preflight.sh");
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAINyw+6K7GLdHy8Z8lAxVMz5rVCCiWQhotLU9107+qSZW alice";

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("mg-ceremony-wizard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env(dir: &Path) -> Env {
        Env {
            starts: vec![dir.join("repo")],
            home: Some(dir.join("home")),
            ssh_keygen: None,
            temp: dir.join("tmp"),
            now: Now::from_unix(1_793_178_305),
        }
    }

    fn drive(env: &Env, answers: &str) -> (Result<(), String>, String) {
        let mut input = answers.as_bytes();
        let mut out = Vec::new();
        let r = run(
            &mut Io {
                input: &mut input,
                out: &mut out,
            },
            env,
        );
        (r, String::from_utf8(out).unwrap())
    }

    fn checkout(dir: &Path) -> PathBuf {
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join(TRUST)).unwrap();
        std::fs::write(repo.join(TRUST).join("README.md"), "anchors\n").unwrap();
        repo
    }

    #[test]
    fn a_rehearsal_runs_end_to_end_and_leaves_nothing_behind() {
        let dir = scratch("rehearse");
        let env = env(&dir);
        let (r, out) = drive(
            &env,
            "1\nA. Officer\nW. Itness\nprac@example.invalid\nt\nn\nGENERATE\n",
        );
        r.unwrap();
        assert!(out.contains("Rehearsal complete"), "{out}");
        assert!(out.contains("sha256 "), "{out}");
        assert!(
            std::fs::read_dir(&env.temp).unwrap().next().is_none(),
            "rehearsal folder kept"
        );
    }

    #[test]
    fn the_real_ceremony_publishes_into_the_checkout_and_keys_outside_it() {
        let dir = scratch("hold");
        let repo = checkout(&dir);
        std::fs::create_dir_all(dir.join("home/.ssh")).unwrap();
        std::fs::write(dir.join("home/.ssh/id_ed25519.pub"), ALICE).unwrap();
        let custody = dir.join("usb").join("midgard-keys");
        let answers = format!(
            "2\n\nA. Officer\na. officer\nW. Itness\n{}\nalice@example.invalid\n1\nn\nGENERATE\n",
            shown(&custody)
        );
        let (r, out) = drive(&env(&dir), &answers);
        r.unwrap();
        assert!(out.contains("must be a different person"), "{out}");
        assert!(out.contains("git commit -S"), "{out}");
        assert!(repo.join(TRUST).join("release-trust.pub").is_file());
        assert!(repo.join(TRUST).join("bifrost-trust.pub").is_file());
        assert!(
            std::fs::read_to_string(repo.join(TRUST).join(ALLOWED_SIGNERS))
                .unwrap()
                .contains("alice@example.invalid namespaces=\"git\" ssh-ed25519 ")
        );
        assert!(custody.join("release.keystore").is_file());
        assert!(custody.join("bifrost.keystore").is_file());
        let record = std::fs::read_to_string(
            repo.join(ceremony::RECORDS)
                .join("2026-10-28-release-keys.md"),
        )
        .unwrap();
        assert!(
            record.contains("| Security Officer | A. Officer |"),
            "{record}"
        );
        assert!(record.contains("| Witness | W. Itness |"), "{record}");
        assert!(!record.contains(&shown(&custody)));

        // Held once: a second run stops before asking anything else.
        let (r, out) = drive(&env(&dir), "2\n\n");
        r.unwrap();
        assert!(out.contains("already exists"), "{out}");
        // And the check reads it back.
        let (r, out) = drive(&env(&dir), "3\n\n");
        r.unwrap();
        assert!(out.contains("pass the release's checks"), "{out}");
    }

    #[test]
    fn a_practice_key_is_not_offered_in_a_real_ceremony() {
        let dir = scratch("nopractice");
        checkout(&dir);
        let answers = format!(
            "2\n\nA. Officer\nW. Itness\n{}\nalice@example.invalid\nt\n",
            dir.join("keys").display()
        );
        let (r, out) = drive(&env(&dir), &answers);
        assert!(r.is_err(), "input ended, so it must not have generated");
        assert!(
            out.contains("Please choose one of the options listed"),
            "{out}"
        );
        assert!(!dir
            .join("repo")
            .join(TRUST)
            .join("release-trust.pub")
            .exists());
    }

    #[test]
    fn anything_but_generate_stops_before_a_key_exists() {
        let dir = scratch("stop");
        checkout(&dir);
        let custody = dir.join("keys");
        std::fs::write(dir.join("alice.pub"), ALICE).unwrap();
        let answers = format!(
            "2\n\nA. Officer\nW. Itness\n{}\nalice@example.invalid\np\n{}\nn\nno\n",
            shown(&custody),
            dir.join("alice.pub").display()
        );
        let (r, out) = drive(&env(&dir), &answers);
        r.unwrap();
        assert!(out.contains("Nothing was generated"), "{out}");
        assert!(!custody.exists());
        assert!(!dir
            .join("repo")
            .join(TRUST)
            .join("release-trust.pub")
            .exists());
    }

    #[test]
    fn a_custody_folder_inside_the_checkout_is_asked_again() {
        let dir = scratch("inside");
        let repo = checkout(&dir);
        let answers = format!(
            "2\n\nA. Officer\nW. Itness\n{}\n",
            repo.join("keys").display()
        );
        let (r, out) = drive(&env(&dir), &answers);
        assert!(r.is_err());
        assert!(out.contains("inside this checkout"), "{out}");
    }
}
