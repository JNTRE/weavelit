import { readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test, type Page, type Request, type Route } from "@playwright/test";

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

const SELECTION_ACTION_NAME = "Select SQLite";
const RESTORE_CHOICE_NAME = "Restore from a backup";
const RESTORE_ACTION_NAME = "Restore backup";
const LOGIN_ACTION_NAME = "Sign in";
const VERIFY_ACTION_NAME = "Verify code";
const CONFIRM_ACTION_NAME = "Confirm authenticator app";
const ATTEMPT_ENDED_MESSAGE =
  "That code was not accepted. This sign-in attempt has ended, so sign in again to start a new one.";

const SELECTION_PATH = "/api/v1/application-database";
const RESTORE_KEY_PATH = "/api/v1/restore";
const RESTORE_ARTIFACT_PATH = "/api/v1/restore/artifact";
const LOGIN_PATH = "/api/v1/auth/login";
const SESSION_PATH = "/api/v1/auth/session";
const MFA_VERIFY_PATH = "/api/v1/auth/mfa/verify";
const MFA_ENROLLMENT_PATH = "/api/v1/auth/mfa/enrollment";
const MFA_CONFIRM_PATH = "/api/v1/auth/mfa/enrollment/confirm";

const ARTIFACT_INPUT = "#weavelit-restore-artifact";
const RECOVERY_KEY_INPUT = "#weavelit-restore-recovery-key";
const USERNAME_INPUT = "#weavelit-login-username";
const PASSWORD_INPUT = "#weavelit-login-password";
const CODE_INPUT = "#weavelit-login-code";
const SETUP_KEY_INPUT = "#weavelit-login-setup-key";
const SETUP_LINK_INPUT = "#weavelit-login-setup-link";
const LOGIN_PANEL = "section[data-authentication-state]";

const CSRF_HEADER = "x-weavelit-csrf";
const JSON_MEDIA_TYPE = "application/json; charset=utf-8";

const FIXTURE_USERNAME = "administrator";
const FIXTURE_PASSWORD = "fixture-administrator-password";

/**
 * The second-factor values this scenario drives the Web UI with.
 *
 * The committed Restore fixture carries an account with no enrolled factor and
 * no requirement to hold one, so the real Server answers its login with a
 * session rather than a continuation. The four second-factor routes are
 * therefore answered from the browser with the exact envelopes the Server's own
 * contract renders, which is what makes the step deterministic: no clock, no
 * shared secret, and no code arithmetic takes part in the run. The Rust tests
 * own the proof that the Server produces these envelopes.
 */
const CONTINUATION = "Y29udGludWF0aW9uLXZhbHVlLWZvci10ZXN0aW5n";
const ENROLLMENT = "ZW5yb2xsbWVudC12YWx1ZS1mb3ItdGVzdGluZw";
const SECRET = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
const PROVISIONING_URI =
  "otpauth://totp/Weavelit:administrator?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ" +
  "&issuer=Weavelit&algorithm=SHA1&digits=6&period=30";
const CODE = "123456";
const CORRELATION = "0123456789abcdef0123456789abcdef";

const fixturesDirectory = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../crates/core/weavelit-server-restore/tests/fixtures",
);
const VALID_ARTIFACT_PATH = join(fixturesDirectory, "valid-web-ui-sqlite.wlitbackup");

function committedKey(name: string): string {
  return readFileSync(join(fixturesDirectory, name), "utf8").trim();
}

function envelope(result: Record<string, unknown>): string {
  return JSON.stringify({ result, correlation_id: CORRELATION });
}

async function fulfilJson(route: Route, status: number, body: string): Promise<void> {
  await route.fulfill({ status, contentType: JSON_MEDIA_TYPE, body });
}

/** Every request the browser issued, so a target can be searched for a secret. */
function observeRequests(page: Page): Request[] {
  const observed: Request[] = [];
  page.on("request", (request) => {
    observed.push(request);
  });
  return observed;
}

/** Reads what the browser actually holds in its two script-visible stores. */
async function browserState(page: Page): Promise<{ local: string; session: string }> {
  return page.evaluate(() => ({
    local: JSON.stringify(globalThis.localStorage),
    session: JSON.stringify(globalThis.sessionStorage),
  }));
}

let fixtureRoot = "";
let configuration: ServerSpawnConfiguration | null = null;
let server: RunningServer | null = null;
let baseUrl = "";

