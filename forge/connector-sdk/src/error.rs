//! Connector error taxonomy. A connector failure never yields a partial dataset —
//! every path returns an explicit error, so a broken source cannot silently commit
//! garbage into the store.
//!
//! An error's [`Display`](std::fmt::Display) is what reaches logs and job status, and
//! log sinks are read more widely than the secrets directory, so it is redacted: a
//! stable class, hosts and secret names only as short hashes (`h:<8 hex>`,
//! `s:<8 hex>`), absolute paths reduced to their file name, and the source's own
//! text stripped of control characters, the whole capped at
//! [`redact::MAX_DISPLAY`] characters. [`ConnectorError::diagnostic`] keeps the full
//! detail for a debug log an operator turns on deliberately. [VLD-ANB-MIDGARD-0071]

use forge_dataset_store::{DataError, StoreError};

/// A connector or credential failure.
#[derive(Debug)]
pub enum ConnectorError {
    /// The connector asked to reach a host the sandbox does not permit. In the
    /// air-gapped default this is any outbound access. [NFR-PRV-1]
    SandboxDenied {
        /// The host the connector tried to reach.
        host: String,
    },
    /// A required credential could not be resolved (unknown ref, or the KMS refused
    /// to unseal it), or the source refused the one presented.
    Credential {
        /// The secret's reference, where one is involved.
        secret: Option<String>,
        /// What went wrong.
        detail: String,
    },
    /// The source payload could not be parsed into the declared schema.
    Parse(String),
    /// The parsed rows did not honour the declared schema.
    Data(DataError),
    /// Committing the sealed raw dataset to the store failed.
    Store(String),
    /// Reading the source (file / local object / network peer) failed. [FR-F-2]
    Io {
        /// The `host:port` being read, where the failure has one.
        target: Option<String>,
        /// What went wrong.
        detail: String,
    },
}

impl ConnectorError {
    /// A credential failure that names no secret.
    pub fn credential(detail: impl Into<String>) -> Self {
        ConnectorError::Credential {
            secret: None,
            detail: detail.into(),
        }
    }

    /// A credential failure for the secret referenced as `secret`.
    pub fn secret(secret: impl Into<String>, detail: impl Into<String>) -> Self {
        ConnectorError::Credential {
            secret: Some(secret.into()),
            detail: detail.into(),
        }
    }

    /// A source I/O failure with no network peer.
    pub fn io(detail: impl Into<String>) -> Self {
        ConnectorError::Io {
            target: None,
            detail: detail.into(),
        }
    }

    /// A source I/O failure talking to `target` (`host:port`).
    pub fn io_at(target: impl Into<String>, detail: impl Into<String>) -> Self {
        ConnectorError::Io {
            target: Some(target.into()),
            detail: detail.into(),
        }
    }

    /// The stable class this error is counted, logged and audited under.
    pub fn class(&self) -> &'static str {
        match self {
            ConnectorError::SandboxDenied { .. } => "sandbox-denied",
            ConnectorError::Credential { .. } => "credential",
            ConnectorError::Parse(_) => "parse",
            ConnectorError::Data(_) => "data",
            ConnectorError::Store(_) => "store",
            ConnectorError::Io { .. } => "io",
        }
    }

    /// The full, unredacted detail: hosts, secret names, paths and the source's own
    /// text. Only for a debug log an operator has asked for (the worker's
    /// `--diagnostics`); never for job status, metrics or the ledger.
    pub fn diagnostic(&self) -> String {
        match self {
            ConnectorError::SandboxDenied { host } => {
                format!("sandbox denied egress to '{host}'")
            }
            ConnectorError::Credential {
                secret: Some(secret),
                detail,
            } => format!("credential error: secret '{secret}': {detail}"),
            ConnectorError::Credential {
                secret: None,
                detail,
            } => format!("credential error: {detail}"),
            ConnectorError::Parse(m) => format!("parse error: {m}"),
            ConnectorError::Data(e) => format!("schema violation: {e}"),
            ConnectorError::Store(m) => format!("store error: {m}"),
            ConnectorError::Io {
                target: Some(target),
                detail,
            } => format!("source I/O error: {target}: {detail}"),
            ConnectorError::Io {
                target: None,
                detail,
            } => format!("source I/O error: {detail}"),
        }
    }
}

