import { execFileSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Handoff record written by global setup and consumed by the specs and teardown. */
export interface FixtureState {
  readonly baseUrl: string;
  readonly serverPid: number;
  readonly fixtureRoot: string;
  readonly stdoutPath: string;
  readonly stderrPath: string;
}

/** Environment variable carrying the handoff record path into the worker processes. */
export const FIXTURE_STATE_ENV = 'WEAVELIT_BROWSER_FIXTURE_STATE';

const webUiDirectory = resolve(dirname(fileURLToPath(import.meta.url)), '..');

/** Absolute path of the release Server binary the smoke test exercises. */
export function releaseBinaryPath(): string {
  return process.env['WEAVELIT_SERVER_BINARY'] ?? resolve(webUiDirectory, '../target/release/weavelit-server');
}

/** Creates the isolated fixture directory tree with owner-only permissions. */
export function createFixtureRoot(): string {
  const created = mkdtempSync(join(realpathSync(tmpdir()), 'weavelit-browser-smoke-'));
  // The Server rejects TLS material reached through a symlinked path component.
  const fixtureRoot = realpathSync(created);
  chmodSync(fixtureRoot, 0o700);
  return fixtureRoot;
}

/**
 * Generates a short-lived self-signed certificate covering the loopback host.
 *
 * The private key is written with mode `0600` and the certificate with mode
 * `0644` because the Server refuses group- or world-writable TLS material and
 * refuses any private key readable beyond its owner.
 */
export function generateTlsMaterial(fixtureRoot: string): {
  certificatePath: string;
  privateKeyPath: string;
} {
  const certificatePath = join(fixtureRoot, 'certificate.pem');
  const privateKeyPath = join(fixtureRoot, 'private-key.pem');

  execFileSync(
    'openssl',
    [
      'req',
      '-x509',
      '-newkey',
      'rsa:2048',
      '-sha256',
      '-days',
      '1',
      '-noenc',
      '-subj',
      '/CN=localhost',
      '-addext',
      'subjectAltName=DNS:localhost,IP:127.0.0.1',
      '-keyout',
      privateKeyPath,
      '-out',
      certificatePath,
    ],
    { stdio: ['ignore', 'ignore', 'pipe'] },
  );

  chmodSync(privateKeyPath, 0o600);
  chmodSync(certificatePath, 0o644);
  return { certificatePath, privateKeyPath };
}

/** Creates the isolated, empty state root the Server classifies at startup. */
export function createStateRoot(fixtureRoot: string): string {
  const stateRoot = join(fixtureRoot, 'state-root');
  mkdirSync(stateRoot, { mode: 0o700 });
  chmodSync(stateRoot, 0o700);
  return stateRoot;
}

/** Reads the handoff record written by global setup. */
export function readFixtureState(): FixtureState {
  const path = process.env[FIXTURE_STATE_ENV];
  if (path === undefined) {
    throw new Error(`${FIXTURE_STATE_ENV} is not set; the browser fixture did not start`);
  }
  return JSON.parse(readFileSync(path, 'utf8')) as FixtureState;
}

/** Writes the handoff record and publishes its path to the worker processes. */
export function writeFixtureState(fixtureRoot: string, state: FixtureState): string {
  const path = join(fixtureRoot, 'fixture-state.json');
  writeFileSync(path, JSON.stringify(state), { mode: 0o600 });
  process.env[FIXTURE_STATE_ENV] = path;
  return path;
}