/**
 * Restores a deployment once and leaves it sealed for every scenario below.
 *
 * Restore is the only way this deployment can acquire an account, and a sealed
 * deployment is the only one that serves the sign-in panel at all, so the
 * scenarios run against a Server that really did seal rather than against a
 * stubbed status projection.
 */
test.beforeAll(async ({ browser }) => {
  test.setTimeout(180_000);

  const recoveryKey = committedKey("valid-identity.txt");
  fixtureRoot = createFixtureRoot("weavelit-browser-mfa-");
  const { certificatePath, privateKeyPath } = generateTlsMaterial(fixtureRoot);
  const stateRoot = createStateRoot(fixtureRoot);
  const port = await reserveLoopbackPort();
  configuration = {
    binary: releaseBinaryPath(),
    listenerAddress: `127.0.0.1:${port}`,
    certificatePath,
    privateKeyPath,
    stateRoot,
    stdoutPath: join(fixtureRoot, "server-stdout.log"),
    stderrPath: join(fixtureRoot, "server-stderr.log"),
  };
  baseUrl = `https://${configuration.listenerAddress}`;

  const restoring = spawnServer(configuration);
  await waitForServerReady(restoring, configuration);

  const page = await browser.newPage();
  try {
    await page.goto(baseUrl, { waitUntil: "load" });
    await page.getByRole("button", { name: RESTORE_CHOICE_NAME }).click();
    const selection = page.waitForResponse(
      (response) => new URL(response.url()).pathname === SELECTION_PATH,
    );
    await page.getByRole("button", { name: SELECTION_ACTION_NAME }).click();
    expect((await selection).status()).toBe(200);

    await page.locator(ARTIFACT_INPUT).setInputFiles(VALID_ARTIFACT_PATH);
    await page.locator(RECOVERY_KEY_INPUT).fill(recoveryKey);
    const ticket = page.waitForResponse(
      (response) => new URL(response.url()).pathname === RESTORE_KEY_PATH,
    );
    const upload = page.waitForResponse(
      (response) => new URL(response.url()).pathname === RESTORE_ARTIFACT_PATH,
    );
    await page.getByRole("button", { name: RESTORE_ACTION_NAME }).click();
    expect((await ticket).status()).toBe(202);
    expect((await upload).status()).toBe(200);
    // The completed Restore withdraws the setup surface in the loaded page,
    // which is what confirms the deployment sealed before this Server stops.
    await expect(page.locator("section.shell__restore")).toHaveCount(0);
    await expect(page.locator("p.shell__status")).toHaveAttribute(
      "data-status-state",
      "initialized",
    );
  } finally {
    await page.close();
  }

  const exit = await terminateServer(restoring);
  expect(exit.signal, "the Server was not killed").toBeNull();
  expect(exit.code, "the Server shut down cleanly").toBe(0);
});

/**
 * Starts one Server generation per scenario over the same restored state root.
 *
 * Each generation is counted by its own request limiter, so a scenario spends
 * only its own page load out of the listener's burst and a drifting `429`
 * cannot appear from scenarios accumulating against one another.
 */
test.beforeEach(async () => {
  if (configuration === null) {
    throw new Error("the restored fixture did not start");
  }
  server = spawnServer(configuration);
  await waitForServerReady(server, configuration);
});

test.afterEach(async () => {
  if (server !== null) {
    await terminateServer(server).catch(() => undefined);
    server = null;
  }
});

test.afterAll(() => {
  if (fixtureRoot !== "") {
    rmSync(fixtureRoot, { recursive: true, force: true });
  }
});

