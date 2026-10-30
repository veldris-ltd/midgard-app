//! Anti-rollback: the **release floor**. [MG-SEC-14] [AC-B-1] [AC-MS-2]
//!
//! A valid signature proves a release came from the release key; it does not prove
//! the release is current. An older release, signed in its day and vulnerable since,
//! verifies exactly as well as the latest one, so every place that accepts signed
//! software also refuses to go backwards. For each artefact id the floor keeps the
//! highest release accepted so far — its version and the digest of what was signed —
//! and a candidate is admitted only if it is:
//!
//! - **newer** than the floor (the floor rises to it), or
//! - **the same release again**: the same version with the same signed digest (a
//!   restart reloading what it already runs).
//!
//! An older version is refused ([`RollbackError::Older`]), and so is the same version
//! with different signed content ([`RollbackError::Conflict`]): two different things
//! both claiming to be `1.4.0` is equivocation, not an upgrade. A version label that
//! cannot be ordered is refused too ([`RollbackError::Unversioned`]) — fail closed,
//! never "cannot tell, so accept".
//!
//! The floor is decided **after** the signature holds: an unverified version label
//! is attacker-chosen and must not move the floor. A floor opened over a file
//! ([`ReleaseFloor::open`]) persists every rise before [`ReleaseFloor::admit`] returns
//! (temp file, `fsync`, rename), so a restart cannot be used to forget it.
//!
//! Versions order as release versions: dot-separated numbers compared numerically
//! (`1.10` > `1.9`, and a missing component counts as 0, so `1.0` = `1.0.0`), then an
//! optional `-pre-release`, which sorts below the release itself with SemVer's rules
//! (`1.0.0-rc.1` < `1.0.0-rc.2` < `1.0.0`). `+build` metadata is ignored for ordering.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::hex::{hex, unhex};

/// The first line of a floor file.
const FILE_HEADER: &str = "midgard-release-floor 1";

/// The longest version label accepted.
const MAX_VERSION: usize = 128;

/// A release version that can be ordered. See the module docs for the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseVersion {
    numbers: Vec<u64>,
    pre: Vec<PreId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreId {
    Number(u64),
    Text(String),
}

impl ReleaseVersion {
    /// Parse a version label, or say why it cannot be ordered.
    pub fn parse(label: &str) -> Result<Self, String> {
        if label.is_empty() || label.len() > MAX_VERSION {
            return Err(format!("{label:?} is not a release version"));
        }
        let without_build = match label.split_once('+') {
            None => label,
            Some((v, build))
                if !build.is_empty()
                    && build
                        .split('.')
                        .all(|id| !id.is_empty() && id.bytes().all(is_identifier_byte)) =>
            {
                v
            }
            Some(_) => return Err(format!("{label:?} has malformed build metadata")),
        };
        let (core, pre) = match without_build.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (without_build, None),
        };
        let numbers = core
            .split('.')
            .map(|n| {
                if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                n.parse::<u64>().ok()
            })
            .collect::<Option<Vec<u64>>>()
            .ok_or_else(|| {
                format!("{label:?} is not a release version (expected numbers such as 1.4.0)")
            })?;
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => pre
                .split('.')
                .map(|id| {
                    if id.is_empty() || !id.bytes().all(is_identifier_byte) {
                        None
                    } else if id.bytes().all(|b| b.is_ascii_digit()) {
                        id.parse().ok().map(PreId::Number)
                    } else {
                        Some(PreId::Text(id.to_string()))
                    }
                })
                .collect::<Option<Vec<PreId>>>()
                .ok_or_else(|| format!("{label:?} has a malformed pre-release part"))?,
        };
        Ok(Self { numbers, pre })
    }
}

/// A byte of a pre-release or build identifier: `[0-9A-Za-z-]`.
fn is_identifier_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-'
}

