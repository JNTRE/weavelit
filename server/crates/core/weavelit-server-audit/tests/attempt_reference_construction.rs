//! Proves an acknowledged Audit Attempt reference cannot be forged externally.

use std::path::PathBuf;

use serde_json::Value;

#[test]
fn an_audit_attempt_reference_cannot_be_constructed_outside_the_producer() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/forbidden-attempt-reference");
    let target_root = std::env::temp_dir().join(format!(
        "weavelit-server-audit-fixture-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&target_root);

    let forbidden = std::process::Command::new(env!("CARGO"))
        .arg("check")
        .arg("--offline")
        .arg("--quiet")
        .arg("--message-format=json")
        .arg("--manifest-path")
        .arg(fixture_root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_root)
        .output()
        .expect("the forbidden external fixture must run");

    assert!(
        !forbidden.status.success(),
        "the forbidden Audit Attempt reference fixture unexpectedly compiled"
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

    assert_eq!(
        errors.len(),
        1,
        "the fixture must fail for exactly one reason"
    );
    let diagnostic = &errors[0]["message"];
    assert_eq!(diagnostic["code"]["code"].as_str(), Some("E0624"));
    assert_eq!(
        diagnostic["message"].as_str(),
        Some("associated function `new` is private")
    );
    assert!(
        diagnostic["spans"].as_array().is_some_and(|spans| {
            spans.iter().any(|span| {
                span["is_primary"] == true
                    && span["file_name"] == "src/main.rs"
                    && span["line_start"] == 4
                    && span["line_end"] == 4
                    && span["column_start"] == 42
                    && span["column_end"] == 45
            })
        }),
        "the private-constructor diagnostic must pin the exact forging source span"
    );

    let _ = std::fs::remove_dir_all(target_root);
    let _ = std::fs::remove_file(fixture_root.join("Cargo.lock"));
}
