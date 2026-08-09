import { rmSync } from "node:fs";
import { join } from "node:path";

import { expect, test, type Page } from "@playwright/test";

import {
  createFixtureRoot,
  createStateRoot,
  generateTlsMaterial,
  releaseBinaryPath,
  reserveLoopbackPort,
  spawnServer,
  terminateServer,
  waitForServerReady,
  type RunningServer,
  type ServerSpawnConfiguration,
} from "./fixture-support";

const SELECTED_MESSAGE = "An Application Database is selected for this deployment.";
const UNSELECTED_MESSAGE = "No Application Database is selected for this deployment.";
const SELECTION_ACTION_NAME = "Select SQLite";
const SELECTION_PATH = "/api/v1/application-database";

/**
 * Every response the page load and the selection are expected to produce.
 *
 * The listener admits a burst of 12 requests per source, so asserting the exact
 * set also pins the request budget: 5 requests before the restart and 4 after
 * it, each generation counted by its own limiter because a restart replaces the
 * process. Neither generation can drift toward a `429` unnoticed.
 */
const PAGE_LOAD_RESPONSES = [
  "200 /",
  "200 /assets/weavelit-application.js",
  "200 /assets/weavelit-application.css",
  "200 /api/v1/status",
] as const;

function observeResponses(page: Page): string[] {
  const observed: string[] = [];
  page.on("response", (response) => {
    observed.push(`${response.status()} ${new URL(response.url()).pathname}`);
  });
  return observed;
}

function sorted(entries: readonly string[]): string[] {
  return [...entries].sort();
}

test("a selected Application Database is presented immediately and survives a Server restart", async ({
  page,
}) => {
  // The scenario starts two Server generations and generates TLS material, so
  // it needs more than the default per-test budget.
  test.setTimeout(90_000);

  const fixtureRoot = createFixtureRoot("weavelit-browser-restart-");
  let server: RunningServer | null = null;

  try {
    const { certificatePath, privateKeyPath } = generateTlsMaterial(fixtureRoot);
    const stateRoot = createStateRoot(fixtureRoot);
    const port = await reserveLoopbackPort();
    // Recorded once and reused verbatim by the restart, so the second
    // generation runs against the identical state root and listener address.
    const configuration: ServerSpawnConfiguration = {
      binary: releaseBinaryPath(),
      listenerAddress: `127.0.0.1:${port}`,
      certificatePath,
      privateKeyPath,
      stateRoot,
      stdoutPath: join(fixtureRoot, "server-stdout.log"),
      stderrPath: join(fixtureRoot, "server-stderr.log"),
    };
    const baseUrl = `https://${configuration.listenerAddress}`;
    const observed = observeResponses(page);

    server = spawnServer(configuration);
    await waitForServerReady(server, configuration);

    const status = page.locator("p.shell__status");
    const selection = page.locator("section.shell__selection");

    // The operator opens a deployment whose state root has no database yet.
    const document = await page.goto(baseUrl, { waitUntil: "load" });
    expect(document?.status()).toBe(200);
    await expect(status).toHaveAttribute("data-status-state", "available");
    await expect(status).toHaveText(UNSELECTED_MESSAGE);
    await expect(selection).toHaveCount(1);

    // The selection is driven through the control the operator actually uses,
    // not through a request issued by the test.
    const selectionResponse = page.waitForResponse(
      (response) => new URL(response.url()).pathname === SELECTION_PATH,
    );
    await page.getByRole("button", { name: SELECTION_ACTION_NAME }).click();
    const selectionResult = await selectionResponse;
    expect(selectionResult.request().method()).toBe("PUT");
    expect(selectionResult.status()).toBe(200);
    expect(await selectionResult.json()).toEqual({
      lifecycle: "uninitialized",
      database_selected: true,
    });

    // The displayed status changes within the same Server process, and the
    // Web UI stops offering a selection that has already been made.
    await expect(status).toHaveAttribute("data-status-state", "available");
    await expect(status).toHaveText(SELECTED_MESSAGE);
    await expect(selection).toHaveCount(0);
    expect(sorted(observed), "requests issued against the first generation").toEqual(
      sorted([...PAGE_LOAD_RESPONSES, `200 ${SELECTION_PATH}`]),
    );

    // The Server is terminated, not shut down: the binary installs no signal
    // handler. The exit is awaited and asserted rather than assumed, because
    // only a real exit closes the listening socket and the state-root lock
    // descriptor, and the restart below depends on both being released.
    const exit = await terminateServer(server);
    server = null;
    expect(exit.signal, "the Server ended on SIGTERM").toBe("SIGTERM");
    expect(exit.code, "the Server did not exit under its own control").toBeNull();

    // Rebinding the same port and reacquiring the state-root lock is itself
    // evidence that the terminated process released both.
    server = spawnServer(configuration);
    await waitForServerReady(server, configuration);

    // The same origin is reloaded, so the Server still recognises it.
    const reloaded = await page.reload({ waitUntil: "load" });
    expect(reloaded?.status()).toBe(200);
    await expect(status).toHaveAttribute("data-status-state", "available");
    await expect(status).toHaveText(SELECTED_MESSAGE);
    await expect(selection).toHaveCount(0);

    expect(sorted(observed), "every request across both generations").toEqual(
      sorted([...PAGE_LOAD_RESPONSES, `200 ${SELECTION_PATH}`, ...PAGE_LOAD_RESPONSES]),
    );
  } finally {
    if (server !== null) {
      await terminateServer(server).catch(() => undefined);
    }
    rmSync(fixtureRoot, { recursive: true, force: true });
  }
});
