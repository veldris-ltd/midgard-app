//! Strict command-line options for service processes: `--key value` pairs and bare
//! `--flag`s, with anything unknown or repeated refused — a mistyped option must never
//! be silently ignored. [NFR-SEC-1]

use std::collections::BTreeSet;

/// Parsed options.
#[derive(Debug, Default)]
pub struct Args {
    values: Vec<(String, String)>,
    flags: BTreeSet<String>,
}

impl Args {
    /// Parse `argv` (without the program name), accepting only the `valued` options
    /// and bare `flags` named.
    pub fn parse(argv: &[String], valued: &[&str], flags: &[&str]) -> Result<Self, String> {
        let mut parsed = Self::default();
        let mut i = 0;
        while i < argv.len() {
            let arg = &argv[i];
            let key = arg
                .strip_prefix("--")
                .ok_or_else(|| format!("unexpected argument '{arg}'"))?;
            if flags.contains(&key) {
                if !parsed.flags.insert(key.to_string()) {
                    return Err(format!("--{key} given twice"));
                }
                i += 1;
                continue;
            }
            if !valued.contains(&key) {
                return Err(format!("unknown option '--{key}'"));
            }
            let value = argv
                .get(i + 1)
                .filter(|v| !v.starts_with("--"))
                .ok_or_else(|| format!("--{key} requires a value"))?;
            if parsed.get(key).is_some() {
                return Err(format!("--{key} given twice"));
            }
            parsed.values.push((key.to_string(), value.clone()));
            i += 2;
        }
        Ok(parsed)
    }

    /// An option's value, if given.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// A required option's value.
    pub fn require(&self, key: &str) -> Result<&str, String> {
        self.get(key).ok_or_else(|| format!("--{key} is required"))
    }

    /// Whether a bare flag was given.
    pub fn flag(&self, key: &str) -> bool {
        self.flags.contains(key)
    }
}

#[cfg(test)]
mod tests {
    use super::Args;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn parses_values_and_flags_and_refuses_the_rest() {
        let a = Args::parse(
            &argv("--bind 0.0.0.0:7720 --verbose"),
            &["bind"],
            &["verbose"],
        )
        .unwrap();
        assert_eq!(a.get("bind"), Some("0.0.0.0:7720"));
        assert!(a.flag("verbose"));
        assert!(a.require("identity-dir").is_err());

        for bad in [
            "--bnd 1",
            "--bind",
            "--bind --verbose",
            "--bind 1 --bind 2",
            "--verbose --verbose",
            "positional",
        ] {
            assert!(
                Args::parse(&argv(bad), &["bind"], &["verbose"]).is_err(),
                "{bad}"
            );
        }
    }
}
