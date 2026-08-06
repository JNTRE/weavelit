import { rmSync } from 'node:fs';

import { readFixtureState } from './fixture-support';

const TERMINATION_GRACE_MS = 2_000;
const TERMINATION_POLL_INTERVAL_MS = 50;

function isRunning(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

async function delay(milliseconds: number): Promise<void> {
  return new Promise((resolveDelay) => {
    setTimeout(resolveDelay, milliseconds);
  });
}

export default async function globalTeardown(): Promise<void> {
  let state;
  try {
    state = readFixtureState();
  } catch {
    return;
  }

  if (state.serverPid > 0 && isRunning(state.serverPid)) {
    try {
      process.kill(state.serverPid, 'SIGTERM');
    } catch {
      // The process already exited between the check and the signal.
    }
    const deadline = Date.now() + TERMINATION_GRACE_MS;
    while (Date.now() < deadline && isRunning(state.serverPid)) {
      await delay(TERMINATION_POLL_INTERVAL_MS);
    }
    if (isRunning(state.serverPid)) {
      try {
        process.kill(state.serverPid, 'SIGKILL');
      } catch {
        // The process exited while the grace period was elapsing.
      }
    }
  }

  rmSync(state.fixtureRoot, { recursive: true, force: true });
}
