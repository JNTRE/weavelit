//! Proves the operational request path cannot be reordered or replayed.
//!
//! Selection consumes the authorization proof and execution consumes the
//! selection, so an authorization justifies at most one Operation against at
//! most one Service Connection. Those are compile-time properties, so they are
//! asserted by compiling an external crate that tries to violate each one and
//! pinning the exact rustc diagnostic it must fail with.
//!
//! This mirrors the forbidden-proof fixture in `weavelit-server-authorization`,
//! which pins that a proof cannot be constructed at all. Together they cover
//! both halves: a proof cannot be forged, and a real proof cannot be spent
//! twice or borrowed instead of spent.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The violations and the diagnostic each must be rejected with.
///
/// `E0308` is the reference supplied where the proof must be moved; `E0382` is
/// the use of a value already moved. Refusing a struct literal whose fields are
/// all private carries no rustc code at all, so that one is pinned by its exact
/// message instead; pinning nothing there would let the fixture pass on any
/// compilation failure whatsoever, including a typo in the fixture itself.
const FORBIDDEN_FIXTURES: [(&str, Expected); 4] = [
    ("borrowed-proof", Expected::Code("E0308")),
    ("reused-proof", Expected::Code("E0382")),
    ("reused-selection", Expected::Code("E0382")),
    (
        "selection-literal",
        Expected::Message(
            "cannot construct `SelectedServiceConnection` with struct literal syntax due to private fields",
        ),
    ),
];

/// How a fixture's required diagnostic is identified.
#[derive(Clone, Copy, Debug)]
enum Expected {
    /// A structured rustc error code.
    Code(&'static str),
    /// An exact rustc message, for diagnostics that carry no code.
    Message(&'static str),
}

impl Expected {
    fn matches(self, diagnostic: &Value) -> bool {
        match self {
            Self::Code(code) => diagnostic["code"]["code"].as_str() == Some(code),
            Self::Message(message) => diagnostic["message"].as_str() == Some(message),
        }
    }
}

#[test]
fn an_authorization_is_spent_exactly_once_along_the_operational_path() {
    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let target_root = std::env::temp_dir().join(format!(
        "weavelit-server-operation-fixtures-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&target_root);

    for (binary, expected) in FORBIDDEN_FIXTURES {
        assert_forbidden_fixture_rejected(&fixture_root, &target_root, binary, expected);
    }

    let _ = std::fs::remove_dir_all(&target_root);
    let _ = std::fs::remove_file(fixture_root.join("unconsumed-proof/Cargo.lock"));
}

fn assert_forbidden_fixture_rejected(
    fixture_root: &Path,
    target_root: &Path,
    binary: &str,
    expected: Expected,
) {
    let forbidden = std::process::Command::new(env!("CARGO"))
        .arg("check")
        .arg("--offline")
        .arg("--quiet")
        .arg("--message-format=json")
        .arg("--manifest-path")
        .arg(fixture_root.join("unconsumed-proof/Cargo.toml"))
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

    // The expected diagnostic must be the reason the fixture failed, rather than
    // one error among several, so that a fixture cannot start passing because
    // of an unrelated mistake in the fixture source itself.
    assert!(
        errors
            .iter()
            .any(|error| expected.matches(&error["message"])),
        "forbidden {binary} fixture emitted {:?} rather than {expected:?}",
        errors
            .iter()
            .map(|error| error["message"]["message"].as_str())
            .collect::<Vec<_>>()
    );

    let expected_source = format!("src/{}.rs", binary.replace('-', "_"));
    assert!(
        errors.iter().any(|error| {
            expected.matches(&error["message"])
                && error["message"]["spans"].as_array().is_some_and(|spans| {
                    spans.iter().any(|span| {
                        span["is_primary"] == true && span["file_name"] == expected_source
                    })
                })
        }),
        "forbidden {binary} fixture must identify its violating source span"
    );
}
