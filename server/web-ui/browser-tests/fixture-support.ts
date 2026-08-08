import { execFileSync, spawn } from "node:child_process";
import {
  chmodSync,
  createWriteStream,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { connect as connectTls } from "node:tls";
import { fileURLToPath } from "node:url";

const READY_TIMEOUT_MS = 20_000;
const READY_POLL_INTERVAL_MS = 100;
const TERMINATION_TIMEOUT_MS = 10_000;

/** Handoff record written by global setup and consumed by the specs and teardown. */
export interface FixtureState {
  readonly baseUrl: string;
  readonly serverPid: number;
  readonly fixtureRoot: string;
  readonly stdoutPath: string;
  readonly stderrPath: string;
}

/** Environment variable carrying the handoff record path into the worker processes. */
export const FIXTURE_STATE_ENV = "WEAVELIT_BROWSER_FIXTURE_STATE";

const webUiDirectory = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Absolute path of the release Server binary the smoke test exercises. */
export function releaseBinaryPath(): string {
  return (
    process.env["WEAVELIT_SERVER_BINARY"] ??
    resolve(webUiDirectory, "../target/release/weavelit-server")
  );
}

/** Creates the isolated fixture directory tree with owner-only permissions. */
export function createFixtureRoot(prefix = "weavelit-browser-smoke-"): string {
  const created = mkdtempSync(join(realpathSync(tmpdir()), prefix));
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
  const certificatePath = join(fixtureRoot, "certificate.pem");
  const privateKeyPath = join(fixtureRoot, "private-key.pem");

  execFileSync(
    "openssl",
    [
      "req",
      "-x509",
      "-newkey",
      "rsa:2048",
      "-sha256",
      "-days",
      "1",
      "-noenc",
      "-subj",
      "/CN=localhost",
      "-addext",
      "subjectAltName=DNS:localhost,IP:127.0.0.1",
      "-keyout",
      privateKeyPath,
      "-out",
      certificatePath,
    ],
    { stdio: ["ignore", "ignore", "pipe"] },
  );

  chmodSync(privateKeyPath, 0o600);
  chmodSync(certificatePath, 0o644);
  return { certificatePath, privateKeyPath };
}

/** Creates the isolated, empty state root the Server classifies at startup. */
export function createStateRoot(fixtureRoot: string): string {
  const stateRoot = join(fixtureRoot, "state-root");
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
  return JSON.parse(readFileSync(path, "utf8")) as FixtureState;
}

/** Writes the handoff record and publishes its path to the worker processes. */
export function writeFixtureState(fixtureRoot: string, state: FixtureState): string {
  const path = join(fixtureRoot, "fixture-state.json");
  writeFileSync(path, JSON.stringify(state), { mode: 0o600 });
  process.env[FIXTURE_STATE_ENV] = path;
  return path;
}

/** Resolves after the requested delay. */
export async function delay(milliseconds: number): Promise<void> {
  return new Promise((resolveDelay) => {
    setTimeout(resolveDelay, milliseconds);
  });
}

/** Reserves a currently free loopback port and releases it for the Server to bind. */
export async function reserveLoopbackPort(): Promise<number> {
  return new Promise<number>((resolvePort, rejectPort) => {
    const probe = createServer();
    probe.once("error", rejectPort);
    probe.listen(0, "127.0.0.1", () => {
      const address = probe.address();
      if (address === null || typeof address === "string") {
        probe.close(() => rejectPort(new Error("could not reserve a loopback port")));
        return;
      }
      const { port } = address;
      probe.close(() => resolvePort(port));
    });
  });
}

/**
 * Every input that determines how a Server process is started.
 *
 * A restart reuses this record verbatim, so the second generation runs against
 * the identical state root, listener address, and TLS material.
 */
export interface ServerSpawnConfiguration {
  readonly binary: string;
  readonly listenerAddress: string;
  readonly certificatePath: string;
  readonly privateKeyPath: string;
  readonly stateRoot: string;
  readonly stdoutPath: string;
  readonly stderrPath: string;
}

/** How a Server process ended. */
export interface ServerExit {
  readonly code: number | null;
  readonly signal: NodeJS.Signals | null;
}

/** A spawned Server process and the controls the browser fixtures need over it. */
export interface RunningServer {
  readonly pid: number;
  /** Settles once the process has actually ended. */
  readonly exited: Promise<ServerExit>;
  /** The recorded exit, or `null` while the process is still running. */
  observedExit(): ServerExit | null;
  kill(signal: NodeJS.Signals): void;
  /** Stops this Node process from staying alive for the child. */
  detach(): void;
}

/** Starts the release Server binary with exactly the recorded configuration. */
export function spawnServer(configuration: ServerSpawnConfiguration): RunningServer {
  const child = spawn(configuration.binary, [], {
    env: {
      ...process.env,
      WEAVELIT_HTTPS_LISTENER_ADDRESS: configuration.listenerAddress,
      WEAVELIT_TLS_CERTIFICATE_PATH: configuration.certificatePath,
      WEAVELIT_TLS_PRIVATE_KEY_PATH: configuration.privateKeyPath,
      WEAVELIT_STATE_ROOT: configuration.stateRoot,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  // Appending keeps every generation's output across a restart.
  child.stdout.pipe(createWriteStream(configuration.stdoutPath, { flags: "a" }));
  child.stderr.pipe(createWriteStream(configuration.stderrPath, { flags: "a" }));

  let exit: ServerExit | null = null;
  const exited = new Promise<ServerExit>((resolveExit) => {
    child.once("exit", (code, signal) => {
      const observed = { code, signal };
      exit = observed;
      resolveExit(observed);
    });
  });

  return {
    pid: child.pid ?? 0,
    exited,
    observedExit: () => exit,
    kill: (signal) => {
      try {
        child.kill(signal);
      } catch {
        // The process already exited.
      }
    },
    detach: () => child.unref(),
  };
}

function parseListenerAddress(listenerAddress: string): { host: string; port: number } {
  const separator = listenerAddress.lastIndexOf(":");
  return {
    host: listenerAddress.slice(0, separator),
    port: Number(listenerAddress.slice(separator + 1)),
  };
}

/**
 * Reports whether the listener completes a TLS handshake.
 *
 * The fixture certificate is self-signed, so the probe deliberately does not
 * require a trust chain; it only proves the Server is accepting TLS. It sends
 * no request head, so it consumes none of the listener's request-rate budget.
 */
async function acceptsTlsConnections(listenerAddress: string): Promise<boolean> {
  const { host, port } = parseListenerAddress(listenerAddress);
  return new Promise<boolean>((resolveProbe) => {
    const socket = connectTls({ host, port, rejectUnauthorized: false });
    const settle = (accepted: boolean): void => {
      socket.destroy();
      resolveProbe(accepted);
    };
    socket.once("secureConnect", () => settle(true));
    socket.once("error", () => settle(false));
  });
}

/** Renders the Server's captured output for a fixture failure message. */
export function serverDiagnostics(configuration: ServerSpawnConfiguration): string {
  const read = (path: string): string => {
    try {
      return readFileSync(path, "utf8").trim();
    } catch {
      return "<unreadable>";
    }
  };
  return (
    `\n  Server stderr: ${read(configuration.stderrPath) || "<empty>"}` +
    `\n  Server stdout: ${read(configuration.stdoutPath) || "<empty>"}`
  );
}

/**
 * Waits until the Server accepts TLS on its configured listener address.
 *
 * Polling the real listener is what makes a restart deterministic: the caller
 * never assumes readiness, and an early exit fails immediately with the
 * Server's own diagnostics instead of timing out.
 */
export async function waitForServerReady(
  server: RunningServer,
  configuration: ServerSpawnConfiguration,
): Promise<void> {
  const deadline = Date.now() + READY_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const exit = server.observedExit();
    if (exit !== null) {
      throw new Error(
        `The release Server exited before accepting TLS on ${configuration.listenerAddress}: ` +
          `code ${String(exit.code)}, signal ${String(exit.signal)}.${serverDiagnostics(configuration)}`,
      );
    }
    if (await acceptsTlsConnections(configuration.listenerAddress)) {
      return;
    }
    await delay(READY_POLL_INTERVAL_MS);
  }
  throw new Error(
    `The release Server did not accept TLS on ${configuration.listenerAddress} ` +
      `within ${READY_TIMEOUT_MS} ms.${serverDiagnostics(configuration)}`,
  );
}

/**
 * Sends `SIGTERM` and waits for the process to actually end.
 *
 * The Server installs no signal handler, so this terminates the process rather
 * than shutting the application down in an orderly way. Termination is what
 * frees the listener port and the state-root lock, because the kernel closes
 * the descriptors the process held.
 */
export async function terminateServer(server: RunningServer): Promise<ServerExit> {
  const already = server.observedExit();
  if (already !== null) {
    return already;
  }
  server.kill("SIGTERM");
  let timer: NodeJS.Timeout | undefined;
  const expiry = new Promise<never>((_resolveNever, rejectExpiry) => {
    timer = setTimeout(() => {
      rejectExpiry(
        new Error(`the Server did not exit within ${TERMINATION_TIMEOUT_MS} ms of SIGTERM`),
      );
    }, TERMINATION_TIMEOUT_MS);
  });
  try {
    return await Promise.race([server.exited, expiry]);
  } finally {
    clearTimeout(timer);
  }
}
