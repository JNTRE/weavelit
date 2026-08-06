import { spawn } from 'node:child_process';
import { accessSync, constants, createWriteStream, readFileSync, rmSync } from 'node:fs';
import { connect, createServer } from 'node:net';
import { join } from 'node:path';

import {
  createFixtureRoot,
  createStateRoot,
  generateTlsMaterial,
  releaseBinaryPath,
  writeFixtureState,
} from './fixture-support';

const READY_TIMEOUT_MS = 20_000;
const READY_POLL_INTERVAL_MS = 100;

async function reserveLoopbackPort(): Promise<number> {
  return new Promise<number>((resolvePort, rejectPort) => {
    const probe = createServer();
    probe.once('error', rejectPort);
    probe.listen(0, '127.0.0.1', () => {
      const address = probe.address();
      if (address === null || typeof address === 'string') {
        probe.close(() => rejectPort(new Error('could not reserve a loopback port')));
        return;
      }
      const { port } = address;
      probe.close(() => resolvePort(port));
    });
  });
}

async function canConnect(port: number): Promise<boolean> {
  return new Promise<boolean>((resolveConnect) => {
    const socket = connect({ host: '127.0.0.1', port });
    const settle = (accepted: boolean): void => {
      socket.destroy();
      resolveConnect(accepted);
    };
    socket.once('connect', () => settle(true));
    socket.once('error', () => settle(false));
  });
}

async function delay(milliseconds: number): Promise<void> {
  return new Promise((resolveDelay) => {
    setTimeout(resolveDelay, milliseconds);
  });
}

function diagnostics(stdoutPath: string, stderrPath: string): string {
  const read = (path: string): string => {
    try {
      return readFileSync(path, 'utf8').trim();
    } catch {
      return '<unreadable>';
    }
  };
  return `\n  Server stderr: ${read(stderrPath) || '<empty>'}\n  Server stdout: ${read(stdoutPath) || '<empty>'}`;
}

export default async function globalSetup(): Promise<void> {
  const binary = releaseBinaryPath();
  try {
    accessSync(binary, constants.X_OK);
  } catch {
    throw new Error(
      `The release Server binary is missing at ${binary}. ` +
        'Run `cargo build --locked --workspace --release` before the browser smoke test.',
    );
  }

  const fixtureRoot = createFixtureRoot();
  const stdoutPath = join(fixtureRoot, 'server-stdout.log');
  const stderrPath = join(fixtureRoot, 'server-stderr.log');

  try {
    const { certificatePath, privateKeyPath } = generateTlsMaterial(fixtureRoot);
    const stateRoot = createStateRoot(fixtureRoot);
    const port = await reserveLoopbackPort();

    const stdout = createWriteStream(stdoutPath);
    const stderr = createWriteStream(stderrPath);
    const server = spawn(binary, [], {
      env: {
        ...process.env,
        WEAVELIT_HTTPS_LISTENER_ADDRESS: `127.0.0.1:${port}`,
        WEAVELIT_TLS_CERTIFICATE_PATH: certificatePath,
        WEAVELIT_TLS_PRIVATE_KEY_PATH: privateKeyPath,
        WEAVELIT_STATE_ROOT: stateRoot,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    server.stdout.pipe(stdout);
    server.stderr.pipe(stderr);

    const observed: { exit: { code: number | null; signal: NodeJS.Signals | null } | null } = {
      exit: null,
    };
    server.once('exit', (code, signal) => {
      observed.exit = { code, signal };
    });

    const deadline = Date.now() + READY_TIMEOUT_MS;
    let ready = false;
    while (Date.now() < deadline) {
      if (observed.exit !== null) {
        break;
      }
      if (await canConnect(port)) {
        ready = true;
        break;
      }
      await delay(READY_POLL_INTERVAL_MS);
    }

    if (!ready) {
      server.kill('SIGKILL');
      await delay(READY_POLL_INTERVAL_MS);
      const reason =
        observed.exit === null
          ? `it did not accept a connection on 127.0.0.1:${port} within ${READY_TIMEOUT_MS} ms`
          : `it exited early with code ${String(observed.exit.code)} and signal ${String(observed.exit.signal)}`;
      throw new Error(`The release Server did not become ready: ${reason}.${diagnostics(stdoutPath, stderrPath)}`);
    }

    server.unref();
    writeFixtureState(fixtureRoot, {
      baseUrl: `https://127.0.0.1:${port}`,
      serverPid: server.pid ?? 0,
      fixtureRoot,
      stdoutPath,
      stderrPath,
    });
  } catch (error) {
    rmSync(fixtureRoot, { recursive: true, force: true });
    throw error;
  }
}