impl std::fmt::Display for ConnectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted = match self {
            ConnectorError::SandboxDenied { host } => {
                format!("egress to {} denied", redact::host(host))
            }
            ConnectorError::Credential { secret, detail } => {
                with_reference(secret.as_deref().map(redact::secret), detail)
            }
            ConnectorError::Parse(m) | ConnectorError::Store(m) => redact::text(m),
            ConnectorError::Data(e) => redact::text(&e.to_string()),
            ConnectorError::Io { target, detail } => {
                with_reference(target.as_deref().map(redact::host), detail)
            }
        };
        let line = format!("{}: {redacted}", self.class());
        f.write_str(&line.chars().take(redact::MAX_DISPLAY).collect::<String>())
    }
}

/// `<reference>: <redacted detail>`, or the redacted detail alone.
fn with_reference(reference: Option<String>, detail: &str) -> String {
    match reference {
        Some(reference) => format!("{reference}: {}", redact::text(detail)),
        None => redact::text(detail),
    }
}

impl std::error::Error for ConnectorError {}

impl From<DataError> for ConnectorError {
    fn from(e: DataError) -> Self {
        ConnectorError::Data(e)
    }
}

impl From<StoreError> for ConnectorError {
    fn from(e: StoreError) -> Self {
        ConnectorError::Store(e.to_string())
    }
}

/// The redactions a [`ConnectorError`]'s display applies, public so a connector that
/// must name a host inside free text (a peer in a protocol error) names it the same
/// way. [VLD-ANB-MIDGARD-0071]
pub mod redact {
    use pqc_agility::hash::sha256;

    /// The most characters an error's display carries.
    pub const MAX_DISPLAY: usize = 120;

    /// The first 8 hex characters of SHA-256 over `value`.
    fn short_hash(value: &str) -> String {
        let digest = sha256(value.as_bytes());
        format!(
            "{:02x}{:02x}{:02x}{:02x}",
            digest[0], digest[1], digest[2], digest[3]
        )
    }

    /// A host (or `host:port`) as `h:<8 hex>`: stable, so an operator can match it
    /// against the configured sources, but not the topology itself.
    pub fn host(host: &str) -> String {
        format!("h:{}", short_hash(host))
    }

    /// A secret's reference as `s:<8 hex>`.
    pub fn secret(name: &str) -> String {
        format!("s:{}", short_hash(name))
    }

    /// A path reduced to its file name.
    pub fn path(path: &str) -> String {
        path.rsplit(['/', '\\'])
            .find(|part| !part.is_empty())
            .unwrap_or_default()
            .to_string()
    }

    /// Free text from a source or the OS: control characters dropped, every absolute
    /// path in it reduced to its file name, and capped at [`MAX_DISPLAY`] characters.
    pub fn text(text: &str) -> String {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        let words: Vec<String> = clean.split(' ').map(word).collect();
        words.join(" ").chars().take(MAX_DISPLAY).collect()
    }

    /// One space-separated word, with an absolute path in it (after any opening quote
    /// or bracket) reduced to its file name and any closing punctuation kept.
    fn word(word: &str) -> String {
        let rest = word.trim_start_matches(['\'', '"', '(', '[']);
        let lead = &word[..word.len() - rest.len()];
        let body = rest.trim_end_matches(['\'', '"', ')', ']', ',', ':', ';']);
        let tail = &rest[body.len()..];
        let absolute = body.starts_with('/')
            || body.starts_with(r"\\")
            || body.get(1..3) == Some(r":\")
            || body.get(1..3) == Some(":/");
        if absolute && body.len() > 1 {
            format!("{lead}{}{tail}", path(body))
        } else {
            word.to_string()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn absolute_paths_in_text_keep_only_their_file_name() {
            assert_eq!(
                text("secret 'pg' cannot be read from /run/secrets: denied"),
                "secret 'pg' cannot be read from secrets: denied"
            );
            assert_eq!(
                text("reading '/data/in/a.csv': gone"),
                "reading 'a.csv': gone"
            );
            assert_eq!(text(r"open C:\state\keys\x.key"), "open x.key");
            assert_eq!(
                text("aircraft/2026.csv and / alone"),
                "aircraft/2026.csv and / alone"
            );
            assert_eq!(text("a\u{1b}[0mb\r\n"), "a[0mb");
        }

        #[test]
        fn hashes_are_short_and_stable() {
            assert_eq!(host("db:5432"), host("db:5432"));
            assert_ne!(host("db:5432"), host("db:5433"));
            assert_eq!(secret("pg").len(), 10);
        }
    }
}
