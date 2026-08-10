//! The committed fixtures are reproducible, immutable, known-answer vectors.

mod support;

use std::collections::BTreeMap;

use serde::Deserialize;
use support::{MANIFEST_NAME, committed, digest, generate};

#[derive(Deserialize)]
struct Manifest {
    format_version: u32,
    fixtures: BTreeMap<String, ManifestEntry>,
}

#[derive(Deserialize)]
struct ManifestEntry {
    length: usize,
    sha256: String,
}

fn manifest() -> Manifest {
    serde_json::from_slice(&committed(MANIFEST_NAME)).expect("the manifest is canonical JSON")
}

#[test]
fn generator_reproduces_every_committed_fixture_byte_for_byte() {
    for fixture in generate().files {
        let committed = committed(fixture.name);
        assert_eq!(
            digest(&committed),
            digest(&fixture.bytes),
            "regenerated fixture {} differs from the committed bytes",
            fixture.name
        );
        assert_eq!(committed, fixture.bytes);
    }
}

#[test]
fn generator_reproduces_the_committed_manifest() {
    assert_eq!(committed(MANIFEST_NAME), generate().manifest);
}

#[test]
fn manifest_pins_every_committed_fixture_length_and_digest() {
    let manifest = manifest();
    assert_eq!(manifest.format_version, 1);

    let generated = generate();
    assert_eq!(manifest.fixtures.len(), generated.files.len());

    for fixture in generated.files {
        let entry = manifest
            .fixtures
            .get(fixture.name)
            .unwrap_or_else(|| panic!("the manifest records {}", fixture.name));
        let bytes = committed(fixture.name);
        assert_eq!(entry.length, bytes.len(), "{} length", fixture.name);
        assert_eq!(entry.sha256, digest(&bytes), "{} digest", fixture.name);
    }
}

#[test]
fn each_negative_artifact_differs_from_the_valid_artifact() {
    let valid = committed("valid.wlitbackup");
    for name in [
        "bad-magic.wlitbackup",
        "non-zero-flags.wlitbackup",
        "tampered-ciphertext.wlitbackup",
        "tampered-tag.wlitbackup",
        "truncated-stream.wlitbackup",
        "wrong-declared-length.wlitbackup",
        "wrong-inner-version.wlitbackup",
        "wrong-outer-version.wlitbackup",
        "wrong-source-backend.wlitbackup",
    ] {
        assert_ne!(
            committed(name),
            valid,
            "{name} must differ from the valid artifact"
        );
    }
}
