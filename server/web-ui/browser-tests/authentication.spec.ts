import { readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

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

const SELECTION_ACTION_NAME = "Select SQLite";
const RESTORE_CHOICE_NAME = "Restore from a backup";
const RESTORE_ACTION_NAME = "Restore backup";
const LOGIN_ACTION_NAME = "Sign in";
const LOGIN_FAILED_MESSAGE = "Sign-in failed.";
const AUTHENTICATED_MESSAGE = "You are signed in.";

const STATUS_PATH = "/api/v1/status";
const SELECTION_PATH = "/api/v1/application-database";
const RESTORE_KEY_PATH = "/api/v1/restore";
const RESTORE_ARTIFACT_PATH = "/api/v1/restore/artifact";
const LOGIN_PATH = "/api/v1/auth/login";
const SESSION_PATH = "/api/v1/auth/session";

const ARTIFACT_INPUT = "#weavelit-restore-artifact";
const RECOVERY_KEY_INPUT = "#weavelit-restore-recovery-key";
const USERNAME_INPUT = "#weavelit-login-username";
const PASSWORD_INPUT = "#weavelit-login-password";
const LOGIN_PANEL = "section[data-authentication-state]";

const SESSION_COOKIE_NAME = "__Host-weavelit_session";
const CSRF_COOKIE_NAME = "__Host-weavelit_csrf";
const CSRF_HEADER = "x-weavelit-csrf";

/** The shape every opaque token the Server issues is required to have. */
const OPAQUE_TOKEN = /^[A-Za-z0-9_-]{1,48}$/;

/**
 * The credential the committed Restore fixtures restore an account with.
 *
 * The fixture generator derives the stored verifier from this exact password at
 * the approved Argon2id profile, and a Rust test proves the derived verifier
 * authenticates it through the real password authenticator. Restore is the only
 * way this deployment can acquire an account, so it is the only way a browser
 * test can reach a real sign-in.
 */
const FIXTURE_USERNAME = "administrator";
const FIXTURE_PASSWORD = "fixture-administrator-password";
const WRONG_PASSWORD = "fixture-administrator-passworD";
const UNKNOWN_USERNAME = "no-such-account";

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
 * Every response a page load against the pre-operational surface produces.
 *
 * The listener admits a burst of 12 requests per source and each Server
 * generation is counted by its own limiter, so asserting the exact set per
 * generation also pins the request budget and keeps a drifting `429` visible.
 */
const PRE_OPERATIONAL_LOAD = [
  "200 /",
  "200 /assets/weavelit-application.js",
  "200 /assets/weavelit-application.css",
  `200 ${STATUS_PATH}`,
] as const;

/**
 * Every response a page load against the sealed surface produces.
 *
 * The pre-operational status route is gone, so the shell falls through to the
 * sign-in panel, which probes the session route exactly once.
 */
function operationalLoad(sessionStatus: number): readonly string[] {
  return [
    "200 /",
    "200 /assets/weavelit-application.js",
    "200 /assets/weavelit-application.css",
    `404 ${STATUS_PATH}`,
    `${sessionStatus} ${SESSION_PATH}`,
  ];
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

test("an operator signs in to a restored deployment and the session survives a restart", async ({
  page,
}) => {
  // The scenario runs three Server generations, generates TLS material, drives a
  // complete Restore, and performs three real Argon2id verifications, so it
  // needs well beyond the default per-test budget.
  test.setTimeout(180_000);

  const recoveryKey = committedKey("valid-identity.txt");
  const fixtureRoot = createFixtureRoot("weavelit-browser-authentication-");
  let server: RunningServer | null = null;

  try {
    const { certificatePath, privateKeyPath } = generateTlsMaterial(fixtureRoot);
    const stateRoot = createStateRoot(fixtureRoot);
    const port = await reserveLoopbackPort();
    // Recorded once and reused verbatim by both restarts, so every generation
    // runs against the identical state root and listener address.
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

    // ---- Generation 1: restore a deployment that carries an account. --------

    server = spawnServer(configuration);
    await waitForServerReady(server, configuration);

    const status = page.locator("p.shell__status");
    const restore = page.locator("section.shell__restore");
    const login = page.locator(LOGIN_PANEL);

    const document = await page.goto(baseUrl, { waitUntil: "load" });
    expect(document?.status()).toBe(200);
    await expect(status).toHaveAttribute("data-status-state", "available");
    // The pre-operational surface offers no sign-in control at all.
    await expect(login).toHaveCount(0);

    await page.getByRole("button", { name: RESTORE_CHOICE_NAME }).click();
    const selectionResponse = page.waitForResponse(
      (response) => new URL(response.url()).pathname === SELECTION_PATH,
    );
    await page.getByRole("button", { name: SELECTION_ACTION_NAME }).click();
    expect((await selectionResponse).status()).toBe(200);
    await expect(restore).toHaveCount(1);

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
    expect((await upload).status(), `rendered Restore state: ${await restore.innerText()}`).toBe(
      200,
    );
    await expect(restore).toHaveAttribute("data-restore-state", "completed");

    expect(sorted(observed), "requests issued against the first generation").toEqual(
      sorted([
        ...PRE_OPERATIONAL_LOAD,
        `200 ${SELECTION_PATH}`,
        `202 ${RESTORE_KEY_PATH}`,
        `200 ${RESTORE_ARTIFACT_PATH}`,
      ]),
    );
    // Nothing has been signed in to yet, so no cookie can exist.
    expect(await page.context().cookies(), "the pre-operational surface sets no cookie").toEqual(
      [],
    );

    // ---- Generation 2: sign in to the sealed deployment. --------------------

    let exit = await terminateServer(server);
    server = null;
    expect(exit.signal, "the Server was not killed").toBeNull();
    expect(exit.code, "the Server shut down cleanly").toBe(0);

    server = spawnServer(configuration);
    await waitForServerReady(server, configuration);

    const secondGeneration = observed.length;
    const reloaded = await page.reload({ waitUntil: "load" });
    expect(reloaded?.status()).toBe(200);
    // The sealed surface serves no status projection, and the sign-in panel
    // reports that the browser holds nothing that authenticates.
    await expect(login).toHaveAttribute("data-authentication-state", "unauthenticated");

    // 1. A denied sign-in for an unknown account.
    const unknownDenial = page.waitForResponse(
      (response) => new URL(response.url()).pathname === LOGIN_PATH,
    );
    await page.locator(USERNAME_INPUT).fill(UNKNOWN_USERNAME);
    await page.locator(PASSWORD_INPUT).fill(WRONG_PASSWORD);
    await page.getByRole("button", { name: LOGIN_ACTION_NAME }).click();
    const unknownResponse = await unknownDenial;
    await expect(login).toHaveAttribute("data-authentication-state", "failed");
    const unknownRendered = await page.locator("p.shell__login-failure").innerText();

    // 2. A denied sign-in for the real account with a wrong password.
    const wrongDenial = page.waitForResponse(
      (response) => new URL(response.url()).pathname === LOGIN_PATH,
    );
    await page.locator(USERNAME_INPUT).fill(FIXTURE_USERNAME);
    await page.locator(PASSWORD_INPUT).fill(WRONG_PASSWORD);
    await page.getByRole("button", { name: LOGIN_ACTION_NAME }).click();
    const wrongResponse = await wrongDenial;
    await expect(login).toHaveAttribute("data-authentication-state", "failed");
    const wrongRendered = await page.locator("p.shell__login-failure").innerText();

    // Neither the Server's answer nor the rendered page distinguishes an
    // unknown account from a wrong password. The bodies are compared field by
    // field with only the per-request correlation identifier set aside, so a
    // future divergence anywhere else fails here.
    expect(unknownResponse.status(), "both denials share one status").toBe(401);
    expect(wrongResponse.status()).toBe(unknownResponse.status());
    expect(sorted(Object.keys(await wrongResponse.allHeaders()))).toEqual(
      sorted(Object.keys(await unknownResponse.allHeaders())),
    );
    const unknownBody = (await unknownResponse.json()) as Record<string, unknown>;
    const wrongBody = (await wrongResponse.json()) as Record<string, unknown>;
    expect(unknownBody.error, "the denial carries the one authentication code").toBe(
      "authentication_failed",
    );
    expect({ ...wrongBody, correlation_id: null }).toEqual({
      ...unknownBody,
      correlation_id: null,
    });
    expect(unknownBody.correlation_id).not.toBe(wrongBody.correlation_id);
    expect(wrongRendered, "both denials render one fixed message").toBe(unknownRendered);
    expect(unknownRendered).toBe(LOGIN_FAILED_MESSAGE);
    // A denial issues nothing: no session exists to hand out.
    expect(await page.context().cookies(), "a denied sign-in sets no cookie").toEqual([]);

    // 3. The accepted sign-in.
    const accepted = page.waitForResponse(
      (response) => new URL(response.url()).pathname === LOGIN_PATH,
    );
    await page.locator(USERNAME_INPUT).fill(FIXTURE_USERNAME);
    await page.locator(PASSWORD_INPUT).fill(FIXTURE_PASSWORD);
    await page.getByRole("button", { name: LOGIN_ACTION_NAME }).click();
    const acceptedResponse = await accepted;
    expect(acceptedResponse.request().method()).toBe("PUT");
    expect(acceptedResponse.status(), `rendered sign-in state: ${await login.innerText()}`).toBe(
      200,
    );
    // The Server returns no token in the body: the session travels in cookies.
    expect(await acceptedResponse.json()).toEqual({
      result: { authenticated: true },
      correlation_id: expect.stringMatching(/^[0-9a-f]{32}$/),
    });
    await expect(login).toHaveAttribute("data-authentication-state", "authenticated");
    await expect(page.locator("p.shell__login-authenticated")).toHaveText(AUTHENTICATED_MESSAGE);

    expect(
      sorted(observed.slice(secondGeneration)),
      "requests against the second generation",
    ).toEqual(
      sorted([
        ...operationalLoad(401),
        `401 ${LOGIN_PATH}`,
        `401 ${LOGIN_PATH}`,
        `200 ${LOGIN_PATH}`,
      ]),
    );

    // ---- The issued cookies, read as the browser actually holds them. -------

    const jar = await page.context().cookies();
    expect(
      sorted(jar.map((cookie) => cookie.name)),
      "the deployment sets exactly the two approved cookies",
    ).toEqual(sorted([SESSION_COOKIE_NAME, CSRF_COOKIE_NAME]));

    const sessionCookie = jar.find((cookie) => cookie.name === SESSION_COOKIE_NAME);
    const csrfCookie = jar.find((cookie) => cookie.name === CSRF_COOKIE_NAME);
    expect(sessionCookie?.value, "the session value is a canonical opaque token").toMatch(
      OPAQUE_TOKEN,
    );
    expect(csrfCookie?.value, "the CSRF value is a canonical opaque token").toMatch(OPAQUE_TOKEN);
    const sessionValue = sessionCookie?.value ?? "";
    const csrfValue = csrfCookie?.value ?? "";
    expect(sessionValue, "the two issued values are independent").not.toBe(csrfValue);

    // The session value is withheld from script; the CSRF value is deliberately
    // readable, because the client has to echo it.
    expect(sessionCookie?.httpOnly).toBe(true);
    expect(sessionCookie?.secure).toBe(true);
    expect(sessionCookie?.sameSite).toBe("Strict");
    expect(sessionCookie?.path).toBe("/");
    expect(csrfCookie?.httpOnly).toBe(false);
    expect(csrfCookie?.secure).toBe(true);
    expect(csrfCookie?.sameSite).toBe("Strict");
    expect(csrfCookie?.path).toBe("/");

    const scriptCookies = await page.evaluate(() => globalThis.document.cookie);
    expect(scriptCookies, "the readable jar carries the CSRF value").toContain(csrfValue);
    expect(scriptCookies, "the readable jar withholds the session value").not.toContain(
      sessionValue,
    );

    // The password really was transmitted, so asserting its absence everywhere
    // else below is an assertion about a value this run actually produced.
    const loginRequests = requests.filter(
      (request) => new URL(request.url()).pathname === LOGIN_PATH,
    );
    expect(loginRequests, "three sign-in attempts were submitted").toHaveLength(3);
    expect(
      loginRequests[2]?.postData(),
      "the accepted attempt carried the credential in its body",
    ).toContain(FIXTURE_PASSWORD);
    expect(loginRequests[0]?.postData(), "the denied attempts carried the wrong one").toContain(
      WRONG_PASSWORD,
    );
    for (const request of loginRequests) {
      // Login is the bootstrap request: no per-session token exists yet.
      expect((await request.allHeaders())[CSRF_HEADER]).toBe("1");
    }

    // ---- Generation 3: the session outlives the process that issued it. -----

    exit = await terminateServer(server);
    server = null;
    expect(exit.signal, "the Server was not killed").toBeNull();
    expect(exit.code).toBe(0);

    server = spawnServer(configuration);
    await waitForServerReady(server, configuration);

    const thirdGeneration = observed.length;
    const requestsBeforeRestart = requests.length;
    const restored = await page.reload({ waitUntil: "load" });
    expect(restored?.status()).toBe(200);
    // No credential is re-entered: the new process recognises the cookies the
    // previous process issued, which is only possible from a persisted session.
    await expect(login).toHaveAttribute("data-authentication-state", "authenticated");
    await expect(page.locator("p.shell__login-authenticated")).toHaveText(AUTHENTICATED_MESSAGE);

    expect(
      sorted(observed.slice(thirdGeneration)),
      "requests against the third generation",
    ).toEqual(sorted(operationalLoad(200)));

    // The presented session resolves to the account the fixture restored, and
    // the probe echoed the CSRF value the browser is actually holding.
    const probe = requests
      .slice(requestsBeforeRestart)
      .find((request) => new URL(request.url()).pathname === SESSION_PATH);
    expect(probe, "the reload probed the session route").toBeDefined();
    const probeHeaders = await (probe as Request).allHeaders();
    expect(probeHeaders[CSRF_HEADER], "the client echoed the issued CSRF value").toBe(csrfValue);
    const probeResponse = await (probe as Request).response();
    expect(await probeResponse?.json()).toEqual({
      result: { account_id: expect.stringMatching(/^[0-9a-f]+$/), client_module: "web-ui" },
      correlation_id: expect.stringMatching(/^[0-9a-f]{32}$/),
    });

    // ---- Nothing sensitive escaped its one permitted channel. ---------------

    const confidential = [FIXTURE_PASSWORD, WRONG_PASSWORD, sessionValue, csrfValue];
    const requestedUrls = [page.url(), ...requests.map((request) => request.url())];
    for (const url of requestedUrls) {
      for (const secret of confidential) {
        expect(url, "no request target carries a credential or token").not.toContain(secret);
      }
    }

    const browserState = await page.evaluate(() => ({
      localStorage: JSON.stringify(globalThis.localStorage),
      sessionStorage: JSON.stringify(globalThis.sessionStorage),
    }));
    expect(browserState.localStorage, "nothing is persisted to local storage").toBe("{}");
    expect(browserState.sessionStorage, "nothing is persisted to session storage").toBe("{}");

    const rendered = await page.content();
    const serverOutput =
      capturedOutput(configuration.stdoutPath) + capturedOutput(configuration.stderrPath);
    // The guard belongs on the needles, not on the haystacks. A healthy Server
    // legitimately writes nothing at all, so requiring output here would fail a
    // correct run; but searching for an empty or unobserved string would pass
    // against anything, so each secret is pinned to a real observed value.
    expect(confidential, "every searched secret is a real observed value").toEqual(
      confidential.filter((secret) => typeof secret === "string" && secret.length > 0),
    );
    expect(sessionValue, "the session value came from the issued cookie").not.toBe(csrfValue);
    expect(rendered.length, "the page rendered content to search").toBeGreaterThan(0);
    for (const secret of confidential) {
      expect(rendered, "the page renders no credential or token").not.toContain(secret);
      expect(serverOutput, "the Server logs no credential or token").not.toContain(secret);
    }
  } finally {
    if (server !== null) {
      await terminateServer(server).catch(() => undefined);
    }
    rmSync(fixtureRoot, { recursive: true, force: true });
  }
});
