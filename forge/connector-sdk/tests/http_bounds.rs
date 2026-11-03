//! The connector HTTP client against an in-process server that sends a hostile
//! response: a chunk size that wraps a `usize`, one that does not fit a `u64` and one
//! larger than what is left of the body budget are each refused as data, quickly and
//! without a panic or a large allocation, and legitimate chunks still reassemble
//! exactly. [FR-F-2] [NFR-SEC-1] [AC-34.1] [AC-34.2] [VLD-ANB-MIDGARD-0009]

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use forge_connector_sdk::http::{self, Response, MAX_BODY};
use forge_connector_sdk::ConnectorError;

/// A server on 127.0.0.1 that answers each connection with `response` once it has
/// read the request, then closes.
fn serve(response: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let response = response.clone();
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 0) && line != "\r\n" {
                    line.clear();
                }
                let mut stream = stream;
                let _ = stream.write_all(&response);
            });
        }
    });
    addr
}

/// `GET /` against a server that sends `response`, on a thread of its own, so a panic
/// in the client is seen rather than taking the test down with it.
fn fetch(response: &[u8]) -> Result<Response, ConnectorError> {
    let addr = serve(response.to_vec());
    let client = thread::spawn(move || {
        let stream = TcpStream::connect(addr).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        http::get(stream, "/", &[("host".to_string(), "fake".to_string())])
    });
    client.join().expect("the HTTP client panicked")
}

fn chunked(chunks: &str) -> Vec<u8> {
    format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{chunks}").into_bytes()
}

fn refused_with(result: Result<Response, ConnectorError>, wording: &str) {
    match result {
        Err(ConnectorError::Parse(m)) => assert!(m.contains(wording), "{m}"),
        other => panic!("expected a parse error naming {wording:?}, got {other:?}"),
    }
}

// [AC-34.1] [VLD-ANB-MIDGARD-0009]
#[test]
fn a_chunk_size_that_wraps_usize_is_refused_as_data() {
    refused_with(
        fetch(&chunked("5\r\nhello\r\nffffffffffffffff\r\n")),
        "too large",
    );
}

// [AC-34.2] [VLD-ANB-MIDGARD-0009]
#[test]
fn a_chunk_size_beyond_u64_is_malformed() {
    let started = Instant::now();
    refused_with(
        fetch(&chunked("5\r\nhello\r\n10000000000000000\r\n")),
        "malformed chunk size",
    );
    assert!(started.elapsed() < Duration::from_secs(2));
}

// [AC-34.2] [VLD-ANB-MIDGARD-0009]
#[test]
fn a_chunk_larger_than_the_remaining_budget_is_refused() {
    let started = Instant::now();
    refused_with(
        fetch(&chunked(&format!("5\r\nhello\r\n{MAX_BODY:x}\r\n"))),
        "too large",
    );
    // Refused before MAX_BODY bytes are allocated or waited for.
    assert!(started.elapsed() < Duration::from_secs(2));
}

// [AC-34.2] [FR-F-2]
#[test]
fn two_legitimate_chunks_reassemble_exactly() {
    let response = fetch(&chunked("5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n")).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"hello world");
}
