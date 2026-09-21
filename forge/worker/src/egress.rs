//! The egress broker: TLS for the worker's connectors, outside the worker. [NFR-SEC-1]
//! [NFR-PRV-1] [FR-F-2]
//!
//! The connectors speak plaintext to a local port; the broker — HAProxy, beside the
//! worker — originates TLS 1.3 from that port to exactly one upstream, verifying the
//! upstream's certificate against a CA pinned for that route and its name. A routes file
//! declares every route:
//!
//! ```json
//! { "version": 1,
//!   "routes": [ { "name": "exports", "listen": 19000,
//!                 "upstream": { "host": "minio.example.org", "port": 9000 },
//!                 "ca": "exports-ca.pem", "serverName": "minio.example.org" } ] }
//! ```
//!
//! `serverName` defaults to the upstream host; `ca` is a file in the broker's CA
//! directory. From the routes file, [`render_haproxy`] writes the broker's whole
//! configuration — nothing else is reachable through it — and, with the worker's
//! `--egress-routes`, [`check_jobs`] refuses any job whose source is not one of those
//! local ports. An undeclared host is unreachable twice.
//!
//! Names, hosts and file names are checked character by character, so nothing in the
//! routes file can inject configuration into HAProxy.

use std::collections::BTreeSet;

use service_runtime::json::{parse, Json};
use service_runtime::strict_json::{array, identifier, object, only_keys, text, unique};

use crate::{Job, Source};

/// The one routes file version this build reads.
pub const ROUTES_FILE_VERSION: i64 = 1;

/// One route: a local port, and the TLS upstream behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// The route's name.
    pub name: String,
    /// The local port the connectors reach.
    pub listen: u16,
    /// The upstream host.
    pub host: String,
    /// The upstream port.
    pub port: u16,
    /// The CA file the upstream's certificate must chain to.
    pub ca: String,
    /// The name the upstream's certificate must carry, sent as SNI.
    pub server_name: String,
}

/// Parse a routes file strictly.
pub fn parse_routes(text_in: &str) -> Result<Vec<Route>, String> {
    let doc = parse(text_in).map_err(|e| format!("not JSON: {e}"))?;
    only_keys(
        object(&doc, "the file")?,
        "the file",
        &["version", "routes"],
    )?;
    match doc.get("version") {
        Some(Json::Int(ROUTES_FILE_VERSION)) => {}
        Some(other) => return Err(format!("unsupported version {}", other.encode())),
        None => return Err("missing \"version\"".into()),
    }
    let routes = array(&doc, "the file", "routes")?
        .iter()
        .enumerate()
        .map(|(i, item)| route(&format!("routes[{i}]"), item))
        .collect::<Result<Vec<_>, _>>()?;
    if routes.is_empty() {
        return Err("\"routes\" declares no routes".into());
    }
    unique(routes.iter().map(|r| r.name.as_str()), "route name")?;
    let mut ports = BTreeSet::new();
    for r in &routes {
        if !ports.insert(r.listen) {
            return Err(format!("two routes listen on port {}", r.listen));
        }
    }
    Ok(routes)
}

fn route(at: &str, item: &Json) -> Result<Route, String> {
    only_keys(
        object(item, at)?,
        at,
        &["name", "listen", "upstream", "ca", "serverName"],
    )?;
    let name = identifier(item, at, "name")?.to_string();
    let at = format!("route {name}");
    let listen = port(item.get("listen"), &at, "listen")?;
    if listen < 1024 {
        return Err(format!(
            "{at}: \"listen\" must be 1024 or above, so the broker needs no privilege"
        ));
    }
    let upstream = item
        .get("upstream")
        .ok_or_else(|| format!("{at}: missing \"upstream\""))?;
    let upstream_at = format!("{at}: upstream");
    only_keys(
        object(upstream, &upstream_at)?,
        &upstream_at,
        &["host", "port"],
    )?;
    let host = hostname(text(upstream, &upstream_at, "host")?, &upstream_at, "host")?;
    let port = port(upstream.get("port"), &upstream_at, "port")?;
    let ca = text(item, &at, "ca")?;
    if ca.starts_with('.')
        || !ca
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
    {
        return Err(format!(
            "{at}: \"ca\" must be a plain file name in the CA directory, not \"{ca}\""
        ));
    }
    let server_name = match item.get("serverName") {
        None => host.clone(),
        Some(_) => hostname(text(item, &at, "serverName")?, &at, "serverName")?,
    };
    Ok(Route {
        name,
        listen,
        host,
        port,
        ca: ca.to_string(),
        server_name,
    })
}

