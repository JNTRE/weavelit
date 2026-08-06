#!/usr/bin/env node
// Validates the production build inventory and raw byte budgets.
//
// The Web UI Client Module embeds a fixed compile-time asset allowlist, so a
// missing, renamed, extra, or oversized output is a build failure. Budgets are
// enforced against raw bytes because the Server applies no compression; gzip
// sizes are reported for information only.

import { gzipSync } from 'node:zlib';
import { readdirSync, readFileSync } from 'node:fs';
import { join, posix, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const KIB = 1024;

const EXPECTED_ASSETS = new Map([
  ['index.html', 16 * KIB],
  ['assets/application.js', 256 * KIB],
  ['assets/application.css', 64 * KIB],
]);

const COMBINED_LIMIT = 336 * KIB;

const distDirectory = fileURLToPath(new URL('../dist', import.meta.url));

function listFiles(directory) {
  const found = [];
  for (const entry of readdirSync(directory, { withFileTypes: true, recursive: true })) {
    if (entry.isFile()) {
      const absolute = join(entry.parentPath, entry.name);
      found.push(relative(directory, absolute).split(sep).join(posix.sep));
    }
  }
  return found.sort();
}

function formatBytes(bytes) {
  return `${bytes.toString().padStart(7)} B (${(bytes / KIB).toFixed(2).padStart(7)} KiB)`;
}

let actual;
try {
  actual = listFiles(distDirectory);
} catch {
  console.error(`Build output directory is missing: ${distDirectory}`);
  process.exit(1);
}

const failures = [];

for (const unexpected of actual.filter((name) => !EXPECTED_ASSETS.has(name))) {
  failures.push(`Unexpected build output: ${unexpected}`);
}
for (const missing of [...EXPECTED_ASSETS.keys()].filter((name) => !actual.includes(name))) {
  failures.push(`Missing build output: ${missing}`);
}

let combinedRaw = 0;
let combinedGzip = 0;

console.log('Weavelit Web UI production bundle report');
console.log(`  ${'asset'.padEnd(24)} ${'raw'.padStart(21)} ${'gzip'.padStart(21)} ${'limit'.padStart(21)}`);

for (const [name, limit] of EXPECTED_ASSETS) {
  if (!actual.includes(name)) {
    continue;
  }
  const contents = readFileSync(join(distDirectory, name));
  const raw = contents.byteLength;
  const gzip = gzipSync(contents, { level: 9 }).byteLength;
  combinedRaw += raw;
  combinedGzip += gzip;
  console.log(`  ${name.padEnd(24)} ${formatBytes(raw)} ${formatBytes(gzip)} ${formatBytes(limit)}`);
  if (raw > limit) {
    failures.push(`Build output exceeds its raw size limit: ${name} (${raw} B > ${limit} B)`);
  }
}

console.log(
  `  ${'combined'.padEnd(24)} ${formatBytes(combinedRaw)} ${formatBytes(combinedGzip)} ${formatBytes(COMBINED_LIMIT)}`,
);
if (combinedRaw > COMBINED_LIMIT) {
  failures.push(`Combined build output exceeds its raw size limit: ${combinedRaw} B > ${COMBINED_LIMIT} B`);
}

if (failures.length > 0) {
  console.error('\nProduction bundle validation failed:');
  for (const failure of failures) {
    console.error(`  - ${failure}`);
  }
  process.exit(1);
}

console.log('\nProduction bundle validation passed.');
