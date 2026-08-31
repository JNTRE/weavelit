//! Proves an external consumer cannot access the active Audit generation resolver.

use std::{
    path::{Path, PathBuf},
    process::Output,
};

use serde_json::Value;
use tempfile::TempDir;

const FIXTURE_BINARY: &str = "weavelit-server-forbidden-audit-generation-resolver-fixture";

struct FixtureLockfileGuard {
    path: PathBuf,
    was_absent: bool,
}

impl FixtureLockfileGuard {
    fn require_absent(path: PathBuf) -> Self {
        let was_absent = !path.exists();
        assert!(
            was_absent,
            "the forbidden Audit generation resolver fixture must not have a Cargo.lock: {}",
            path.display()
        );
        Self { path, was_absent }
    }

    fn remove_generated(&self) {
        if self.was_absent && self.path.exists() {
            std::fs::remove_file(&self.path).unwrap_or_else(|error| {
                panic!(
                    "the generated forbidden fixture Cargo.lock must be removable: {}: {error}",
                    self.path.display()
                )
            });
        }
        assert!(
            !self.path.exists(),
            "the forbidden Audit generation resolver fixture Cargo.lock must be absent after the test: {}",
            self.path.display()
        );
    }
}

impl Drop for FixtureLockfileGuard {
    fn drop(&mut self) {
        if self.was_absent && self.path.exists() {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[test]
fn audit_generation_resolver_remains_server_private() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/forbidden-audit-generation-resolver");
    let fixture_manifest = fixture_root.join("Cargo.toml");
    let fixture_lockfile = FixtureLockfileGuard::require_absent(fixture_root.join("Cargo.lock"));
    let target_directory: TempDir = tempfile::tempdir()
        .expect("the forbidden Audit generation resolver fixture target directory must be created");
    let command_args = vec![
        "check".to_owned(),
        "--offline".to_owned(),
        "--quiet".to_owned(),
        "--message-format=json".to_owned(),
        "--manifest-path".to_owned(),
        fixture_manifest.display().to_string(),
        "--bin".to_owned(),
        FIXTURE_BINARY.to_owned(),
    ];

    let forbidden = std::process::Command::new(env!("CARGO"))
        .args(&command_args)
        .current_dir(&fixture_root)
        .env("CARGO_TARGET_DIR", target_directory.path())
        .output()
        .unwrap_or_else(|error| {
            panic!(
                "the forbidden Audit generation resolver fixture must run: {error}; {}",
                command_context(
                    &fixture_root,
                    &fixture_manifest,
                    target_directory.path(),
                    &command_args,
                )
            )
        });
    let context = diagnostic_context(
        &forbidden,
        &fixture_root,
        &fixture_manifest,
        target_directory.path(),
        &command_args,
    );

    assert!(
        !forbidden.status.success(),
        "the forbidden Audit generation resolver fixture unexpectedly compiled; {context}"
    );
    let stdout = String::from_utf8_lossy(&forbidden.stdout);
    let error_messages = stdout
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|error| panic!("Cargo must emit JSON messages: {error}; {context}"))
        })
        .filter(|message| {
            message["reason"] == "compiler-message" && message["message"]["level"] == "error"
        })
        .collect::<Vec<_>>();
    assert_eq!(
        error_messages.len(),
        1,
        "the fixture must fail only because the resolver module is private; {context}"
    );
    let error = &error_messages[0];
    assert_eq!(
        error["message"]["code"]["code"].as_str(),
        Some("E0603"),
        "the fixture must fail with E0603; {context}"
    );
    let primary_spans = error["message"]["spans"]
        .as_array()
        .unwrap_or_else(|| panic!("the E0603 diagnostic must include spans; {context}"))
        .iter()
        .filter(|span| span["is_primary"] == true)
        .collect::<Vec<_>>();
    assert_eq!(
        primary_spans.len(),
        1,
        "the E0603 diagnostic must have exactly one primary span; {context}"
    );
    let primary_span = primary_spans[0];
    assert_eq!(
        primary_span["file_name"].as_str(),
        Some("src/main.rs"),
        "the E0603 diagnostic must point to the forbidden fixture source; {context}"
    );
    assert_eq!(
        primary_span["line_start"].as_u64(),
        Some(1),
        "the E0603 diagnostic must point to the forbidden import line; {context}"
    );
    assert_eq!(
        primary_span["line_end"].as_u64(),
        Some(1),
        "the E0603 diagnostic must end on the forbidden import line; {context}"
    );
    assert_eq!(
        primary_span["column_start"].as_u64(),
        Some(22),
        "the E0603 diagnostic must start at the private operational_audit module; {context}"
    );
    assert_eq!(
        primary_span["column_end"].as_u64(),
        Some(39),
        "the E0603 diagnostic must end at the private operational_audit module; {context}"
    );

    fixture_lockfile.remove_generated();
}

fn command_context(
    fixture_root: &Path,
    fixture_manifest: &Path,
    target_directory: &Path,
    command_args: &[String],
) -> String {
    format!(
        "cargo_executable={}; args={command_args:?}; cwd={}; manifest={}; bin={FIXTURE_BINARY}; cargo_target_dir={}",
        env!("CARGO"),
        fixture_root.display(),
        fixture_manifest.display(),
        target_directory.display(),
    )
}

fn diagnostic_context(
    output: &Output,
    fixture_root: &Path,
    fixture_manifest: &Path,
    target_directory: &Path,
    command_args: &[String],
) -> String {
    format!(
        "status={}; {}; stdout={}; stderr={}",
        output.status,
        command_context(
            fixture_root,
            fixture_manifest,
            target_directory,
            command_args,
        ),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}
