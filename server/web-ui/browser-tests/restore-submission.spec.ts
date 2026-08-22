import { readFileSync, rmSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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
const INITIALIZED_MESSAGE = "This deployment is initialized and now runs in normal operation.";
const SELECTION_ACTION_NAME = "Select SQLite";
const RESTORE_CHOICE_NAME = "Restore from a backup";
const RESTORE_ACTION_NAME = "Restore backup";

const SELECTION_PATH = "/api/v1/application-database";
const RESTORE_KEY_PATH = "/api/v1/restore";
const RESTORE_ARTIFACT_PATH = "/api/v1/restore/artifact";
const SESSION_PATH = "/api/v1/auth/session";
const RESTORE_TICKET_HEADER = "x-weavelit-restore-ticket";

const ARTIFACT_INPUT = "#weavelit-restore-artifact";
const RECOVERY_KEY_INPUT = "#weavelit-restore-recovery-key";

/**
 * The Server-owned Restore fixtures, used exactly as committed.
 *
 * The browser test consumes the same immutable artifact and recovery key the
 * Rust validation tests do, so a Restore that succeeds here is a Restore of the
 * canonical fixture rather than of anything this test generated. The artifact
 * is the one whose referenced components are exactly what the release binary
 * compiles in: the Web UI Client Module and the SQLite Log Module. The Server
 * refuses a backup naming anything else, which the Rust tests prove separately.
 */
const fixturesDirectory = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../crates/core/weavelit-server-restore/tests/fixtures",
);
const VALID_ARTIFACT_PATH = join(fixturesDirectory, "valid-web-ui-sqlite.wlitbackup");

function committedKey(name: string): string {
  return readFileSync(join(fixturesDirectory, name), "utf8").trim();
}

function capturedOutput(path: string): string {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return "";
  }
}

