//! Proves an ordinary consumer cannot forge recovery authority or obligation contents.

use std::path::PathBuf;

use serde_json::Value;

#[test]
fn terminal_recovery_authority_and_obligations_cannot_be_forged_externally() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/forbidden-terminal-recovery-authority");
    let target_root = std::env::temp_dir().join(format!(
        "weavelit-server-database-recovery-authority-fixture-{}",
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
        .expect("the forbidden recovery-authority fixture must run");

    assert!(
        !forbidden.status.success(),
        "the forbidden recovery-authority fixture unexpectedly compiled"
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

    assert_eq!(errors.len(), 2, "every private-field forgery must fail");
    assert!(
        errors.iter().all(|message| {
            let diagnostic = &message["message"];
            diagnostic["code"]["code"].as_str() == Some("E0451")
                && diagnostic["spans"].as_array().is_some_and(|spans| {
                    spans.iter().any(|span| {
                        span["is_primary"] == true && span["file_name"] == "src/main.rs"
                    })
                })
        }),
        "only private recovery fields may reject the fixture"
    );
    assert!(
        errors.iter().any(|message| {
            message["message"]["spans"].as_array().is_some_and(|spans| {
                spans.iter().any(|span| {
                    span["is_primary"] == true
                        && span["file_name"] == "src/main.rs"
                        && (span["line_start"] == 8 || span["line_start"] == 9)
                        && span["line_start"] == span["line_end"]
                })
            })
        }) && errors.iter().any(|message| {
            message["message"]["spans"].as_array().is_some_and(|spans| {
                spans.iter().any(|span| {
                    span["is_primary"] == true
                        && span["file_name"] == "src/main.rs"
                        && span["line_start"] == 14
                        && span["line_end"] == 14
                })
            })
        }),
        "the diagnostics must identify obligation and authority forging lines"
    );

    let _ = std::fs::remove_dir_all(target_root);
    let _ = std::fs::remove_file(fixture_root.join("Cargo.lock"));
}
