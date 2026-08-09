//! Fails the build with an actionable diagnostic when the generated Web UI
//! production assets this crate embeds are absent or stale.
//!
//! The generated output is deliberately not committed, so a fresh checkout must
//! build the Web UI before the Rust workspace. Presence alone is not enough: a
//! `dist/` produced before a later source edit would silently embed outdated
//! bytes, so this script also re-hashes the declared bundle inputs and the
//! generated assets and compares them against the build content manifest.
//!
//! This script is hermetic. It never invokes a package manager, performs
//! network access, or writes outside Cargo's output directory.

mod build_manifest;

use std::{env, path::PathBuf, process};

const REMEDIATION: &str = "Rebuild the Web UI before building the Rust workspace:\n  make -C \
                           server check-web-ui\nThe Web UI build output is intentionally not \
                           committed.";

fn main() {
    let manifest_directory =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"));
    let web_ui_root = manifest_directory
        .ancestors()
        .nth(4)
        .expect("the crate manifest lives four levels below the Server root")
        .join("web-ui");
    let distribution = web_ui_root.join("dist");

    for path in build_manifest::watched_paths(&web_ui_root) {
        println!("cargo:rerun-if-changed={}", path.display());
    }

    let missing: Vec<PathBuf> = build_manifest::GENERATED_ASSETS
        .iter()
        .map(|asset| distribution.join(asset))
        .filter(|path| !path.is_file())
        .collect();
    if !missing.is_empty() {
        eprintln!("The generated Weavelit Web UI production assets are missing:");
        for path in missing {
            eprintln!("  {}", path.display());
        }
        eprintln!("{REMEDIATION}");
        process::exit(1);
    }

    if let Err(failures) = build_manifest::verify(&web_ui_root) {
        eprintln!("The embedded Weavelit Web UI production assets are not current:");
        for failure in failures {
            eprintln!("  - {failure}");
        }
        eprintln!("{REMEDIATION}");
        process::exit(1);
    }
}
