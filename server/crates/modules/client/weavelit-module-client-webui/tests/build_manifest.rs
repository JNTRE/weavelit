//! Tests the build-time freshness check for the embedded Web UI assets.
//!
//! The build script's module is compiled directly so the same code that gates
//! compilation is the code under test.

#[path = "../build_manifest.rs"]
mod build_manifest;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use build_manifest::{
    MANIFEST_FILE_NAME, MANIFEST_FORMAT_VERSION, collect_input_inventory, verify,
};

struct WebUi {
    root: PathBuf,
}

impl Drop for WebUi {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl WebUi {
    /// Creates a minimal Web UI tree with a manifest that matches its contents.
    fn fresh() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let root = std::env::temp_dir().join(format!(
            "weavelit-webui-manifest-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("dist/assets")).expect("temporary dist directory");
        fs::create_dir_all(root.join("src/nested")).expect("temporary source directory");
        fs::create_dir_all(root.join("src/styles")).expect("temporary styles directory");

        let web_ui = Self { root };
        web_ui.write("index.html", "<!doctype html>");
        web_ui.write("package.json", "{\"name\":\"fixture\"}");
        web_ui.write("package-lock.json", "{\"lockfileVersion\":3}");
        web_ui.write("tsconfig.json", "{\"compilerOptions\":{}}");
        web_ui.write("vite.config.ts", "export default {};");
        web_ui.write("src/main.tsx", "export const main = 1;");
        web_ui.write(
            "src/styles/weavelit-application.css",
            ":root { color: black; }",
        );
        web_ui.write("src/nested/helper.ts", "export const helper = 1;");
        web_ui.write("src/main.test.tsx", "test-only");
        web_ui.write("src/test-setup.ts", "test-only");
        web_ui.write("dist/index.html", "<!doctype html>built");
        web_ui.write("dist/assets/weavelit-application.js", "console.log(1);");
        web_ui.write("dist/assets/weavelit-application.css", "body{}");
        web_ui.write_manifest();
        web_ui
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn write(&self, relative: &str, contents: &str) {
        fs::write(self.path(relative), contents).expect("temporary file write");
    }

    fn write_manifest(&self) {
        let inputs = hashes(
            &self.root,
            &collect_input_inventory(&self.root).expect("inventory"),
        );
        let assets = hashes(
            &self.root.join("dist"),
            &build_manifest::GENERATED_ASSETS.map(str::to_owned),
        );
        self.write_manifest_json(&format!(
            "{{\"format_version\":{MANIFEST_FORMAT_VERSION},\"inputs\":{{{inputs}}},\"assets\":\
             {{{assets}}}}}"
        ));
    }

    fn write_manifest_json(&self, contents: &str) {
        fs::write(self.path("dist").join(MANIFEST_FILE_NAME), contents).expect("manifest write");
    }

    fn manifest_bytes(&self) -> Vec<u8> {
        fs::read(self.path("dist").join(MANIFEST_FILE_NAME)).expect("manifest read")
    }
}

fn hashes(base: &Path, names: &[String]) -> String {
    names
        .iter()
        .map(|name| {
            let bytes = fs::read(base.join(name)).expect("fixture read");
            let digest = <sha2::Sha256 as sha2::Digest>::digest(&bytes);
            let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("\"{name}\":\"{hex}\"")
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn failures(web_ui: &WebUi) -> Vec<String> {
    verify(&web_ui.root).expect_err("verification must fail")
}

#[test]
fn input_inventory_is_sorted_and_excludes_test_only_sources() {
    let web_ui = WebUi::fresh();
    assert_eq!(
        collect_input_inventory(&web_ui.root).expect("inventory"),
        vec![
            "index.html",
            "package-lock.json",
            "package.json",
            "src/main.tsx",
            "src/nested/helper.ts",
            "src/styles/weavelit-application.css",
            "tsconfig.json",
            "vite.config.ts",
        ]
    );
}

#[test]
fn a_matching_manifest_verifies() {
    let web_ui = WebUi::fresh();
    assert_eq!(verify(&web_ui.root), Ok(()));
}

#[test]
fn an_absent_manifest_fails_closed() {
    let web_ui = WebUi::fresh();
    fs::remove_file(web_ui.path("dist").join(MANIFEST_FILE_NAME)).expect("manifest removal");
    let reported = failures(&web_ui);
    assert_eq!(reported.len(), 1);
    assert!(
        reported[0].starts_with("the build content manifest is missing or unreadable:"),
        "{reported:?}"
    );
}

#[test]
fn malformed_manifest_json_fails_closed() {
    let web_ui = WebUi::fresh();
    web_ui.write_manifest_json("{ not json");
    let reported = failures(&web_ui);
    assert_eq!(reported.len(), 1);
    assert!(
        reported[0].starts_with("the build content manifest is not valid JSON:"),
        "{reported:?}"
    );
}

#[test]
fn a_non_object_manifest_fails_closed() {
    let web_ui = WebUi::fresh();
    web_ui.write_manifest_json("[]");
    assert_eq!(
        failures(&web_ui),
        vec!["the build content manifest is not a JSON object"]
    );
}

#[test]
fn an_unrecognized_format_version_fails_closed() {
    let web_ui = WebUi::fresh();
    web_ui.write_manifest_json("{\"format_version\":2,\"inputs\":{},\"assets\":{}}");
    assert_eq!(
        failures(&web_ui),
        vec!["the build content manifest format version is not recognized: 2 (expected 1)"]
    );
}

#[test]
fn an_unrecognized_manifest_field_fails_closed() {
    let web_ui = WebUi::fresh();
    web_ui
        .write_manifest_json("{\"format_version\":1,\"inputs\":{},\"assets\":{},\"blessed\":true}");
    assert_eq!(
        failures(&web_ui),
        vec!["the build content manifest has unrecognized fields: blessed"]
    );
}

#[test]
fn an_edited_bundle_input_fails_closed() {
    let web_ui = WebUi::fresh();
    web_ui.write("src/main.tsx", "export const main = 2;");
    assert_eq!(
        failures(&web_ui),
        vec!["the bundle input `src/main.tsx` changed after the Web UI was built"]
    );
}

#[test]
fn an_edited_test_only_source_still_verifies() {
    let web_ui = WebUi::fresh();
    web_ui.write("src/main.test.tsx", "test-only, edited");
    web_ui.write("src/test-setup.ts", "test-only, edited");
    assert_eq!(verify(&web_ui.root), Ok(()));
}

#[test]
fn a_corrupted_generated_asset_fails_closed() {
    let web_ui = WebUi::fresh();
    web_ui.write("dist/assets/weavelit-application.js", "console.log(2);");
    assert_eq!(
        failures(&web_ui),
        vec![
            "the generated asset `assets/weavelit-application.js` changed after the Web UI was built"
        ]
    );
}

#[test]
fn an_added_bundle_input_fails_closed() {
    let web_ui = WebUi::fresh();
    web_ui.write("src/added.ts", "export const added = 1;");
    assert_eq!(
        failures(&web_ui),
        vec!["the build content manifest has no recorded bundle input: src/added.ts"]
    );
}

#[test]
fn a_removed_bundle_input_fails_closed() {
    let web_ui = WebUi::fresh();
    fs::remove_file(web_ui.path("src/nested/helper.ts")).expect("source removal");
    assert_eq!(
        failures(&web_ui),
        vec![
            "the build content manifest records a bundle input that no longer exists: \
             src/nested/helper.ts"
        ]
    );
}

#[test]
fn a_manifest_hash_that_is_not_a_digest_fails_closed() {
    let web_ui = WebUi::fresh();
    web_ui.write_manifest_json(
        "{\"format_version\":1,\"inputs\":{\"index.html\":\"\"},\"assets\":{}}",
    );
    let reported = failures(&web_ui);
    assert!(
        reported.iter().any(|failure| failure
            == "the build content manifest entry `inputs.index.html` is not a SHA-256 hex \
                    digest"),
        "{reported:?}"
    );
}

#[test]
fn verification_never_rewrites_the_manifest() {
    let web_ui = WebUi::fresh();
    let before = web_ui.manifest_bytes();
    web_ui.write("src/main.tsx", "export const main = 3;");

    assert!(verify(&web_ui.root).is_err());
    assert_eq!(web_ui.manifest_bytes(), before);
    assert!(verify(&web_ui.root).is_err());
}

#[test]
fn every_bundle_input_and_generated_asset_triggers_a_rebuild() {
    let web_ui = WebUi::fresh();
    let watched = build_manifest::watched_paths(&web_ui.root);

    for expected in [
        web_ui.path("src"),
        web_ui.path("src/main.tsx"),
        web_ui.path("vite.config.ts"),
        web_ui.path("package-lock.json"),
        web_ui.path("dist").join(MANIFEST_FILE_NAME),
        web_ui.path("dist/assets/weavelit-application.js"),
    ] {
        assert!(watched.contains(&expected), "{expected:?} is not watched");
    }
    assert!(!watched.contains(&web_ui.path("src/main.test.tsx")));
}
