//! AWS Signature Version 4, as S3-compatible object stores require it: a request is
//! signed with HMAC-SHA256 under a key derived from the secret, the date, the region
//! and the service, so the secret itself is never sent. [FR-F-2] [NFR-SEC-1]

use pqc_agility::hash::{hmac_sha256, sha256};

use crate::wire::hex;

/// The SHA-256 of an empty payload, as hex.
pub const EMPTY_PAYLOAD_SHA256: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// An access key and its secret.
#[derive(Debug, Clone, Copy)]
pub struct Credentials<'a> {
    /// The access key id (not secret).
    pub access_key: &'a str,
    /// The secret access key.
    pub secret_key: &'a str,
}

/// A request to sign: the method, the canonical (already encoded) path, the raw query
/// parameters, the headers to sign (lowercase names), and the payload's SHA-256 hex.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// `GET`, `PUT`, …
    pub method: &'a str,
    /// The URI path, each segment already percent-encoded.
    pub path: &'a str,
    /// Query parameters, not yet encoded.
    pub query: &'a [(String, String)],
    /// Headers to sign, with lowercase names. Must include `host` and `x-amz-date`.
    pub headers: &'a [(String, String)],
    /// The payload's SHA-256, as hex.
    pub payload_sha256: &'a str,
}

/// `YYYYMMDDTHHMMSSZ` and `YYYYMMDD` for a Unix time.
pub fn amz_date(unix: u64) -> (String, String) {
    let days = (unix / 86_400) as i64;
    let seconds = unix % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let date = format!("{year:04}{month:02}{day:02}");
    let time = format!(
        "{date}T{:02}{:02}{:02}Z",
        seconds / 3_600,
        seconds % 3_600 / 60,
        seconds % 60
    );
    (time, date)
}

/// Percent-encode everything but the unreserved characters (`A-Z a-z 0-9 - . _ ~`),
/// and `/` too unless `encode_slash` is false.
pub fn uri_encode(value: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for b in value.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(b))
            }
            b'/' if !encode_slash => out.push('/'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The canonical query string: every name and value encoded, sorted.
pub fn canonical_query(query: &[(String, String)]) -> String {
    let mut pairs: Vec<(String, String)> = query
        .iter()
        .map(|(k, v)| (uri_encode(k, true), uri_encode(v, true)))
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// The `Authorization` header value for `request`, signed at `amz_date`
/// (`YYYYMMDDTHHMMSSZ`) for `region` and `service`.
pub fn authorization(
    request: &Request,
    region: &str,
    service: &str,
    amz_date: &str,
    credentials: Credentials,
) -> String {
    let date = &amz_date[..8];
    let mut headers: Vec<(String, String)> = request
        .headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    headers.sort();
    let canonical_headers = headers.iter().fold(String::new(), |mut out, (k, v)| {
        use std::fmt::Write;
        let _ = writeln!(out, "{k}:{v}");
        out
    });
    let signed_headers = headers
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");
    let canonical_request = format!(
        "{}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{}",
        request.method,
        request.path,
        canonical_query(request.query),
        request.payload_sha256
    );
    let scope = format!("{date}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
        hex(&sha256(canonical_request.as_bytes()))
    );
    let k_date = hmac_sha256(
        format!("AWS4{}", credentials.secret_key).as_bytes(),
        date.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, \
         Signature={signature}",
        credentials.access_key
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_formatted_in_utc() {
        assert_eq!(
            amz_date(1_440_938_160),
            ("20150830T123600Z".to_string(), "20150830".to_string())
        );
        assert_eq!(amz_date(0).0, "19700101T000000Z");
        assert_eq!(amz_date(951_782_400).0, "20000229T000000Z");
    }

    #[test]
    fn encoding_keeps_only_unreserved_characters() {
        assert_eq!(uri_encode("a b/c~d", false), "a%20b/c~d");
        assert_eq!(uri_encode("a b/c", true), "a%20b%2Fc");
        assert_eq!(
            canonical_query(&[
                ("prefix".into(), "exports/".into()),
                ("list-type".into(), "2".into())
            ]),
            "list-type=2&prefix=exports%2F"
        );
    }

    #[test]
    fn signing_matches_the_aws_test_suite() {
        // aws-sig-v4-test-suite: get-vanilla.
        let headers = [
            ("host".to_string(), "example.amazonaws.com".to_string()),
            ("x-amz-date".to_string(), "20150830T123600Z".to_string()),
        ];
        let request = Request {
            method: "GET",
            path: "/",
            query: &[],
            headers: &headers,
            payload_sha256: EMPTY_PAYLOAD_SHA256,
        };
        let credentials = Credentials {
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
        };
        assert_eq!(
            authorization(
                &request,
                "us-east-1",
                "service",
                "20150830T123600Z",
                credentials
            ),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }
}
