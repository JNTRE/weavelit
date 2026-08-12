import { readFileSync, rmSync } from "node:fs";
import { join } from "node:path";

import { expect, test, type Page, type Request } from "@playwright/test";

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

const INIT_CHOICE_NAME = "Set up a new deployment";
const SELECTION_ACTION_NAME = "Select SQLite";
const PREPARE_ACTION_NAME = "Prepare recovery key";
const COPY_ACTION_NAME = "Copy recovery key";
const KEY_CONTINUE_NAME = "Continue to review";
const FINALIZE_ACTION_NAME = "Complete setup";
const LOGIN_ACTION_NAME = "Sign in";

const UNSELECTED_MESSAGE = "No Application Database is selected for this deployment.";
const SELECTED_MESSAGE = "An Application Database is selected for this deployment.";
const INITIALIZED_MESSAGE = "This deployment is initialized and now runs in normal operation.";
const KEY_ONCE_WARNING =
  "This is the only time this recovery key is ever shown. Weavelit cannot display it again, cannot recover it, and cannot issue a replacement. Save it outside Weavelit now, before you continue.";
const COPIED_MESSAGE = "The recovery key was copied to the clipboard.";
const AUTHENTICATED_MESSAGE = "You are signed in.";

const STATUS_PATH = "/api/v1/status";
const SELECTION_PATH = "/api/v1/application-database";
const RECOVERY_KEY_PATH = "/api/v1/init/recovery-key";
const INIT_PATH = "/api/v1/init";
const SESSION_PATH = "/api/v1/auth/session";
const LOGIN_PATH = "/api/v1/auth/login";

const SYSTEM_LOG_SELECT = "#weavelit-init-system-log";
const AUDIT_LOG_SELECT = "#weavelit-init-audit-log";
const USERNAME_INPUT = "#weavelit-init-username";
const DISPLAY_NAME_INPUT = "#weavelit-init-display-name";
const PASSWORD_INPUT = "#weavelit-init-password";
const RECOVERY_KEY_OUTPUT = "#weavelit-init-recovery-key";
const ACKNOWLEDGEMENT_INPUT = "#weavelit-init-acknowledgement";
const LOGIN_USERNAME_INPUT = "#weavelit-login-username";
const LOGIN_PASSWORD_INPUT = "#weavelit-login-password";

/** The identity this test creates through the workflow and then signs in as. */
const ADMINISTRATOR_USERNAME = "administrator";
const ADMINISTRATOR_DISPLAY_NAME = "First Administrator";
const ADMINISTRATOR_PASSWORD = "browser-fixture-administrator-password";

/** The canonical shapes the delivered values are required to have. */
const RECOVERY_KEY_SHAPE = /^AGE-SECRET-KEY-1[0-9A-Z]+$/;
const OPAQUE_TOKEN = /^[A-Za-z0-9_-]{1,48}$/;
const PROOF_SHAPE = /^[A-Za-z0-9_-]{43}$/;

function capturedOutput(path: string): string {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return "";
  }
}

function observeResponses(page: Page): string[] {
  const observed: string[] = [];
  page.on("response", (response) => {
    observed.push(`${response.status()} ${new URL(response.url()).pathname}`);
  });
  return observed;
}

function observeRequests(page: Page): Request[] {
  const observed: Request[] = [];
  page.on("request", (request) => {
    observed.push(request);
  });
  return observed;
}

function sorted(entries: readonly string[]): string[] {
  return [...entries].sort();
}

/**
 * Every response the one Server generation is expected to produce.
 *
 * The listener admits a burst of 12 requests per source, so asserting the exact
 * set also pins the request budget: 4 for the page load, 1 selection, 2 for the
 * two-request Init, 1 session probe, and 1 sign-in. The two route-absence
 * probes below are issued outside the page and add 2 more, leaving the run
 * inside the burst without relying on replenishment.
 */
const EXPECTED_RESPONSES = [
  "200 /",
  "200 /assets/weavelit-application.js",
  "200 /assets/weavelit-application.css",
  `200 ${STATUS_PATH}`,
  `200 ${SELECTION_PATH}`,
  `200 ${RECOVERY_KEY_PATH}`,
  `200 ${INIT_PATH}`,
  `401 ${SESSION_PATH}`,
  `200 ${LOGIN_PATH}`,
] as const;

