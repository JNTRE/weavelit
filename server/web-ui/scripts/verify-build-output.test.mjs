// Tests the build content manifest write and check modes.
//
// The critical property under test is that check mode never writes or repairs
// the manifest: a stale build must stay failed until it is rebuilt.

import { deepStrictEqual, match, ok, strictEqual } from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, test } from "node:test";

import {
  MANIFEST_FILE_NAME,
  MANIFEST_FORMAT_VERSION,
  buildManifest,
  checkManifest,
  collectInputInventory,
  manifestPath,
  run,
  writeManifest,
} from "./verify-build-output.mjs";

const roots = [];

after(() => {
  for (const root of roots) {
    rmSync(root, { recursive: true, force: true });
  }
});

function createWebUi() {
  const root = mkdtempSync(join(tmpdir(), "weavelit-web-ui-"));
  roots.push(root);
  const dist = join(root, "dist");
  mkdirSync(join(dist, "assets"), { recursive: true });
  mkdirSync(join(root, "src", "nested"), { recursive: true });

  mkdirSync(join(root, "src", "styles"), { recursive: true });

  writeFileSync(join(root, "index.html"), '<!doctype html><div id="weavelit-root"></div>');
  writeFileSync(join(root, "package.json"), '{"name":"fixture"}');
  writeFileSync(join(root, "package-lock.json"), '{"lockfileVersion":3}');
  writeFileSync(join(root, "tsconfig.json"), '{"compilerOptions":{}}');
  writeFileSync(join(root, "vite.config.ts"), "export default {};");

  writeFileSync(join(root, "src", "main.tsx"), "export const main = 1;");
  writeFileSync(join(root, "src", "styles", "weavelit-application.css"), ":root { color: black; }");
  writeFileSync(join(root, "src", "nested", "helper.ts"), "export const helper = 1;");
  writeFileSync(join(root, "src", "main.test.tsx"), "test-only");
  writeFileSync(join(root, "src", "nested", "helper.test.ts"), "test-only");
  writeFileSync(join(root, "src", "test-setup.ts"), "test-only");

  writeFileSync(join(dist, "index.html"), "<!doctype html>built");
  writeFileSync(join(dist, "assets", "weavelit-application.js"), "console.log(1);");
  writeFileSync(join(dist, "assets", "weavelit-groups-workspace.js"), "console.log(2);");
  writeFileSync(join(dist, "assets", "weavelit-application.css"), "body{}");

  return { root, dist };
}

const silent = { log: () => {}, error: () => {} };

test("the input inventory is deterministic and excludes test-only sources", () => {
  const { root } = createWebUi();
  deepStrictEqual(collectInputInventory(root), [
    "index.html",
    "package-lock.json",
    "package.json",
    "src/main.tsx",
    "src/nested/helper.ts",
    "src/styles/weavelit-application.css",
    "tsconfig.json",
    "vite.config.ts",
  ]);
});

test("write mode records the format version, inputs, and generated assets", () => {
  const { root, dist } = createWebUi();
  strictEqual(run(["--write"], root, dist, silent), 0);

  const manifest = JSON.parse(readFileSync(manifestPath(dist), "utf8"));
  strictEqual(manifest.format_version, MANIFEST_FORMAT_VERSION);
  deepStrictEqual(Object.keys(manifest.inputs), collectInputInventory(root));
  deepStrictEqual(Object.keys(manifest.assets), [
    "assets/weavelit-application.css",
    "assets/weavelit-application.js",
    "assets/weavelit-groups-workspace.js",
    "index.html",
  ]);
  for (const digest of Object.values(manifest.assets)) {
    match(digest, /^[0-9a-f]{64}$/);
  }
});

test("a manifest written from the current tree checks clean", () => {
  const { root, dist } = createWebUi();
  writeManifest(root, dist);
  deepStrictEqual(checkManifest(root, dist), []);
  strictEqual(run(["--check"], root, dist, silent), 0);
});

test("check mode reports an edited bundle input", () => {
  const { root, dist } = createWebUi();
  writeManifest(root, dist);
  writeFileSync(join(root, "src", "main.tsx"), "export const main = 2;");

  const failures = checkManifest(root, dist);
  deepStrictEqual(failures, [
    "Build content manifest bundle input hash does not match the current file: src/main.tsx",
  ]);
});

test("check mode ignores an edited test-only source", () => {
  const { root, dist } = createWebUi();
  writeManifest(root, dist);
  writeFileSync(join(root, "src", "main.test.tsx"), "test-only, edited");

  deepStrictEqual(checkManifest(root, dist), []);
});

test("check mode reports an added and a removed bundle input", () => {
  const { root, dist } = createWebUi();
  writeManifest(root, dist);
  writeFileSync(join(root, "src", "added.ts"), "export const added = 1;");
  rmSync(join(root, "src", "nested", "helper.ts"));

  deepStrictEqual(checkManifest(root, dist), [
    "Build content manifest has no recorded bundle input: src/added.ts",
    "Build content manifest records a bundle input that no longer exists: src/nested/helper.ts",
  ]);
});

test("check mode reports a corrupted generated asset", () => {
  const { root, dist } = createWebUi();
  writeManifest(root, dist);
  writeFileSync(join(dist, "assets", "weavelit-groups-workspace.js"), "console.log(3);");

  deepStrictEqual(checkManifest(root, dist), [
    "Build content manifest generated asset hash does not match the current file: assets/weavelit-groups-workspace.js",
  ]);
});

test("check mode reports a missing manifest", () => {
  const { root, dist } = createWebUi();
  const failures = checkManifest(root, dist);
  strictEqual(failures.length, 1);
  match(failures[0], /^Build content manifest is missing or unreadable: /);
});

test("check mode reports malformed manifest JSON", () => {
  const { root, dist } = createWebUi();
  writeFileSync(manifestPath(dist), "{ not json");

  const failures = checkManifest(root, dist);
  strictEqual(failures.length, 1);
  match(failures[0], /^Build content manifest is missing or unreadable: /);
});

test("check mode reports an unrecognized format version", () => {
  const { root, dist } = createWebUi();
  const manifest = buildManifest(root, dist);
  manifest.format_version = MANIFEST_FORMAT_VERSION + 1;
  writeFileSync(manifestPath(dist), JSON.stringify(manifest));

  deepStrictEqual(checkManifest(root, dist), [
    `Build content manifest format version is not recognized: 2 (expected ${MANIFEST_FORMAT_VERSION})`,
  ]);
});

test("check mode never writes, repairs, or refreshes a stale manifest", () => {
  const { root, dist } = createWebUi();
  writeManifest(root, dist);
  writeFileSync(join(root, "src", "main.tsx"), "export const main = 3;");

  const before = readFileSync(manifestPath(dist));
  strictEqual(run(["--check"], root, dist, silent), 1);
  const after = readFileSync(manifestPath(dist));

  ok(before.equals(after), "check mode must leave the manifest byte-for-byte unchanged");
  strictEqual(run(["--check"], root, dist, silent), 1);
});

test("an unknown mode is rejected without touching the manifest", () => {
  const { root, dist } = createWebUi();
  strictEqual(run([], root, dist, silent), 2);
  strictEqual(run(["--refresh"], root, dist, silent), 2);
});

test("the manifest is not treated as a generated asset", () => {
  const { root, dist } = createWebUi();
  writeManifest(root, dist);
  strictEqual(run(["--check"], root, dist, silent), 0);
  ok(!Object.hasOwn(buildManifest(root, dist).assets, MANIFEST_FILE_NAME));
});
