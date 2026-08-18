//! Proves an ordinary Application Database implementor cannot issue decoders.
//!
//! The external fixture implements the public backend-neutral contract, then
//! separately attempts every forbidden authority action. Each attempt must
//! produce exactly one structured rustc diagnostic at its source span.

use std::path::{Path, PathBuf};

use serde_json::Value;

const FORBIDDEN_FIXTURES: [(&str, &str); 5] = [
    ("authority-import", "E0432"),
    ("selected-database-constructor", "E0451"),
    ("lifecycle-private-constructor", "E0624"),
    ("persistence-issuer", "E0599"),
    ("arbitrary-decode", "E0451"),
];

#[test]
fn an_external_database_implementor_cannot_obtain_persistence_authority() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/forbidden-database-authority");
    let target_root = std::env::temp_dir().join(format!(
        "weavelit-server-database-authority-fixtures-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&target_root);

    for (binary, expected_code) in FORBIDDEN_FIXTURES {
        assert_forbidden_fixture_rejected(&fixture_root, &target_root, binary, expected_code);
    }

    let _ = std::fs::remove_dir_all(&target_root);
    let _ = std::fs::remove_file(fixture_root.join("Cargo.lock"));
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
        .arg(fixture_root.join("Cargo.toml"))
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