impl Ord for ReleaseVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        let width = self.numbers.len().max(other.numbers.len());
        for i in 0..width {
            let a = self.numbers.get(i).copied().unwrap_or(0);
            let b = other.numbers.get(i).copied().unwrap_or(0);
            match a.cmp(&b) {
                Ordering::Equal => {}
                unequal => return unequal,
            }
        }
        // A pre-release sorts below the release it precedes.
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (false, false) => {}
        }
        for (a, b) in self.pre.iter().zip(&other.pre) {
            let order = match (a, b) {
                (PreId::Number(a), PreId::Number(b)) => a.cmp(b),
                (PreId::Number(_), PreId::Text(_)) => Ordering::Less,
                (PreId::Text(_), PreId::Number(_)) => Ordering::Greater,
                (PreId::Text(a), PreId::Text(b)) => a.cmp(b),
            };
            if order != Ordering::Equal {
                return order;
            }
        }
        self.pre.len().cmp(&other.pre.len())
    }
}

impl PartialOrd for ReleaseVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Why a release was refused, or the floor could not be kept. Every variant is a
/// refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RollbackError {
    /// The version label cannot be ordered, so whether it goes backwards cannot be
    /// decided.
    Unversioned {
        /// The artefact id.
        id: String,
        /// Why the label does not parse.
        reason: String,
    },
    /// The release is older than one already accepted.
    Older {
        /// The artefact id.
        id: String,
        /// The refused version.
        version: String,
        /// The highest version accepted so far.
        floor: String,
    },
    /// The release claims the accepted version but its signed content differs.
    Conflict {
        /// The artefact id.
        id: String,
        /// The version both claim.
        version: String,
    },
    /// The floor file could not be read or written; nothing is admitted.
    Io(String),
}

impl fmt::Display for RollbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RollbackError::Unversioned { id, reason } => {
                write!(f, "{id}: refused, the version cannot be ordered: {reason}")
            }
            RollbackError::Older { id, version, floor } => write!(
                f,
                "{id}: refused, version {version} is older than {floor}, already accepted \
                 (anti-rollback)"
            ),
            RollbackError::Conflict { id, version } => write!(
                f,
                "{id}: refused, version {version} was already accepted with different signed \
                 content"
            ),
            RollbackError::Io(m) => write!(f, "release floor: {m}"),
        }
    }
}

impl std::error::Error for RollbackError {}

/// How a release relates to the floor when it is admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// The first release seen for this id, or a newer one: the floor rose.
    Raised,
    /// The release the floor already holds, again.
    Same,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Mark {
    version: String,
    parsed: ReleaseVersion,
    digest: [u8; 32],
}

/// The highest release accepted per artefact id, optionally persisted to a file.
#[derive(Debug, Clone, Default)]
pub struct ReleaseFloor {
    path: Option<PathBuf>,
    marks: BTreeMap<String, Mark>,
}

