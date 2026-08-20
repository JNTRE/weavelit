//! Proves an external consumer cannot access the inert Audit generation resolver.

use std::path::PathBuf;

#[test]
fn audit_generation_resolver_remains_server_private() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/forbidden-audit-generation-resolver");
    let target_root = std::env::temp_dir().join(format!(
        "weavelit-server-audit-generation-resolver-fixture-{}",
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
        .expect("the forbidden Audit generation resolver fixture must run");

    assert!(
        !forbidden.status.success(),
        "the forbidden Audit generation resolver fixture unexpectedly compiled"
    );
    let stdout =
        std::str::from_utf8(&forbidden.stdout).expect("Cargo JSON output must be valid UTF-8");
    let error_messages = stdout
        .lines()
        .filter(|line| line.contains("\"level\":\"error\""))
        .collect::<Vec<_>>();
    assert_eq!(
        error_messages.len(),
        1,
        "the fixture must fail only because the resolver module is private"
    );
    let error = error_messages[0];
    assert!(error.contains("\"code\":\"E0603\""), "{error}");
    assert!(error.contains("src/main.rs"), "{error}");

    let _ = std::fs::remove_dir_all(target_root);
    let _ = std::fs::remove_file(fixture_root.join("Cargo.lock"));
}
