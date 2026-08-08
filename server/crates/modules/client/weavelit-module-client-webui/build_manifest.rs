//! Verifies that the generated Web UI assets this crate embeds were produced
//! from the current Web UI sources.
//!
//! `dist/build-manifest.json` is written by the Web UI production build and
//! records the SHA-256 hash of every bundle input and of each generated asset.
//! Re-hashing both sets at compile time turns a stale `dist/` into a build
//! failure instead of a binary that silently serves outdated bytes.
//!
//! This module is build metadata only. The manifest is never embedded, never
//! served, and never written by this crate.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

/// Build metadata file written into the Web UI build output directory.
pub const MANIFEST_FILE_NAME: &str = "build-manifest.json";

/// Only this manifest layout is understood; any other value fails the build.
pub const MANIFEST_FORMAT_VERSION: u64 = 1;

/// The generated browser assets this crate embeds, relative to `dist/`.
pub const GENERATED_ASSETS: [&str; 3] = [
    "assets/weavelit-application.css",
    "assets/weavelit-application.js",
    "index.html",
];

/// Bundle inputs outside `src/`: the Vite entry document and the build,
/// compiler, and dependency configuration that determine the emitted bundle.
const CONFIGURATION_INPUTS: [&str; 5] = [
    "index.html",
    "package-lock.json",
    "package.json",
    "tsconfig.json",
    "vite.config.ts",
];

/// Application sources under this directory are bundle inputs unless they are
/// test-only.
pub const SOURCE_DIRECTORY: &str = "src";

/// Test-only sources never reach the production bundle, so editing one must not
/// fail an otherwise correct build.
const TEST_ONLY_SUFFIXES: [&str; 2] = [".test.ts", ".test.tsx"];
const TEST_ONLY_NAMES: [&str; 1] = ["test-setup.ts"];

/// Returns the deterministic, sorted bundle input paths relative to the Web UI
/// root, using `/` separators so the inventory matches the manifest writer.
pub fn collect_input_inventory(web_ui_root: &Path) -> Result<Vec<String>, String> {
    let mut inventory = Vec::new();
    for name in CONFIGURATION_INPUTS {
        if !web_ui_root.join(name).is_file() {
            return Err(format!("Web UI bundle input is missing: {name}"));
        }
        inventory.push(name.to_owned());
    }
    collect_sources(
        &web_ui_root.join(SOURCE_DIRECTORY),
        SOURCE_DIRECTORY,
        &mut inventory,
    )?;
    inventory.sort();
    Ok(inventory)
}

fn collect_sources(
    directory: &Path,
    prefix: &str,
    inventory: &mut Vec<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("Web UI source directory is unreadable: {prefix} ({error})"))?;
    let mut children = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            format!("Web UI source directory is unreadable: {prefix} ({error})")
        })?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| format!("Web UI source name is not valid UTF-8 under {prefix}"))?;
        children.push((entry.path(), format!("{prefix}/{name}"), name));
    }
    children.sort_by(|left, right| left.1.cmp(&right.1));

    for (path, relative, name) in children {
        if path.is_dir() {
            collect_sources(&path, &relative, inventory)?;
        } else if !is_test_only(&name) {
            inventory.push(relative);
        }
    }
    Ok(())
}

fn is_test_only(name: &str) -> bool {
    TEST_ONLY_SUFFIXES
        .iter()
        .any(|suffix| name.ends_with(suffix))
        || TEST_ONLY_NAMES.contains(&name)
}

fn hash_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|error| format!("cannot read {} for hashing: {error}", path.display()))?;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    Ok(digest.iter().fold(String::with_capacity(64), |mut out, b| {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
        out
    }))
}

fn section<'a>(
    manifest: &'a serde_json::Map<String, serde_json::Value>,
    name: &str,
) -> Result<BTreeMap<String, &'a str>, String> {
    let value = manifest
        .get(name)
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            format!("the build content manifest section `{name}` is missing or not an object")
        })?;
    value
        .iter()
        .map(|(key, hash)| {
            hash.as_str()
                .filter(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(|hash| (key.clone(), hash))
                .ok_or_else(|| {
                    format!(
                        "the build content manifest entry `{name}.{key}` is not a SHA-256 hex \
                         digest"
                    )
                })
        })
        .collect()
}