impl ReleaseFloor {
    /// A floor held in memory only: it lasts as long as the process.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// The floor persisted at `path`, read if the file exists and empty otherwise. A
    /// file that exists but does not read back exactly is refused rather than treated
    /// as empty: an unreadable floor must not become no floor.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RollbackError> {
        let path = path.as_ref().to_path_buf();
        let mut floor = Self {
            path: Some(path.clone()),
            marks: BTreeMap::new(),
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(floor),
            Err(e) => {
                return Err(RollbackError::Io(format!(
                    "cannot read {}: {e}",
                    path.display()
                )))
            }
        };
        let corrupt = |what: &str| RollbackError::Io(format!("{}: {what}", path.display()));
        let mut lines = text.lines();
        if lines.next() != Some(FILE_HEADER) {
            return Err(corrupt("not a release floor file"));
        }
        for line in lines.filter(|l| !l.is_empty()) {
            let parts: Vec<&str> = line.split(' ').collect();
            let [id, version, digest] = parts.as_slice() else {
                return Err(corrupt("malformed line"));
            };
            let id = unhex(id)
                .and_then(|b| String::from_utf8(b).ok())
                .ok_or_else(|| corrupt("malformed id"))?;
            let parsed = ReleaseVersion::parse(version).map_err(|e| corrupt(&e))?;
            let digest = unhex(digest)
                .and_then(|b| <[u8; 32]>::try_from(b).ok())
                .ok_or_else(|| corrupt("malformed digest"))?;
            let mark = Mark {
                version: (*version).to_string(),
                parsed,
                digest,
            };
            if floor.marks.insert(id, mark).is_some() {
                return Err(corrupt("an id appears twice"));
            }
        }
        Ok(floor)
    }

    /// Whether the floor file exists yet (always `false` in memory).
    pub fn is_persisted(&self) -> bool {
        self.path.as_ref().is_some_and(|p| p.exists())
    }

    /// The highest version accepted for `id`, and the digest of its signed content.
    pub fn get(&self, id: &str) -> Option<(&str, &[u8; 32])> {
        self.marks.get(id).map(|m| (m.version.as_str(), &m.digest))
    }

    /// Whether `version` of `id`, whose signed content hashes to `digest`, would be
    /// admitted. Nothing changes. Call it only once the signature has verified.
    pub fn check(
        &self,
        id: &str,
        version: &str,
        digest: &[u8; 32],
    ) -> Result<Admission, RollbackError> {
        let parsed =
            ReleaseVersion::parse(version).map_err(|reason| RollbackError::Unversioned {
                id: id.to_string(),
                reason,
            })?;
        let Some(mark) = self.marks.get(id) else {
            return Ok(Admission::Raised);
        };
        match parsed.cmp(&mark.parsed) {
            Ordering::Greater => Ok(Admission::Raised),
            Ordering::Equal if pqc_agility::hash::ct_eq(digest, &mark.digest) => {
                Ok(Admission::Same)
            }
            Ordering::Equal => Err(RollbackError::Conflict {
                id: id.to_string(),
                version: version.to_string(),
            }),
            Ordering::Less => Err(RollbackError::Older {
                id: id.to_string(),
                version: version.to_string(),
                floor: mark.version.clone(),
            }),
        }
    }

    /// Whether an artefact accepted earlier is consistent with the floor: at or below
    /// it, and — at the floor's own version — the very content the floor recorded.
    /// For re-checking what a store already holds (its history may hold older
    /// releases); new releases go through [`admit`](Self::admit).
    pub fn covers(&self, id: &str, version: &str, digest: &[u8; 32]) -> Result<(), RollbackError> {
        let parsed =
            ReleaseVersion::parse(version).map_err(|reason| RollbackError::Unversioned {
                id: id.to_string(),
                reason,
            })?;
        let Some(mark) = self.marks.get(id) else {
            return Err(RollbackError::Io(format!(
                "{id}@{version} is stored but the release floor has no record of {id}"
            )));
        };
        match parsed.cmp(&mark.parsed) {
            Ordering::Less => Ok(()),
            Ordering::Equal if pqc_agility::hash::ct_eq(digest, &mark.digest) => Ok(()),
            Ordering::Equal => Err(RollbackError::Conflict {
                id: id.to_string(),
                version: version.to_string(),
            }),
            Ordering::Greater => Err(RollbackError::Io(format!(
                "{id}@{version} is stored above the release floor ({})",
                mark.version
            ))),
        }
    }

    /// Admit `version` of `id`, whose signed content hashes to `digest`: refuse it if
    /// it goes backwards or conflicts, otherwise raise the floor to it — persisted
    /// before this returns, for a floor opened over a file. Call it only once the
    /// signature has verified.
    pub fn admit(
        &mut self,
        id: &str,
        version: &str,
        digest: &[u8; 32],
    ) -> Result<Admission, RollbackError> {
        let admission = self.check(id, version, digest)?;
        if admission == Admission::Raised {
            let mark = Mark {
                version: version.to_string(),
                parsed: ReleaseVersion::parse(version).expect("checked above"),
                digest: *digest,
            };
            let previous = self.marks.insert(id.to_string(), mark);
            if let Err(e) = self.persist() {
                // Not on disk, so not raised: put the floor back as it was.
                match previous {
                    Some(previous) => self.marks.insert(id.to_string(), previous),
                    None => self.marks.remove(id),
                };
                return Err(e);
            }
        }
        Ok(admission)
    }

    /// Raise the floor for releases a store already held before it kept a floor,
    /// without the anti-rollback check (they were accepted then). Persisted.
    pub fn adopt<'a>(
        &mut self,
        accepted: impl IntoIterator<Item = (&'a str, &'a str, [u8; 32])>,
    ) -> Result<(), RollbackError> {
        for (id, version, digest) in accepted {
            let parsed =
                ReleaseVersion::parse(version).map_err(|reason| RollbackError::Unversioned {
                    id: id.to_string(),
                    reason,
                })?;
            let higher = self.marks.get(id).is_none_or(|m| parsed > m.parsed);
            if higher {
                self.marks.insert(
                    id.to_string(),
                    Mark {
                        version: version.to_string(),
                        parsed,
                        digest,
                    },
                );
            }
        }
        self.persist()
    }

    /// Write the floor to its file (temp, `fsync`, rename). A no-op in memory.
    fn persist(&self) -> Result<(), RollbackError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let mut text = String::from(FILE_HEADER);
        text.push('\n');
        for (id, mark) in &self.marks {
            text.push_str(&format!(
                "{} {} {}\n",
                hex(id.as_bytes()),
                mark.version,
                hex(&mark.digest)
            ));
        }
        let io = |e: std::io::Error| RollbackError::Io(format!("{}: {e}", path.display()));
        let tmp = path.with_extension("tmp");
        {
            let mut opts = OpenOptions::new();
            opts.create(true).write(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            let mut file = opts.open(&tmp).map_err(io)?;
            file.write_all(text.as_bytes()).map_err(io)?;
            file.sync_all().map_err(io)?;
        }
        fs::rename(&tmp, path).map_err(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> ReleaseVersion {
        ReleaseVersion::parse(s).unwrap()
    }

    #[test]
    fn versions_order_as_releases() {
        assert!(v("1.10.0") > v("1.9.9"));
        assert!(v("2") > v("1.99"));
        assert_eq!(v("1.0"), v("1.0").clone());
        assert_eq!(v("1.0").cmp(&v("1.0.0")), Ordering::Equal);
        assert!(v("1.0.0-rc.1") < v("1.0.0-rc.2"));
        assert!(v("1.0.0-rc.2") < v("1.0.0"));
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"));
        assert!(v("1.0.0-1") < v("1.0.0-alpha"));
        assert_eq!(v("1.4.0+build.7").cmp(&v("1.4.0")), Ordering::Equal);
    }

    #[test]
    fn labels_that_cannot_be_ordered_are_refused() {
        for bad in [
            "",
            "latest",
            "v1.0",
            "1..0",
            "1.0-",
            "1.0-rc..1",
            "1.0+",
            "1.x",
        ] {
            assert!(ReleaseVersion::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_floor_rises_and_never_goes_back() {
        let mut floor = ReleaseFloor::in_memory();
        let (a, b) = ([1u8; 32], [2u8; 32]);
        assert_eq!(floor.admit("svc", "1.4.0", &a), Ok(Admission::Raised));
        assert_eq!(floor.admit("svc", "1.4.0", &a), Ok(Admission::Same));
        assert!(matches!(
            floor.admit("svc", "1.4.0", &b),
            Err(RollbackError::Conflict { .. })
        ));
        assert!(matches!(
            floor.admit("svc", "1.3.9", &b),
            Err(RollbackError::Older { .. })
        ));
        assert_eq!(floor.admit("svc", "1.5.0", &b), Ok(Admission::Raised));
        assert!(matches!(
            floor.admit("svc", "1.4.0", &a),
            Err(RollbackError::Older { .. })
        ));
        // Another id has its own floor.
        assert_eq!(floor.admit("other", "0.1.0", &a), Ok(Admission::Raised));
        assert!(matches!(
            floor.admit("svc", "next", &a),
            Err(RollbackError::Unversioned { .. })
        ));
    }

    #[test]
    fn a_persisted_floor_survives_a_restart_and_refuses_a_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("attest-floor-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("release.floor");
        let mut floor = ReleaseFloor::open(&path).unwrap();
        assert!(!floor.is_persisted());
        floor.admit("model a", "3.3", &[7u8; 32]).unwrap();
        assert!(floor.is_persisted());

        let reopened = ReleaseFloor::open(&path).unwrap();
        assert_eq!(reopened.get("model a"), Some(("3.3", &[7u8; 32])));
        assert!(matches!(
            reopened.check("model a", "3.2", &[7u8; 32]),
            Err(RollbackError::Older { .. })
        ));

        fs::write(&path, "garbage\n").unwrap();
        assert!(matches!(
            ReleaseFloor::open(&path),
            Err(RollbackError::Io(_))
        ));
        let _ = fs::remove_dir_all(&dir);
    }
}
