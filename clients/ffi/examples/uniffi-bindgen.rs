//! Generate the Swift bindings from the built static library (library mode).
//! [DD-6] [FR-C-1]
//!
//!   cargo run -p midgard-client-ffi --example uniffi-bindgen -- --library <lib.a> --out <dir>
//!
//! Writes midgard_client_ffi.swift, midgard_client_ffiFFI.h and module.modulemap
//! (module `midgard_client_ffiFFI`). The module map is a plain `module`, not UniFFI's
//! `framework module` form: the XCFramework carries a static library with a Headers
//! directory (clients/shells/apple/tools/build-xcframework.sh), and Clang cannot
//! resolve a framework module's header there. Built without UniFFI's
//! `cargo-metadata` feature: no `uniffi.toml` is read, so the defaults apply.

use camino::Utf8PathBuf;
use uniffi_bindgen::bindings::{generate_swift_bindings, SwiftBindingsOptions};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let get = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
            .unwrap_or_else(|| {
                eprintln!("usage: uniffi-bindgen --library <lib.a> --out <dir>");
                std::process::exit(2)
            })
    };
    generate_swift_bindings(SwiftBindingsOptions {
        generate_swift_sources: true,
        generate_headers: true,
        generate_modulemap: true,
        library_path: Utf8PathBuf::from(get("--library")),
        out_dir: Utf8PathBuf::from(get("--out")),
        xcframework: false,
        module_name: Some("midgard_client_ffiFFI".into()),
        modulemap_filename: Some("module.modulemap".into()),
        metadata_no_deps: true,
    })
    .unwrap_or_else(|e| {
        eprintln!("uniffi-bindgen: {e:#}");
        std::process::exit(1)
    });
}
