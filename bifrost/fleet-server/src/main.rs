//! `bifrost-fleet` — the earlier name of `bifrost-server`, kept as an alias so existing
//! deployments (`deploy/compose`, `verify-bifrost`) keep working. It serves the same:
//! see [`bifrost_fleet_server::server`]. [FR-B-3]

fn main() {
    bifrost_fleet_server::server::main("bifrost-fleet");
}
