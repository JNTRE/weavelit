//! Proves the administration gate and its proofs cannot be bypassed externally.
//!
//! These are compile-time properties: the entry point requires the existing
//! authorization proof, while current-session and step-up construction require
//! trusted Server composition or private fields. Each detached fixture pins the
//! exact rustc code and its own violating source span.

use std::path::{Path, PathBuf};

use serde_json::Value;

const FORBIDDEN_FIXTURES: [(&str, &str); 8] = [
    ("admission-literal", "E0451"),
    ("authority-reexport", "E0603"),
    ("authorized-action-literal", "E0451"),
    ("invoke-without-authorization", "E0308"),
    ("projection-literal", "E0451"),
    ("projection-sensitive-field", "E0609"),
    ("session-literal", "E0451"),
    ("step-up-literal", "E0451"),
];

#[test]
fn external_callers_cannot_bypass_the_administration_boundary() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let target_root = std::env::temp_dir().join(format!(
        "weavelit-server-administration-fixtures-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&target_root);

    for (binary, expected_code) in FORBIDDEN_FIXTURES {
        assert_forbidden_fixture_rejected(&fixture_root, &target_root, binary, expected_code);
    }

    let _ = std::fs::remove_dir_all(&target_root);
    let _ = std::fs::remove_file(fixture_root.join("forbidden-administration/Cargo.lock"));
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
        .arg(fixture_root.join("forbidden-administration/Cargo.toml"))
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
    let errors = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("Cargo must emit JSON messages"))
        .filter(|message| {
            message["reason"] == "compiler-message" && message["message"]["level"] == "error"
        })
        .collect::<Vec<_>>();

    let expected_source = format!("src/{}.rs", binary.replace('-', "_"));
    assert!(
        errors.iter().any(|error| {
            error["message"]["code"]["code"].as_str() == Some(expected_code)
                && error["message"]["spans"].as_array().is_some_and(|spans| {
                    spans.iter().any(|span| {
                        span["is_primary"] == true && span["file_name"] == expected_source
                    })
                })
        }),
        "forbidden {binary} fixture did not emit {expected_code} at its violating source span: {:?}",
        errors
            .iter()
            .map(|error| error["message"]["message"].as_str())
            .collect::<Vec<_>>()
    );
}
