import { accessSync, constants, rmSync } from "node:fs";
import { join } from "node:path";

import {
  createFixtureRoot,
  createStateRoot,
  generateTlsMaterial,
  releaseBinaryPath,
  reserveLoopbackPort,
  spawnServer,
  waitForServerReady,
  writeFixtureState,
  type ServerSpawnConfiguration,
} from "./fixture-support";

export default async function globalSetup(): Promise<void> {
  const binary = releaseBinaryPath();
  try {
    accessSync(binary, constants.X_OK);
  } catch {
    throw new Error(
      `The release Server binary is missing at ${binary}. ` +
        "Run `cargo build --locked --workspace --release` before the browser smoke test.",
    );
  }

  const fixtureRoot = createFixtureRoot();
  const stdoutPath = join(fixtureRoot, "server-stdout.log");
  const stderrPath = join(fixtureRoot, "server-stderr.log");

  try {
    const { certificatePath, privateKeyPath } = generateTlsMaterial(fixtureRoot);
    const stateRoot = createStateRoot(fixtureRoot);
    const port = await reserveLoopbackPort();
    const configuration: ServerSpawnConfiguration = {
      binary,
      listenerAddress: `127.0.0.1:${port}`,
      certificatePath,
      privateKeyPath,
      stateRoot,
      stdoutPath,
      stderrPath,
    };

    const server = spawnServer(configuration);
    try {
      await waitForServerReady(server, configuration);
    } catch (error) {
      server.kill("SIGKILL");
      throw error;
    }

    server.detach();
    writeFixtureState(fixtureRoot, {
      baseUrl: `https://${configuration.listenerAddress}`,
      serverPid: server.pid,
      fixtureRoot,
      stdoutPath,
      stderrPath,
    });
  } catch (error) {
    rmSync(fixtureRoot, { recursive: true, force: true });
    throw error;
  }
}
