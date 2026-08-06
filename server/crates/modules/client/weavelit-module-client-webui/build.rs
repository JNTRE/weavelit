//! Fails the build with an actionable diagnostic when the generated Web UI
//! production assets this crate embeds are absent.
//!
//! The generated output is deliberately not committed, so a fresh checkout must
//! build the Web UI before the Rust workspace. This script never invokes a
//! package manager, network, or code generator; it only reports the missing
//! inputs and the command that produces them.

use std::{env, path::PathBuf, process};

const EMBEDDED_ASSETS: [&str; 3] = [
    "index.html",
    "assets/application.js",
    "assets/application.css",
];

fn main() {
    let manifest_directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"));
    let distribution = manifest_directory
        .ancestors()
        .nth(4)
        .expect("the crate manifest lives four levels below the Server root")
        .join("web-ui/dist");

    let mut missing = Vec::new();
    for asset in EMBEDDED_ASSETS {
        let path = distribution.join(asset);
        println!("cargo:rerun-if-changed={}", path.display());
        if !path.is_file() {
            missing.push(path);
        }
    }

    if missing.is_empty() {
        return;
    }

    eprintln!("The generated Weavelit Web UI production assets are missing:");
    for path in missing {
        eprintln!("  {}", path.display());
    }
    eprintln!(
        "Build them before building the Rust workspace:\n  make -C server check-web-ui\nThe Web UI \
         build output is intentionally not committed."
    );
    process::exit(1);
}
