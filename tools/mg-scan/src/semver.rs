//! Just enough semver to decide whether a locked version falls inside an
//! advisory's patched or unaffected ranges.
//!
//! ## Why hand-rolled
//! The workspace vendors no general-purpose crates, and this needs to run
//! offline inside the release gate. The subset RustSec actually uses is small:
//! `>=`, `>`, `<`, `<=`, `=`, `^`, and comma-separated conjunctions.
//!
//! ## Fail closed on anything unrecognised
//! [`Req::parse`] returns `None` for syntax it does not understand, and every
//! caller treats that as *possibly affected* rather than skipping the advisory.
//! A version comparator that silently ignores what it cannot read is how a real
//! vulnerability gets marked clean.

/// A parsed semantic version. Build metadata is discarded (it is not ordered);
/// pre-release identifiers are kept because they order below the release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: Vec<String>,
}

impl Version {
    /// Parse `MAJOR.MINOR.PATCH[-PRE][+BUILD]`. Missing minor/patch default to 0.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let s = s.split('+').next()?; // drop build metadata
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, p.split('.').map(|x| x.to_string()).collect()),
            None => (s, Vec::new()),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        if parts.next().is_some() {
            return None; // four components is not semver
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                // A release outranks any of its pre-releases.
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (false, false) => compare_pre(&self.pre, &other.pre),
            })
    }
}

/// Pre-release precedence: numeric identifiers compare numerically and rank
/// below alphanumeric ones; a longer identifier list outranks its prefix.
fn compare_pre(a: &[String], b: &[String]) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    for (x, y) in a.iter().zip(b.iter()) {
        let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(xn), Ok(yn)) => xn.cmp(&yn),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

/// One comparator, e.g. `>= 0.9.1`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Op {
    Gte,
    Gt,
    Lte,
    Lt,
    Eq,
    /// `^x.y.z` — compatible-with: `>= x.y.z` and `< next-breaking`.
    Caret,
}

/// A conjunction of comparators. `Req::matches` is true only if all hold.
#[derive(Debug, Clone)]
pub struct Req {
    parts: Vec<(Op, Version)>,
}

impl Req {
    /// Parse a comma-separated requirement. Returns `None` on anything not
    /// understood, which callers must treat as "possibly affected".
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = Vec::new();
        for raw in s.split(',') {
            let t = raw.trim();
            if t.is_empty() {
                continue;
            }
            let (op, rest) = if let Some(r) = t.strip_prefix(">=") {
                (Op::Gte, r)
            } else if let Some(r) = t.strip_prefix("<=") {
                (Op::Lte, r)
            } else if let Some(r) = t.strip_prefix('>') {
                (Op::Gt, r)
            } else if let Some(r) = t.strip_prefix('<') {
                (Op::Lt, r)
            } else if let Some(r) = t.strip_prefix('^') {
                (Op::Caret, r)
            } else if let Some(r) = t.strip_prefix('=') {
                (Op::Eq, r)
            } else {
                // A bare version means exact equality in RustSec's usage.
                (Op::Eq, t)
            };
            parts.push((op, Version::parse(rest.trim())?));
        }
        (!parts.is_empty()).then_some(Self { parts })
    }

    /// Whether `v` satisfies every comparator.
    pub fn matches(&self, v: &Version) -> bool {
        self.parts.iter().all(|(op, bound)| match op {
            Op::Gte => v >= bound,
            Op::Gt => v > bound,
            Op::Lte => v <= bound,
            Op::Lt => v < bound,
            Op::Eq => v == bound,
            Op::Caret => v >= bound && v < &caret_ceiling(bound),
        })
    }
}

/// The exclusive upper bound of `^x.y.z`, per Cargo's rules: the next non-zero
/// leading component. `^0.2.3` allows `<0.3.0`; `^0.0.3` allows `<0.0.4`.
fn caret_ceiling(b: &Version) -> Version {
    let (major, minor, patch) = if b.major > 0 {
        (b.major + 1, 0, 0)
    } else if b.minor > 0 {
        (0, b.minor + 1, 0)
    } else {
        (0, 0, b.patch + 1)
    };
    Version {
        major,
        minor,
        patch,
        pre: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap_or_else(|| panic!("parse {s}"))
    }

    #[test]
    fn parses_and_orders_versions() {
        assert!(v("1.0.0") < v("1.0.1"));
        assert!(v("1.0.0") < v("1.1.0"));
        assert!(v("1.9.0") < v("2.0.0"));
        assert_eq!(
            v("1.2.3"),
            v("1.2.3+build.5"),
            "build metadata is not ordered"
        );
        assert_eq!(v("1"), v("1.0.0"), "missing components default to zero");
    }

    #[test]
    fn a_prerelease_ranks_below_its_release() {
        assert!(v("1.0.0-alpha") < v("1.0.0"));
        assert!(v("1.0.0-alpha") < v("1.0.0-beta"));
        assert!(v("1.0.0-alpha.1") < v("1.0.0-alpha.2"));
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"), "prefix ranks lower");
        assert!(
            v("1.0.0-1") < v("1.0.0-alpha"),
            "numeric ranks below alphanumeric"
        );
    }

    #[test]
    fn matches_the_comparators_rustsec_uses() {
        assert!(Req::parse(">= 0.9.1").unwrap().matches(&v("0.9.1")));
        assert!(Req::parse(">= 0.9.1").unwrap().matches(&v("1.0.0")));
        assert!(!Req::parse(">= 0.9.1").unwrap().matches(&v("0.9.0")));
        assert!(Req::parse("< 1.0.0").unwrap().matches(&v("0.9.9")));
        assert!(Req::parse("> 1.0.0, < 2.0.0").unwrap().matches(&v("1.5.0")));
        assert!(!Req::parse("> 1.0.0, < 2.0.0").unwrap().matches(&v("2.0.0")));
        assert!(Req::parse("= 1.2.3").unwrap().matches(&v("1.2.3")));
        assert!(
            Req::parse("1.2.3").unwrap().matches(&v("1.2.3")),
            "bare = exact"
        );
    }

    #[test]
    fn caret_follows_cargos_zero_version_rules() {
        assert!(Req::parse("^1.2.3").unwrap().matches(&v("1.9.0")));
        assert!(!Req::parse("^1.2.3").unwrap().matches(&v("2.0.0")));
        assert!(Req::parse("^0.2.3").unwrap().matches(&v("0.2.9")));
        assert!(!Req::parse("^0.2.3").unwrap().matches(&v("0.3.0")));
        assert!(!Req::parse("^0.0.3").unwrap().matches(&v("0.0.4")));
    }

    #[test]
    fn unparseable_input_is_none_so_callers_can_fail_closed() {
        assert!(Req::parse("~1.2.3").is_none(), "tilde is not handled");
        assert!(Req::parse(">= not-a-version").is_none());
        assert!(Req::parse("").is_none());
        assert!(Version::parse("1.2.3.4").is_none());
        assert!(Version::parse("banana").is_none());
    }
}
