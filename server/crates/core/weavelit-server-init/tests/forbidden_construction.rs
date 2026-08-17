//! Proves Init's ordering and one-time delivery cannot be broken by a caller.
//!
//! Three properties are compile-time properties rather than run-time checks:
//! the evidence that the trusted authority answered cannot be forged, a
//! checkpoint whose expected proof the caller chose cannot be constructed, and
//! a prepared recovery key cannot be duplicated into a second live value. Each
//! is asserted by compiling an external crate that tries to break it and
//! pinning the exact rustc diagnostic it must fail with.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The forbidden constructions and the rustc code each must be rejected with.
///
/// `E0624` is the private associated function; `E0599` is the absent method.
/// Each fixture touches exactly one forbidden construction, so each produces
/// exactly one structured error.
const FORBIDDEN_FIXTURES: [(&str, &str); 3] = [
    ("authorization-constructor", "E0624"),
    ("checkpoint-constructor", "E0624"),
    ("delivery-clone", "E0599"),
];

#[test]
fn init_ordering_and_one_time_delivery_cannot_be_broken_outside_this_crate() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let target_root = std::env::temp_dir().join(format!(
        "weavelit-server-init-fixtures-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&target_root);

    for (binary, expected_code) in FORBIDDEN_FIXTURES {
        assert_forbidden_fixture_rejected(&fixture_root, &target_root, binary, expected_code);
    }

    let _ = std::fs::remove_dir_all(&target_root);
    let _ = std::fs::remove_file(fixture_root.join("forbidden-init/Cargo.lock"));
}

fn assert_forbidden_fixture_rejected(
    fixture_root: &Path,
    target_root: &Path,
    binary: &str,
    expected_code: &str,
) {
    let forbidden = std::process::Command::new(env!("CARGO"))
        .arg("check")
        .arg("--offline")
        .arg("--quiet")
        .arg("--message-format=json")
        .arg("--manifest-path")
        .arg(fixture_root.join("forbidden-init/Cargo.toml"))
        .arg("--bin")
        .arg(binary)
        .env("CARGO_TARGET_DIR", target_root)
        .output()
        .expect("forbidden external fixture must run");

    assert!(
        !forbidden.status.success(),
        "forbidden {binary} fixture unexpectedly compiled"
    );

    let stdout =
        std::str::from_utf8(&forbidden.stdout).expect("Cargo JSON output must be valid UTF-8");
    let messages = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("Cargo must emit JSON messages"))
        .collect::<Vec<_>>();
    let errors = messages
        .iter()
        .filter(|message| {
            message["reason"] == "compiler-message" && message["message"]["level"] == "error"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        errors.len(),
        1,
        "forbidden {binary} fixture must emit exactly one structured compiler error"
    );

    let diagnostic = &errors[0]["message"];
    assert_eq!(
        diagnostic["code"]["code"].as_str(),
        Some(expected_code),
        "forbidden {binary} fixture emitted an unexpected rustc code"
    );
    let expected_source = format!("src/{}.rs", binary.replace('-', "_"));
    assert!(
        diagnostic["spans"].as_array().is_some_and(|spans| {
            spans
                .iter()
                .any(|span| span["is_primary"] == true && span["file_name"] == expected_source)
        }),
        "forbidden {binary} fixture must identify its forbidden source span"
    );
}
