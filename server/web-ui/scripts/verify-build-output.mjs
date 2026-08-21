#!/usr/bin/env node
// Validates the production build inventory and raw byte budgets, and maintains
// the build content manifest that proves the generated assets are current.
//
// The Web UI Client Module embeds a fixed compile-time asset allowlist, so a
// missing, renamed, extra, or oversized output is a build failure. Budgets are
// enforced against raw bytes because the Server applies no compression; gzip
// sizes are reported for information only.
//
// Modes:
//   --write  Validate the bundle, then write `dist/build-manifest.json`. Run
//            only immediately after a clean `vite build`.
//   --check  Validate the bundle and re-verify the manifest against the current
//            inputs and outputs. This mode never writes the manifest, so a
//            stale build can never be blessed by re-running verification.

import { createHash } from "node:crypto";
import { gzipSync } from "node:zlib";
import { readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join, posix, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const KIB = 1024;

export const MANIFEST_FILE_NAME = "build-manifest.json";
export const MANIFEST_FORMAT_VERSION = 1;

export const EXPECTED_ASSETS = new Map([
  ["index.html", 16 * KIB],
  ["assets/weavelit-application.js", 256 * KIB],
  ["assets/weavelit-groups-workspace.js", 32 * KIB],
  ["assets/weavelit-application.css", 64 * KIB],
]);

const COMBINED_LIMIT = 336 * KIB;

// Bundle inputs are the files that can change the production bundle: the Vite
// entry document, the non-test application sources, and the build, compiler,
// and dependency configuration. Test-only files are excluded because they never
// reach the production bundle, and including them would make editing a unit
// test fail an otherwise correct Rust build.
const CONFIGURATION_INPUTS = [
  "index.html",
  "package-lock.json",
  "package.json",
  "tsconfig.json",
  "vite.config.ts",
];

const SOURCE_DIRECTORY = "src";

const TEST_ONLY_SUFFIXES = [".test.ts", ".test.tsx"];
const TEST_ONLY_NAMES = ["test-setup.ts"];

const webUiRootDefault = fileURLToPath(new URL("..", import.meta.url));

function toPosix(value) {
  return value.split(sep).join(posix.sep);
}

function listFiles(directory) {
  const found = [];
  for (const entry of readdirSync(directory, { withFileTypes: true, recursive: true })) {
    if (entry.isFile()) {
      found.push(toPosix(relative(directory, join(entry.parentPath, entry.name))));
    }
  }
  return found.sort();
}

function isTestOnly(relativePath) {
  const name = relativePath.slice(relativePath.lastIndexOf(posix.sep) + 1);
  return (
    TEST_ONLY_SUFFIXES.some((suffix) => name.endsWith(suffix)) || TEST_ONLY_NAMES.includes(name)
  );
}

/** Returns the deterministic, sorted list of bundle input paths, relative to the Web UI root. */
export function collectInputInventory(webUiRoot) {
  const inputs = [];
  for (const name of CONFIGURATION_INPUTS) {
    if (statSync(join(webUiRoot, name), { throwIfNoEntry: false })?.isFile() !== true) {
      throw new Error(`Web UI bundle input is missing: ${name}`);
    }
    inputs.push(name);
  }
  for (const name of listFiles(join(webUiRoot, SOURCE_DIRECTORY))) {
    if (!isTestOnly(name)) {
      inputs.push(`${SOURCE_DIRECTORY}${posix.sep}${name}`);
    }
  }
  return inputs.sort();
}

function hashFile(path) {
  return createHash("sha256").update(readFileSync(path)).digest("hex");
}

function hashInto(baseDirectory, names) {
  const hashes = {};
  for (const name of [...names].sort()) {
    hashes[name] = hashFile(join(baseDirectory, ...name.split(posix.sep)));
  }
  return hashes;
}

/** Builds the manifest object describing the current bundle inputs and generated assets. */
export function buildManifest(webUiRoot, distDirectory) {
  return {
    format_version: MANIFEST_FORMAT_VERSION,
    inputs: hashInto(webUiRoot, collectInputInventory(webUiRoot)),
    assets: hashInto(distDirectory, [...EXPECTED_ASSETS.keys()]),
  };
}

export function manifestPath(distDirectory) {
  return join(distDirectory, MANIFEST_FILE_NAME);
}

/** Writes the manifest. Called only immediately after a clean production build. */
export function writeManifest(webUiRoot, distDirectory) {
  const manifest = buildManifest(webUiRoot, distDirectory);
  writeFileSync(manifestPath(distDirectory), `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

function compareHashes(label, recorded, actual, failures) {
  for (const name of Object.keys(actual)) {
    if (!Object.hasOwn(recorded, name)) {
      failures.push(`Build content manifest has no recorded ${label}: ${name}`);
    } else if (recorded[name] !== actual[name]) {
      failures.push(
        `Build content manifest ${label} hash does not match the current file: ${name}`,
      );
    }
  }
  for (const name of Object.keys(recorded)) {
    if (!Object.hasOwn(actual, name)) {
      failures.push(`Build content manifest records a ${label} that no longer exists: ${name}`);
    }
  }
}

/**
 * Re-verifies the recorded manifest against the current inputs and outputs.
 *
 * This never writes, repairs, or refreshes the manifest: a stale build stays
 * failed until it is rebuilt.
 */
export function checkManifest(webUiRoot, distDirectory) {
  const failures = [];
  let recorded;
  try {
    recorded = JSON.parse(readFileSync(manifestPath(distDirectory), "utf8"));
  } catch (error) {
    return [`Build content manifest is missing or unreadable: ${error.message}`];
  }
  if (recorded === null || typeof recorded !== "object" || Array.isArray(recorded)) {
    return ["Build content manifest is not a JSON object."];
  }
  if (recorded.format_version !== MANIFEST_FORMAT_VERSION) {
    return [
      `Build content manifest format version is not recognized: ${JSON.stringify(recorded.format_version)} (expected ${MANIFEST_FORMAT_VERSION})`,
    ];
  }

  const current = buildManifest(webUiRoot, distDirectory);
  for (const section of ["inputs", "assets"]) {
    const value = recorded[section];
    if (value === null || typeof value !== "object" || Array.isArray(value)) {
      failures.push(`Build content manifest section is missing or not an object: ${section}`);
      continue;
    }
    compareHashes(
      section === "inputs" ? "bundle input" : "generated asset",
      value,
      current[section],
      failures,
    );
  }
  return failures;
}

function formatBytes(bytes) {
  return `${bytes.toString().padStart(7)} B (${(bytes / KIB).toFixed(2).padStart(7)} KiB)`;
}

/** Validates the generated inventory and raw byte budgets, ignoring build metadata. */
export function validateBundle(distDirectory, log = console.log) {
  const failures = [];
  let actual;
  try {
    actual = listFiles(distDirectory).filter((name) => name !== MANIFEST_FILE_NAME);
  } catch {
    return [`Build output directory is missing: ${distDirectory}`];
  }

  for (const unexpected of actual.filter((name) => !EXPECTED_ASSETS.has(name))) {
    failures.push(`Unexpected build output: ${unexpected}`);
  }
  for (const missing of [...EXPECTED_ASSETS.keys()].filter((name) => !actual.includes(name))) {
    failures.push(`Missing build output: ${missing}`);
  }

  let combinedRaw = 0;
  let combinedGzip = 0;

  log("Weavelit Web UI production bundle report");
  log(
    `  ${"asset".padEnd(24)} ${"raw".padStart(21)} ${"gzip".padStart(21)} ${"limit".padStart(21)}`,
  );

  for (const [name, limit] of EXPECTED_ASSETS) {
    if (!actual.includes(name)) {
      continue;
    }
    const contents = readFileSync(join(distDirectory, name));
    const raw = contents.byteLength;
    const gzip = gzipSync(contents, { level: 9 }).byteLength;
    combinedRaw += raw;
    combinedGzip += gzip;
    log(`  ${name.padEnd(24)} ${formatBytes(raw)} ${formatBytes(gzip)} ${formatBytes(limit)}`);
    if (raw > limit) {
      failures.push(`Build output exceeds its raw size limit: ${name} (${raw} B > ${limit} B)`);
    }
  }

  log(
    `  ${"combined".padEnd(24)} ${formatBytes(combinedRaw)} ${formatBytes(combinedGzip)} ${formatBytes(COMBINED_LIMIT)}`,
  );
  if (combinedRaw > COMBINED_LIMIT) {
    failures.push(
      `Combined build output exceeds its raw size limit: ${combinedRaw} B > ${COMBINED_LIMIT} B`,
    );
  }

  return failures;
}

export function run(
  argv,
  webUiRoot = webUiRootDefault,
  distDirectory = join(webUiRoot, "dist"),
  output = { log: console.log, error: console.error },
) {
  const mode = argv.find((argument) => argument === "--write" || argument === "--check");
  if (mode === undefined) {
    output.error("Usage: verify-build-output.mjs (--write | --check)");
    return 2;
  }

  const failures = validateBundle(distDirectory, output.log);
  if (failures.length === 0 && mode === "--check") {
    failures.push(...checkManifest(webUiRoot, distDirectory));
  }

  if (failures.length > 0) {
    output.error("\nProduction bundle validation failed:");
    for (const failure of failures) {
      output.error(`  - ${failure}`);
    }
    output.error("\nRebuild the Web UI before continuing:\n  make -C server check-web-ui");
    return 1;
  }

  if (mode === "--write") {
    writeManifest(webUiRoot, distDirectory);
    output.log(`\nWrote the build content manifest: ${MANIFEST_FILE_NAME}`);
  }
  output.log("\nProduction bundle validation passed.");
  return 0;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  process.exit(run(process.argv.slice(2)));
}
