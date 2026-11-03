# pq-mtls fuzz target

A libFuzzer harness for `pq_mtls::decode_message`, the decoder every mesh listener runs
on a peer's first frames before any certificate is checked. It is a developer and
release-runner job, not a CI stage: cargo-fuzz needs a nightly toolchain and a network
connection to fetch `libfuzzer-sys`, and the gate builds offline on the pinned stable
toolchain. The workspace excludes this directory, so the offline build never sees it.
[NFR-SEC-1] [VLD-ANB-MIDGARD-0010]

## Running it

```sh
cargo +nightly install cargo-fuzz
cd crypto/pq-mtls/fuzz
cargo +nightly fuzz run --fuzz-dir . decode_message -- -max_total_time=600
```

Run it from this directory: Cargo finds `.cargo/config.toml` from the directory it is
started in, and the one here is what points crates.io at the live index instead of the
repository's vendored crates.

The release runner runs it for at least ten minutes before a release and attaches the
summary (docs/ops/RUNBOOK_ga_gate.md).

## Output

- `corpus/decode_message/`: the inputs libFuzzer has found interesting, reused on the
  next run.
- `artifacts/decode_message/`: any input that made the target crash, panic, time out or
  run out of memory.

Both are ignored by git. A crash is a security finding: report it privately as
SECURITY.md describes, with the artifact attached, and do not commit it.