test("a code submitted after a 202 mfa_required completes the sign-in", async ({ page }) => {
  test.setTimeout(60_000);

  const requests = observeRequests(page);
  const verified: Request[] = [];

  await page.route(`${baseUrl}${LOGIN_PATH}`, (route) =>
    fulfilJson(route, 202, envelope({ mfa: "mfa_required", continuation: CONTINUATION })),
  );
  await page.route(`${baseUrl}${MFA_VERIFY_PATH}`, (route) => {
    verified.push(route.request());
    return fulfilJson(route, 200, envelope({ authenticated: true }));
  });
  let sessionProbes = 0;
  await page.route(`${baseUrl}${SESSION_PATH}`, (route) => {
    sessionProbes += 1;
    return sessionProbes === 1
      ? route.continue()
      : fulfilJson(
          route,
          200,
          envelope({
            account_id: "a1".repeat(16),
            client_module: "web-ui",
            password_change_required: false,
          }),
        );
  });

  const login = page.locator(LOGIN_PANEL);
  await page.goto(baseUrl, { waitUntil: "load" });
  await expect(login).toHaveAttribute("data-authentication-state", "unauthenticated");

  await page.locator(USERNAME_INPUT).fill(FIXTURE_USERNAME);
  await page.locator(PASSWORD_INPUT).fill(FIXTURE_PASSWORD);
  await page.getByRole("button", { name: LOGIN_ACTION_NAME }).click();

  // The verified password issued no session, so the panel asks for the factor
  // instead of reporting a sign-in.
  await expect(login).toHaveAttribute("data-authentication-state", "second-factor");
  await expect(page.locator(USERNAME_INPUT)).toHaveCount(0);
  await expect(page.locator(PASSWORD_INPUT)).toHaveCount(0);
  const verify = page.getByRole("button", { name: VERIFY_ACTION_NAME });
  await expect(verify).toBeDisabled();

  await page.locator(CODE_INPUT).fill(CODE);
  await expect(verify).toBeEnabled();
  await verify.click();

  await expect(page.getByRole("heading", { name: "Accounts" })).toBeVisible();

  expect(verified, "the code was submitted exactly once").toHaveLength(1);
  const submitted = verified[0] as Request;
  expect(submitted.method()).toBe("PUT");
  expect(submitted.postData()).toBe(JSON.stringify({ continuation: CONTINUATION, code: CODE }));
  // The route carries no session, so it echoes the pre-session literal login
  // itself echoes rather than a per-session token.
  expect((await submitted.allHeaders())[CSRF_HEADER]).toBe("1");

  // The continuation reached exactly one request body and nothing else.
  const rendered = await page.content();
  const state = await browserState(page);
  for (const url of [page.url(), ...requests.map((request) => request.url())]) {
    expect(url, "no request target carries the continuation").not.toContain(CONTINUATION);
    expect(url).not.toContain(FIXTURE_PASSWORD);
  }
  expect(rendered).not.toContain(CONTINUATION);
  expect(state.local, "nothing is persisted to local storage").toBe("{}");
  expect(state.session, "nothing is persisted to session storage").toBe("{}");
});

test("a rejected code after a 202 mfa_required ends the attempt", async ({ page }) => {
  test.setTimeout(60_000);

  const verified: Request[] = [];

  await page.route(`${baseUrl}${LOGIN_PATH}`, (route) =>
    fulfilJson(route, 202, envelope({ mfa: "mfa_required", continuation: CONTINUATION })),
  );
  await page.route(`${baseUrl}${MFA_VERIFY_PATH}`, (route) => {
    verified.push(route.request());
    return fulfilJson(
      route,
      401,
      JSON.stringify({ error: "authentication_failed", correlation_id: CORRELATION }),
    );
  });

  const login = page.locator(LOGIN_PANEL);
  await page.goto(baseUrl, { waitUntil: "load" });
  await expect(login).toHaveAttribute("data-authentication-state", "unauthenticated");

  await page.locator(USERNAME_INPUT).fill(FIXTURE_USERNAME);
  await page.locator(PASSWORD_INPUT).fill(FIXTURE_PASSWORD);
  await page.getByRole("button", { name: LOGIN_ACTION_NAME }).click();
  await expect(login).toHaveAttribute("data-authentication-state", "second-factor");

  await page.locator(CODE_INPUT).fill(CODE);
  await page.getByRole("button", { name: VERIFY_ACTION_NAME }).click();

  // The continuation was spent by the refused submission, so the panel states
  // that the attempt is over rather than offering the same code entry again.
  await expect(login).toHaveAttribute("data-authentication-state", "attempt-ended");
  await expect(page.locator("p.shell__login-attempt-ended")).toHaveText(ATTEMPT_ENDED_MESSAGE);
  await expect(page.locator(CODE_INPUT)).toHaveCount(0);
  await expect(page.locator(USERNAME_INPUT)).toHaveCount(1);
  await expect(page.locator(PASSWORD_INPUT)).toHaveValue("");

  expect(verified, "the spent continuation was presented exactly once").toHaveLength(1);
  expect(await page.content()).not.toContain(CONTINUATION);
});

