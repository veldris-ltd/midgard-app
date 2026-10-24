//! `bifrost-server` — Bifrost's control plane: the fleet manager and the artefact
//! registry as one mesh service. See [`bifrost_fleet_server::server`]. [FR-B-1] [FR-B-3]

fn main() {
    bifrost_fleet_server::server::main("bifrost-server");
}
