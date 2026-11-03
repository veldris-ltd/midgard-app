//! What a connector failure says where logs can read it: a stable class, hosts and
//! secret names only as short hashes, absolute paths as file names, and the source's
//! own text control-free and capped, while `diagnostic()` keeps the full detail for
//! a debug log an operator asks for. [NFR-SEC-1] [AC-27.5] [VLD-ANB-MIDGARD-0071]

use forge_connector_sdk::error::redact;
use forge_connector_sdk::ConnectorError;

fn is_short_hash(s: &str) -> bool {
    s.len() == 8
        && s.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// The 8 hex characters after the first occurrence of `prefix` in `shown`.
fn hash_after<'a>(shown: &'a str, prefix: &str) -> &'a str {
    let at = shown.find(prefix).expect("a hashed reference") + prefix.len();
    &shown[at..at + 8]
}

#[test]
fn connector_errors_redact_hosts_secret_names_and_server_text() {
    // A secret that cannot be read from the secrets directory.
    let credential = ConnectorError::secret(
        "db-password",
        "cannot be read from /run/secrets: No such file or directory (os error 2)",
    );
    let shown = credential.to_string();
    assert_eq!(credential.class(), "credential");
    assert!(shown.starts_with("credential: s:"), "{shown}");
    assert!(is_short_hash(hash_after(&shown, "s:")), "{shown}");
    assert_eq!(
        redact::secret("db-password"),
        format!("s:{}", hash_after(&shown, "s:"))
    );
    assert!(!shown.contains("db-password"), "{shown}");
    assert!(!shown.contains("/run/secrets"), "{shown}");
    assert!(credential.diagnostic().contains("db-password"));
    assert!(credential.diagnostic().contains("/run/secrets"));

    // A source that cannot be reached.
    let io = ConnectorError::io_at("db.internal.example:5432", "cannot connect: refused");
    let shown = io.to_string();
    assert_eq!(io.class(), "io");
    assert!(shown.starts_with("io: h:"), "{shown}");
    assert!(is_short_hash(hash_after(&shown, "h:")), "{shown}");
    assert!(!shown.contains("db.internal.example"), "{shown}");
    assert!(io.diagnostic().contains("db.internal.example:5432"));

    // 2000 characters of server text with an escape sequence in it.
    let server_text = format!("ERROR \u{1b}[31m{}\r\nforged line", "x".repeat(2000));
    let parse = ConnectorError::Parse(server_text.clone());
    let shown = parse.to_string();
    assert_eq!(parse.class(), "parse");
    assert!(
        shown.chars().count() <= 120,
        "{} characters",
        shown.chars().count()
    );
    assert!(!shown.chars().any(char::is_control), "{shown:?}");
    assert!(parse.diagnostic().contains(&server_text));

    // A denied egress names the host only by its hash.
    let denied = ConnectorError::SandboxDenied {
        host: "feeds.partner.example:443".into(),
    };
    assert_eq!(denied.class(), "sandbox-denied");
    assert!(!denied.to_string().contains("partner"), "{denied}");
    assert_eq!(
        hash_after(&denied.to_string(), "h:"),
        redact::host("feeds.partner.example:443").trim_start_matches("h:")
    );
}