fn compare(
    label: &str,
    recorded: &BTreeMap<String, &str>,
    base: &Path,
    names: &[String],
    failures: &mut Vec<String>,
) {
    for name in names {
        match recorded.get(name) {
            None => failures.push(format!(
                "the build content manifest has no recorded {label}: {name}"
            )),
            Some(expected) => {
                match hash_file(&base.join(name.replace('/', std::path::MAIN_SEPARATOR_STR))) {
                    Err(error) => failures.push(error),
                    Ok(actual) if actual != *expected => failures.push(format!(
                        "the {label} `{name}` changed after the Web UI was built"
                    )),
                    Ok(_) => {}
                }
            }
        }
    }
    for name in recorded.keys() {
        if !names.contains(name) {
            failures.push(format!(
                "the build content manifest records a {label} that no longer exists: {name}"
            ));
        }
    }
}

/// Verifies the recorded manifest against the current sources and build output.
///
/// Returns every failure so one build reports the complete picture. This only
/// reads: it never repairs, refreshes, or writes the manifest.
pub fn verify(web_ui_root: &Path) -> Result<(), Vec<String>> {
    let distribution = web_ui_root.join("dist");
    let manifest_path = distribution.join(MANIFEST_FILE_NAME);

    let raw = fs::read_to_string(&manifest_path).map_err(|error| {
        vec![format!(
            "the build content manifest is missing or unreadable: {} ({error})",
            manifest_path.display()
        )]
    })?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        vec![format!(
            "the build content manifest is not valid JSON: {error}"
        )]
    })?;
    let manifest = parsed
        .as_object()
        .ok_or_else(|| vec!["the build content manifest is not a JSON object".to_owned()])?;

    let mut unknown: Vec<&str> = manifest
        .keys()
        .map(String::as_str)
        .filter(|key| !matches!(*key, "format_version" | "inputs" | "assets"))
        .collect();
    unknown.sort_unstable();
    if !unknown.is_empty() {
        return Err(vec![format!(
            "the build content manifest has unrecognized fields: {}",
            unknown.join(", ")
        )]);
    }

    match manifest
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
    {
        Some(MANIFEST_FORMAT_VERSION) => {}
        other => {
            return Err(vec![format!(
                "the build content manifest format version is not recognized: {} (expected {})",
                other.map_or_else(|| "absent".to_owned(), |version| version.to_string()),
                MANIFEST_FORMAT_VERSION
            )]);
        }
    }

    let mut failures = Vec::new();
    let inventory = match collect_input_inventory(web_ui_root) {
        Ok(inventory) => inventory,
        Err(error) => return Err(vec![error]),
    };
    let assets: Vec<String> = GENERATED_ASSETS
        .iter()
        .map(|&name| name.to_owned())
        .collect();

    match section(manifest, "inputs") {
        Ok(recorded) => compare(
            "bundle input",
            &recorded,
            web_ui_root,
            &inventory,
            &mut failures,
        ),
        Err(error) => failures.push(error),
    }
    match section(manifest, "assets") {
        Ok(recorded) => compare(
            "generated asset",
            &recorded,
            &distribution,
            &assets,
            &mut failures,
        ),
        Err(error) => failures.push(error),
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

/// Returns every path whose change must trigger a rebuild of this crate.
pub fn watched_paths(web_ui_root: &Path) -> Vec<PathBuf> {
    let distribution = web_ui_root.join("dist");
    let mut watched = vec![
        distribution.join(MANIFEST_FILE_NAME),
        // Watched as a directory so an added or removed source is also noticed.
        web_ui_root.join(SOURCE_DIRECTORY),
    ];
    watched.extend(
        GENERATED_ASSETS
            .iter()
            .map(|asset| distribution.join(asset)),
    );
    watched.extend(
        CONFIGURATION_INPUTS
            .iter()
            .map(|name| web_ui_root.join(name)),
    );
    watched.extend(
        collect_input_inventory(web_ui_root)
            .unwrap_or_default()
            .iter()
            .map(|name| web_ui_root.join(name)),
    );
    watched.sort();
    watched.dedup();
    watched
}