fn port(value: Option<&Json>, at: &str, key: &str) -> Result<u16, String> {
    match value {
        Some(Json::Int(n)) => u16::try_from(*n)
            .ok()
            .filter(|&p| p > 0)
            .ok_or_else(|| format!("{at}: \"{key}\" {n} is not a port")),
        Some(other) => Err(format!(
            "{at}: \"{key}\" must be a number, not {}",
            other.encode()
        )),
        None => Err(format!("{at}: missing \"{key}\"")),
    }
}

/// A DNS name or IPv4 address: letters, digits, `-` and `.`.
fn hostname(value: &str, at: &str, key: &str) -> Result<String, String> {
    let valid = !value.is_empty()
        && value.len() <= 253
        && !value.starts_with(['-', '.'])
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'));
    if valid {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(format!(
            "{at}: \"{key}\" must be a host name or IPv4 address, not \"{value}\""
        ))
    }
}

/// The broker's HAProxy configuration: one TCP frontend on `bind`:listen per route, and
/// one backend originating TLS 1.3 to its upstream, verifying the certificate against
/// the route's CA in `ca_dir` and its server name.
pub fn render_haproxy(routes: &[Route], bind: &str, ca_dir: &str) -> String {
    let mut out = String::from(
        "# Generated by forge-egress from the worker's egress routes. Do not edit: every\n\
         # route here is one the routes file declares, and nothing else is reachable.\n\
         global\n\
         \x20   log stdout format raw local0 info\n\
         \x20   maxconn 512\n\
         \x20   ssl-default-server-options ssl-min-ver TLSv1.3\n\
         \n\
         # Upstream names are resolved from the pod's resolv.conf, and again as they\n\
         # change: an upstream that does not resolve yet is held down, not fatal.\n\
         resolvers system\n\
         \x20   parse-resolv-conf\n\
         \x20   hold valid 30s\n\
         \n\
         defaults\n\
         \x20   mode tcp\n\
         \x20   log global\n\
         \x20   option tcplog\n\
         \x20   timeout connect 10s\n\
         \x20   timeout client 10m\n\
         \x20   timeout server 10m\n\
         \x20   default-server init-addr last,libc,none resolvers system\n",
    );
    for r in routes {
        out.push_str(&format!(
            "\nfrontend {name}\n\
             \x20   bind {bind}:{listen}\n\
             \x20   default_backend {name}\n\
             \nbackend {name}\n\
             \x20   server upstream {host}:{port} ssl verify required ca-file {ca_dir}/{ca} \
             sni str({server_name}) verifyhost {server_name}\n",
            name = r.name,
            listen = r.listen,
            host = r.host,
            port = r.port,
            ca = r.ca,
            server_name = r.server_name,
        ));
    }
    out
}