test("a first launch initializes the deployment and signs the new Administrator in", async ({
  page,
}) => {
  // The scenario generates TLS material, starts a Server, runs a complete Init
  // including an Argon2id verifier creation, and then performs a real sign-in,
  // so it needs well beyond the default per-test budget.
  test.setTimeout(180_000);

  const fixtureRoot = createFixtureRoot("weavelit-browser-init-");
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
    const requests = observeRequests(page);
    // The copy control is exercised as an operator would use it, so the page is
    // granted the clipboard permission the browser requires for it.
    await page.context().grantPermissions(["clipboard-write"], { origin: baseUrl });

    server = spawnServer(configuration);
    await waitForServerReady(server, configuration);
    const servingPid = server.pid;

    const status = page.locator("p.shell__status");
    const choice = page.locator("section.shell__choice");
    const selection = page.locator("section.shell__selection");
    const init = page.locator("section.shell__init");
    const restore = page.locator("section.shell__restore");
    const login = page.locator("section[data-authentication-state]");

    // 1. The operator opens a deployment that has never been set up.
    const document = await page.goto(baseUrl, { waitUntil: "load" });
    expect(document?.status()).toBe(200);
    await expect(status).toHaveText(UNSELECTED_MESSAGE);

    // The first launch offers two mutually exclusive paths and nothing else.
    await expect(choice).toHaveCount(1);
    await expect(selection).toHaveCount(0);
    await expect(init).toHaveCount(0);
    await expect(restore).toHaveCount(0);

    // 2. Choosing Init withdraws Restore for the rest of the workflow.
    await page.getByRole("button", { name: INIT_CHOICE_NAME }).click();
    await expect(choice).toHaveCount(0);
    await expect(restore).toHaveCount(0);
    // Both paths share the one selection surface rather than duplicating it.
    await expect(selection).toHaveCount(1);
    await expect(init).toHaveCount(0);

    // 3. Select the SQLite Application Database.
    const selectionResponse = page.waitForResponse(
      (response) => new URL(response.url()).pathname === SELECTION_PATH,
    );
    await page.getByRole("button", { name: SELECTION_ACTION_NAME }).click();
    expect((await selectionResponse).status()).toBe(200);
    await expect(status).toHaveText(SELECTED_MESSAGE);
    await expect(selection).toHaveCount(0);
    await expect(init).toHaveAttribute("data-init-state", "details");

    // 4. Assign both logs and name the first Administrator.
    await expect(page.getByRole("button", { name: PREPARE_ACTION_NAME })).toBeDisabled();
    await page.locator(SYSTEM_LOG_SELECT).selectOption("sqlite");
    await page.locator(AUDIT_LOG_SELECT).selectOption("sqlite");
    await page.locator(USERNAME_INPUT).fill(ADMINISTRATOR_USERNAME);
    await page.locator(DISPLAY_NAME_INPUT).fill(ADMINISTRATOR_DISPLAY_NAME);
    await page.locator(PASSWORD_INPUT).fill(ADMINISTRATOR_PASSWORD);
    await expect(page.getByRole("button", { name: PREPARE_ACTION_NAME })).toBeEnabled();

    // 5. The preparation request delivers the one-time recovery key.
    const delivery = page.waitForResponse(
      (response) => new URL(response.url()).pathname === RECOVERY_KEY_PATH,
    );
    await page.getByRole("button", { name: PREPARE_ACTION_NAME }).click();
    const deliveryResponse = await delivery;
    expect(deliveryResponse.request().method()).toBe("PUT");
    expect(deliveryResponse.status(), `rendered Init state: ${await init.innerText()}`).toBe(200);

    const delivered = (await deliveryResponse.json()) as {
      result: { recovery_key: string; delivery_nonce: string };
    };
    const recoveryKey = delivered.result.recovery_key;
    const deliveryNonce = delivered.result.delivery_nonce;
    expect(recoveryKey).toMatch(RECOVERY_KEY_SHAPE);
    expect(deliveryNonce).toMatch(OPAQUE_TOKEN);

    // 6. The key is displayed once, said to be unrepeatable, and copyable.
    await expect(init).toHaveAttribute("data-init-state", "key");
    await expect(page.locator(RECOVERY_KEY_OUTPUT)).toHaveValue(recoveryKey);
    await expect(page.locator("p.shell__init-key-warning")).toHaveText(KEY_ONCE_WARNING);

    await page.getByRole("button", { name: COPY_ACTION_NAME }).click();
    await expect(page.locator("p.shell__init-copy-result")).toHaveText(COPIED_MESSAGE);

    // 7. Continuing is gated on an explicit acknowledgement.
    await expect(page.getByRole("button", { name: KEY_CONTINUE_NAME })).toBeDisabled();
    await page.locator(ACKNOWLEDGEMENT_INPUT).check();
    await expect(page.getByRole("button", { name: KEY_CONTINUE_NAME })).toBeEnabled();
    await page.getByRole("button", { name: KEY_CONTINUE_NAME }).click();

    // 8. Review repeats the assignments and identity, never the secrets.
    await expect(init).toHaveAttribute("data-init-state", "review");
    await expect(page.locator("[data-init-review='username']")).toHaveText(ADMINISTRATOR_USERNAME);
    await expect(page.locator("[data-init-review='display-name']")).toHaveText(
      ADMINISTRATOR_DISPLAY_NAME,
    );
    await expect(page.locator("[data-init-review='system-log']")).toHaveText("SQLite");
    await expect(page.locator("[data-init-review='audit-log']")).toHaveText("SQLite");
    const reviewed = await page.content();
    expect(reviewed, "review repeats neither secret").not.toContain(recoveryKey);
    expect(reviewed).not.toContain(ADMINISTRATOR_PASSWORD);

    // 9. Finalization submits the derived proof and seals the deployment.
    const finalization = page.waitForResponse(
      (response) => new URL(response.url()).pathname === INIT_PATH,
    );
    await page.getByRole("button", { name: FINALIZE_ACTION_NAME }).click();
    const finalizationResponse = await finalization;
    expect(finalizationResponse.request().method()).toBe("PUT");
    // The Init surface is withdrawn the moment finalization succeeds, so the
    // rendered state is only read when the status is actually wrong.
    if (finalizationResponse.status() !== 200) {
      throw new Error(
        `finalization returned ${finalizationResponse.status()}, rendered Init state: ${await init.innerText()}`,
      );
    }
    expect(await finalizationResponse.json()).toEqual({
      result: { lifecycle: "initialized" },
      correlation_id: expect.stringMatching(/^[0-9a-f]{32}$/),
    });

    // The proof is what travels: the private key never returns to the Server.
    const submitted = finalizationResponse.request().postData() ?? "";
    const finalized = JSON.parse(submitted) as { recovery_key_proof: string };
    expect(finalized.recovery_key_proof, "a native HMAC-SHA-256 proof").toMatch(PROOF_SHAPE);
    expect(submitted, "the finalization body carries no private key").not.toContain(recoveryKey);
    expect(submitted, "the finalization body carries no nonce").not.toContain(deliveryNonce);

    // 10. Normal operation activates in the same Server process, and every
    // setup control is withdrawn without the page reloading or re-reading a
    // status projection that is no longer served.
    await expect(status).toHaveAttribute("data-status-state", "initialized");
    await expect(status).toHaveText(INITIALIZED_MESSAGE);
    await expect(init).toHaveCount(0);
    await expect(choice).toHaveCount(0);
    await expect(selection).toHaveCount(0);
    await expect(restore).toHaveCount(0);
    expect(server.observedExit(), "the Server did not exit during Init").toBeNull();
    expect(server.pid, "Init ran in the process that served the page").toBe(servingPid);

    // 11. The new Administrator signs in, with no second factor required.
    await expect(login).toHaveAttribute("data-authentication-state", "unauthenticated");
    const accepted = page.waitForResponse(
      (response) => new URL(response.url()).pathname === LOGIN_PATH,
    );
    await page.locator(LOGIN_USERNAME_INPUT).fill(ADMINISTRATOR_USERNAME);
    await page.locator(LOGIN_PASSWORD_INPUT).fill(ADMINISTRATOR_PASSWORD);
    await page.getByRole("button", { name: LOGIN_ACTION_NAME }).click();
    const acceptedResponse = await accepted;
    expect(acceptedResponse.status(), `rendered sign-in state: ${await login.innerText()}`).toBe(
      200,
    );
    expect(await acceptedResponse.json()).toEqual({
      result: { authenticated: true },
      correlation_id: expect.stringMatching(/^[0-9a-f]{32}$/),
    });
    await expect(login).toHaveAttribute("data-authentication-state", "authenticated");
    await expect(page.locator("p.shell__login-authenticated")).toHaveText(AUTHENTICATED_MESSAGE);

    expect(sorted(observed), "requests the browser issued").toEqual(sorted(EXPECTED_RESPONSES));

    // 12. Both Init routes are absent from the sealed deployment's surface, so
    // no second key can be delivered and no second finalization attempted.
    for (const path of [RECOVERY_KEY_PATH, INIT_PATH]) {
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
      expect(absent.status(), `${path} after Init`).toBe(404);
      expect(await absent.json()).toEqual({ error: "not_found" });
    }
    expect(server.observedExit(), "the Server is still the same process").toBeNull();

    // 13. Neither the recovery key, the nonce, nor the password reached a URL,
    // a cookie, browser storage, the rendered page, or the Server's own output.
    const secrets = [recoveryKey, deliveryNonce, ADMINISTRATOR_PASSWORD];

    for (const request of requests) {
      for (const secret of secrets) {
        expect(request.url(), "no request URL carries setup material").not.toContain(secret);
      }
    }

    for (const cookie of await page.context().cookies()) {
      for (const secret of secrets) {
        expect(cookie.value, `${cookie.name} carries no setup material`).not.toContain(secret);
      }
    }
    const browserState = await page.evaluate(() => ({
      cookie: globalThis.document.cookie,
      localStorage: JSON.stringify(globalThis.localStorage),
      sessionStorage: JSON.stringify(globalThis.sessionStorage),
    }));
    expect(browserState.localStorage, "the workflow writes no local storage").toBe("{}");
    expect(browserState.sessionStorage, "the workflow writes no session storage").toBe("{}");
    for (const secret of secrets) {
      expect(browserState.cookie, "no readable cookie carries setup material").not.toContain(
        secret,
      );
    }

    const rendered = await page.content();
    const serverOutput =
      capturedOutput(configuration.stdoutPath) + capturedOutput(configuration.stderrPath);
    for (const secret of secrets) {
      expect(rendered, "the page renders no setup material").not.toContain(secret);
      expect(serverOutput, "the Server logs no setup material").not.toContain(secret);
    }
  } finally {
    if (server !== null) {
      await terminateServer(server).catch(() => undefined);
    }
    rmSync(fixtureRoot, { recursive: true, force: true });
  }
});