test("an enrollment after a 202 mfa_enrollment_required discloses its key and link once", async ({
  page,
}) => {
  test.setTimeout(60_000);

  const requests = observeRequests(page);
  const opened: Request[] = [];
  const confirmed: Request[] = [];

  await page.route(`${baseUrl}${LOGIN_PATH}`, (route) =>
    fulfilJson(
      route,
      202,
      envelope({ mfa: "mfa_enrollment_required", continuation: CONTINUATION }),
    ),
  );
  await page.route(`${baseUrl}${MFA_ENROLLMENT_PATH}`, (route) => {
    opened.push(route.request());
    return fulfilJson(
      route,
      200,
      envelope({ secret: SECRET, provisioning_uri: PROVISIONING_URI, enrollment: ENROLLMENT }),
    );
  });
  await page.route(`${baseUrl}${MFA_CONFIRM_PATH}`, (route) => {
    confirmed.push(route.request());
    return fulfilJson(route, 200, envelope({ authenticated: true }));
  });
  let sessionProbes = 0;
  await page.route(`${baseUrl}${SESSION_PATH}`, (route) => {
    sessionProbes += 1;
    return sessionProbes === 1
      ? route.continue()
      : fulfilJson(
          route,
          200,
          envelope({
            account_id: "a1".repeat(16),
            client_module: "web-ui",
            password_change_required: false,
          }),
        );
  });

  const login = page.locator(LOGIN_PANEL);
  await page.goto(baseUrl, { waitUntil: "load" });
  await expect(login).toHaveAttribute("data-authentication-state", "unauthenticated");

  await page.locator(USERNAME_INPUT).fill(FIXTURE_USERNAME);
  await page.locator(PASSWORD_INPUT).fill(FIXTURE_PASSWORD);
  await page.getByRole("button", { name: LOGIN_ACTION_NAME }).click();

  await expect(login).toHaveAttribute("data-authentication-state", "enrollment");
  // The one disclosure is presented where the operator can capture it, and it
  // is presented as read-only text rather than as a navigation target.
  await expect(page.locator(SETUP_KEY_INPUT)).toHaveValue(SECRET);
  await expect(page.locator(SETUP_LINK_INPUT)).toHaveValue(PROVISIONING_URI);
  await expect(page.locator(`${SETUP_KEY_INPUT}[readonly]`)).toHaveCount(1);
  await expect(page.locator(`${SETUP_LINK_INPUT}[readonly]`)).toHaveCount(1);
  await expect(page.locator("p.shell__login-disclosure")).toContainText("shown once");
  await expect(page.locator(`${LOGIN_PANEL} a`)).toHaveCount(0);

  expect(opened, "the disclosure was requested exactly once").toHaveLength(1);
  expect((opened[0] as Request).postData()).toBe(JSON.stringify({ continuation: CONTINUATION }));

  await page.locator(CODE_INPUT).fill(CODE);
  await page.getByRole("button", { name: CONFIRM_ACTION_NAME }).click();
  await expect(page.getByRole("heading", { name: "Accounts" })).toBeVisible();

  expect(confirmed).toHaveLength(1);
  expect((confirmed[0] as Request).postData()).toBe(
    JSON.stringify({ enrollment: ENROLLMENT, code: CODE }),
  );
  // Still exactly one disclosure: completing the enrollment did not ask for the
  // secret again, and nothing could have returned it if it had.
  expect(opened).toHaveLength(1);

  // The disclosed values are gone from the page and were never written to any
  // browser store or to a request target.
  const rendered = await page.content();
  const state = await browserState(page);
  await expect(page.locator(SETUP_KEY_INPUT)).toHaveCount(0);
  await expect(page.locator(SETUP_LINK_INPUT)).toHaveCount(0);
  const disclosed = [SECRET, PROVISIONING_URI, ENROLLMENT, CONTINUATION];
  for (const secret of disclosed) {
    expect(rendered, "the page renders no disclosed value").not.toContain(secret);
    expect(state.local, "no disclosed value is persisted to local storage").not.toContain(secret);
    expect(state.session, "no disclosed value is persisted to session storage").not.toContain(
      secret,
    );
    expect(page.url(), "the URL carries no disclosed value").not.toContain(secret);
    for (const request of requests) {
      expect(request.url(), "no request target carries a disclosed value").not.toContain(secret);
    }
  }
  expect(state.local).toBe("{}");
  expect(state.session).toBe("{}");
  expect(await page.context().cookies(), "no disclosed value reached a cookie").toEqual(
    expect.not.arrayContaining([expect.objectContaining({ value: SECRET })]),
  );
});