/// Refuse any job whose source is on the network but is not one of the broker's local
/// ports: with a broker, a connector reaches nothing else.
pub fn check_jobs(jobs: &[Job], routes: &[Route]) -> Result<(), String> {
    for job in jobs {
        let target = match &job.source {
            Source::File { .. } => continue,
            Source::Postgres { host, port, .. }
            | Source::S3 { host, port, .. }
            | Source::Kafka { host, port, .. } => (host, *port),
        };
        let local = matches!(target.0.as_str(), "127.0.0.1" | "localhost");
        if !local || !routes.iter().any(|r| r.listen == target.1) {
            return Err(format!(
                "job {} reaches {}:{}, which is not an egress broker route (a local port the \
                 routes file declares)",
                job.id, target.0, target.1
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const ROUTES: &str = r#"{
      "version": 1,
      "routes": [
        { "name": "exports", "listen": 19000,
          "upstream": { "host": "minio.example.org", "port": 9000 }, "ca": "exports-ca.pem" },
        { "name": "warehouse", "listen": 15432,
          "upstream": { "host": "10.20.0.5", "port": 5432 }, "ca": "pg-ca.pem",
          "serverName": "db.example.org" }
      ]
    }"#;

    #[test]
    fn routes_render_to_a_verifying_tls_broker() {
        let routes = parse_routes(ROUTES).unwrap();
        assert_eq!(routes[0].server_name, "minio.example.org");
        assert_eq!(routes[1].server_name, "db.example.org");
        let config = render_haproxy(&routes, "127.0.0.1", "/etc/egress/ca");
        assert!(config.contains("ssl-min-ver TLSv1.3"));
        assert!(config.contains("default-server init-addr last,libc,none resolvers system"));
        assert!(config.contains("bind 127.0.0.1:19000"));
        assert!(config.contains(
            "server upstream minio.example.org:9000 ssl verify required ca-file \
             /etc/egress/ca/exports-ca.pem sni str(minio.example.org) verifyhost \
             minio.example.org"
        ));
        assert!(config.contains("server upstream 10.20.0.5:5432 ssl verify required"));
        assert!(!config.contains("verify none"));
    }

    #[test]
    fn a_routes_file_that_could_inject_or_collide_is_refused() {
        for (from, to) in [
            (
                "\"minio.example.org\"",
                "\"minio.example.org\\n    server evil 6.6.6.6:1\"",
            ),
            ("\"minio.example.org\"", "\"minio example\""),
            ("\"exports-ca.pem\"", "\"../../etc/ssl/cert.pem\""),
            ("\"db.example.org\"", "\"db.example.org verify none\""),
            ("\"listen\": 15432", "\"listen\": 19000"),
            ("\"listen\": 19000", "\"listen\": 80"),
            ("\"name\": \"warehouse\"", "\"name\": \"exports\""),
            ("\"port\": 9000", "\"port\": 0"),
            ("\"version\": 1", "\"version\": 2"),
            (
                "\"ca\": \"pg-ca.pem\",",
                "\"ca\": \"pg-ca.pem\", \"verify\": false,",
            ),
        ] {
            let bad = ROUTES.replacen(from, to, 1);
            assert_ne!(bad, ROUTES, "{from}");
            assert!(parse_routes(&bad).is_err(), "{to}");
        }
    }

    #[test]
    fn only_jobs_through_the_broker_pass() {
        let routes = parse_routes(ROUTES).unwrap();
        let job = |source: &str| {
            let text = format!(
                r#"{{ "version": 1, "jobs": [ {{ "id": "j", "every": 60,
                  "source": {{ {source}, "columns": [ {{ "name": "k", "type": "text" }} ] }},
                  "objects": {{ "type": "t", "key": "k", "marking": "open",
                                "purposes": ["p"] }} }} ] }}"#
            );
            crate::parse_jobs(&text, Path::new("/data")).unwrap()
        };
        let through = job(
            r#""s3": { "host": "127.0.0.1", "port": 19000, "region": "r", "bucket": "b",
                "accessKey": "a", "secretKey": "s" }"#,
        );
        assert!(check_jobs(&through, &routes).is_ok());
        assert!(check_jobs(&job(r#""file": "x.csv""#), &routes).is_ok());

        let direct = job(
            r#""s3": { "host": "minio.example.org", "port": 9000, "region": "r",
                "bucket": "b", "accessKey": "a", "secretKey": "s" }"#,
        );
        let e = check_jobs(&direct, &routes).unwrap_err();
        assert!(e.contains("minio.example.org:9000"), "{e}");
        let undeclared = job(r#""kafka": { "host": "127.0.0.1", "port": 19092, "topic": "t" }"#);
        assert!(check_jobs(&undeclared, &routes).is_err());
    }
}
