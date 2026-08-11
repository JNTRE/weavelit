//! Proves a released Init checkpoint cannot act without reauthorization.
//!
//! Init releases the exclusive mutation permit and the database handle between
//! its two requests, so the value it retains across that pause must not be
//! able to complete, acknowledge, or seal anything on its own, and must not be
//! forgeable by a caller. Those are compile-time properties rather than
//! run-time checks, so each is asserted by compiling an external crate that
//! tries to break it and pinning the exact rustc diagnostic it must fail with.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The forbidden constructions and the rustc code each must be rejected with.
///
/// `E0451` is the private field a forged value would have to set; `E0599` is
/// the absent method. Each fixture touches exactly one forbidden construction,
/// so each produces exactly one structured error.
const FORBIDDEN_FIXTURES: [(&str, &str); 3] = [
    ("released-checkpoint-constructor", "E0451"),
    ("released-checkpoint-completion", "E0599"),
    ("released-checkpoint-seal", "E0599"),
];

#[test]
fn a_released_init_checkpoint_cannot_act_outside_this_crate() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let target_root = std::env::temp_dir().join(format!(
        "weavelit-server-lifecycle-fixtures-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&target_root);

    for (binary, expected_code) in FORBIDDEN_FIXTURES {
        assert_forbidden_fixture_rejected(&fixture_root, &target_root, binary, expected_code);
    }

    let _ = std::fs::remove_dir_all(&target_root);
    let _ = std::fs::remove_file(fixture_root.join("forbidden-lifecycle/Cargo.lock"));
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
        .arg(fixture_root.join("forbidden-lifecycle/Cargo.toml"))
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