/**
 * Every response the browser is expected to produce, in no particular order.
 *
 * The listener admits a burst of 12 requests per source, so asserting the exact
 * set also pins the request budget: 4 for the page load, 1 selection, 2 for the
 * rejected Restore attempt, 2 for the accepted one, and 1 session probe the
 * sign-in control issues once the Restore withdraws the setup surface. The two
 * route-absence probes below are issued outside the page and add 2 more,
 * leaving the run inside the burst without relying on replenishment.
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

function observeRequestUrls(page: Page): string[] {
  const observed: string[] = [];
  page.on("request", (request) => {
    observed.push(request.url());
  });
  return observed;
}

function sorted(entries: readonly string[]): string[] {
  return [...entries].sort();
}

test("a submitted backup and recovery key restore the deployment through the Web UI", async ({
  page,
}) => {
  // The scenario generates TLS material, starts a Server, and runs a complete
  // Restore, so it needs more than the default per-test budget.
  test.setTimeout(120_000);

  const recoveryKey = committedKey("valid-identity.txt");
  const malformedRecoveryKey = committedKey("malformed-key.txt");
  const fixtureRoot = createFixtureRoot("weavelit-browser-restore-");
  let server: RunningServer | null = null;

  try {
    const { certificatePath, privateKeyPath } = generateTlsMaterial(fixtureRoot);
    const stateRoot = createStateRoot(fixtureRoot);
    const port = await reserveLoopbackPort();
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
    const requestedUrls = observeRequestUrls(page);

    server = spawnServer(configuration);
    await waitForServerReady(server, configuration);
    const servingPid = server.pid;

    const status = page.locator("p.shell__status");
    const restore = page.locator("section.shell__restore");
    const login = page.locator("section[data-authentication-state]");

    // The operator opens a deployment whose state root has no database yet.
    const document = await page.goto(baseUrl, { waitUntil: "load" });
    expect(document?.status()).toBe(200);
    await expect(status).toHaveText(UNSELECTED_MESSAGE);
    // Restore is not offered before a database has been selected.
    await expect(restore).toHaveCount(0);

    // 1. Take the Restore path of the mutually exclusive first-launch choice,
    // then select the SQLite Application Database through the operator's
    // control. Both paths share the one selection surface.
    await page.getByRole("button", { name: RESTORE_CHOICE_NAME }).click();
    await expect(page.locator("section.shell__choice")).toHaveCount(0);
    const selectionResponse = page.waitForResponse(
      (response) => new URL(response.url()).pathname === SELECTION_PATH,
    );
    await page.getByRole("button", { name: SELECTION_ACTION_NAME }).click();
    expect((await selectionResponse).status()).toBe(200);
    await expect(status).toHaveText(SELECTED_MESSAGE);

    // Selecting a database is what makes Restore eligible, and the form appears
    // in the same Server process without a reload.
    await expect(restore).toHaveCount(1);
    await expect(restore).toHaveAttribute("data-restore-state", "idle");

    // 2. A deliberately invalid attempt is rejected and rendered redacted.
    await page.locator(ARTIFACT_INPUT).setInputFiles(VALID_ARTIFACT_PATH);
    await page.locator(RECOVERY_KEY_INPUT).fill(malformedRecoveryKey);
    const rejectedKey = page.waitForResponse(
      (response) => new URL(response.url()).pathname === RESTORE_KEY_PATH,
    );
    const rejectedUpload = page.waitForResponse(
      (response) => new URL(response.url()).pathname === RESTORE_ARTIFACT_PATH,
    );
    await page.getByRole("button", { name: RESTORE_ACTION_NAME }).click();
    const rejectedTicket = await rejectedKey;
    expect(rejectedTicket.status()).toBe(202);
    expect(rejectedTicket.headers()["cache-control"]).toBe("no-store");
    const rejection = await rejectedUpload;
    expect(rejection.status()).toBe(400);
    expect(rejection.headers()["cache-control"]).toBeUndefined();

    await expect(restore).toHaveAttribute("data-restore-state", "failed");
    const failure = page.locator("p.shell__restore-failure");
    // Only the Server's stable code is rendered: no message, path, or detail.
    await expect(failure).toHaveText("recovery_key_invalid");
    // The key is cleared by the attempt, whether or not that attempt succeeded.
    await expect(page.locator(RECOVERY_KEY_INPUT)).toHaveValue("");

    // 3. A valid two-request Restore of the committed fixture.
    await page.locator(RECOVERY_KEY_INPUT).fill(recoveryKey);
    const issuedTicket = page.waitForResponse(
      (response) => new URL(response.url()).pathname === RESTORE_KEY_PATH,
    );
    const completedUpload = page.waitForResponse(
      (response) => new URL(response.url()).pathname === RESTORE_ARTIFACT_PATH,
    );
    await page.getByRole("button", { name: RESTORE_ACTION_NAME }).click();

    const ticketResponse = await issuedTicket;
    expect(ticketResponse.request().method()).toBe("PUT");
    expect(ticketResponse.status()).toBe(202);
    expect(ticketResponse.headers()["content-type"]).toBe("application/json; charset=utf-8");
    expect(ticketResponse.headers()["cache-control"]).toBe("no-store");

    const completion = await completedUpload;
    expect(completion.request().method()).toBe("PUT");
    // The Restore surface is withdrawn the moment the Restore completes, so the
    // rendered state is only read when the status is actually wrong.
    if (completion.status() !== 200) {
      throw new Error(
        `the upload returned ${completion.status()}, rendered Restore state: ${await restore.innerText()}`,
      );
    }
    expect(completion.headers()["content-type"]).toBe("application/json; charset=utf-8");
    expect(completion.headers()["cache-control"]).toBeUndefined();

    // The artifact is uploaded as the request body with the ticket in its one
    // permitted header, and with the exact media type the route accepts. The
    // ticket value is read from that header rather than from the response that
    // issued it, so the assertions below check the value actually presented.
    const uploadHeaders = await completion.request().allHeaders();
    const ticket = uploadHeaders[RESTORE_TICKET_HEADER] ?? "";
    expect(ticket, "the issued ticket is a canonical opaque token").toMatch(
      /^[A-Za-z0-9_-]{1,48}$/,
    );
    expect(uploadHeaders["content-type"]).toBe("application/octet-stream");
    expect(
      Number(uploadHeaders["content-length"]),
      "the committed artifact is uploaded whole",
    ).toBe(statSync(VALID_ARTIFACT_PATH).size);

    // 4. Normal operation activates in the same Server process: no restart, no
    // second generation, and the process that served the page is still serving.
    // The already-loaded page adopts the completion it holds, so every setup
    // control is withdrawn and the sign-in control is offered without a reload
    // and without re-reading a status projection that is no longer served.
    const loadedUrl = page.url();
    await expect(status).toHaveAttribute("data-status-state", "initialized");
    await expect(status).toHaveText(INITIALIZED_MESSAGE);
    await expect(restore).toHaveCount(0);
    await expect(page.locator("section.shell__choice")).toHaveCount(0);
    await expect(page.locator("section.shell__selection")).toHaveCount(0);
    await expect(login).toHaveAttribute("data-authentication-state", "unauthenticated");
    expect(page.url(), "the page was never reloaded or navigated").toBe(loadedUrl);
    expect(server.observedExit(), "the Server did not exit during the Restore").toBeNull();
    expect(server.pid, "the Restore ran in the process that served the page").toBe(servingPid);

    expect(sorted(observed), "requests the browser issued").toEqual(
      sorted([
        ...PAGE_LOAD_RESPONSES,
        `200 ${SELECTION_PATH}`,
        `202 ${RESTORE_KEY_PATH}`,
        `400 ${RESTORE_ARTIFACT_PATH}`,
        `202 ${RESTORE_KEY_PATH}`,
        `200 ${RESTORE_ARTIFACT_PATH}`,
        `401 ${SESSION_PATH}`,
      ]),
    );

    // 5. Both Restore routes are absent from the sealed deployment's surface.
    // Each probe carries the preconditions a mounted route would have required,
    // so a `404` is route absence rather than a rejected precondition.
    for (const path of [RESTORE_KEY_PATH, RESTORE_ARTIFACT_PATH]) {
      const absent = await page.request.fetch(`${baseUrl}${path}`, {
        method: "PUT",
        headers: {
          Accept: "application/json",
          "Content-Type": "application/json",
          "X-Weavelit-CSRF": "1",
          Origin: baseUrl,
        },
        data: "{}",
      });
      expect(absent.status(), `${path} after the Restore`).toBe(404);
      expect(await absent.json()).toEqual({ error: "not_found" });
    }
    expect(server.observedExit(), "the Server is still the same process").toBeNull();

    // 6. Neither the recovery key nor the ticket reached a URL, a cookie,
    // browser storage, the rendered page, or the Server's own output.
    const secrets = [recoveryKey, malformedRecoveryKey, ticket];

    for (const url of [page.url(), ...requestedUrls]) {
      for (const secret of secrets) {
        expect(url, "no request URL carries recovery material").not.toContain(secret);
      }
    }

    expect(await page.context().cookies(), "the surface sets no cookie").toEqual([]);
    const browserState = await page.evaluate(() => ({
      cookie: globalThis.document.cookie,
      localStorage: JSON.stringify(globalThis.localStorage),
      sessionStorage: JSON.stringify(globalThis.sessionStorage),
    }));
    expect(browserState.cookie).toBe("");
    expect(browserState.localStorage).toBe("{}");
    expect(browserState.sessionStorage).toBe("{}");

    const rendered = await page.content();
    const serverOutput =
      capturedOutput(configuration.stdoutPath) + capturedOutput(configuration.stderrPath);
    for (const secret of secrets) {
      expect(rendered, "the page renders no recovery material").not.toContain(secret);
      expect(serverOutput, "the Server logs no recovery material").not.toContain(secret);
    }
  } finally {
    if (server !== null) {
      await terminateServer(server).catch(() => undefined);
    }
    rmSync(fixtureRoot, { recursive: true, force: true });
  }
});
