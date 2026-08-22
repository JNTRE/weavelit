import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { LoginPanel } from "./weavelit-login-form";

const CORRELATION = "0123456789abcdef0123456789abcdef";
const ACCOUNT = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const PUBLIC_ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const USERNAME = "administrator";
const PASSWORD = "fixture-administrator-password";
const CONTINUATION = "Y29udGludWF0aW9uLXZhbHVlLWZvci10ZXN0aW5n";
const ENROLLMENT = "ZW5yb2xsbWVudC12YWx1ZS1mb3ItdGVzdGluZw";
const SECRET = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
const PROVISIONING_URI =
  "otpauth://totp/Weavelit:administrator?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ" +
  "&issuer=Weavelit&algorithm=SHA1&digits=6&period=30";
const CODE = "123456";

const SESSION_PATH = "/api/v1/auth/session";
const LOGIN_PATH = "/api/v1/auth/login";
const MFA_VERIFY_PATH = "/api/v1/auth/mfa/verify";
const MFA_ENROLLMENT_PATH = "/api/v1/auth/mfa/enrollment";
const MFA_CONFIRM_PATH = "/api/v1/auth/mfa/enrollment/confirm";

const ATTEMPT_ENDED_MESSAGE =
  "That code was not accepted. This sign-in attempt has ended, so sign in again to start a new one.";

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function unauthenticatedProbe(): Promise<Response> {
  return Promise.resolve(
    jsonResponse({ error: "session_invalid", correlation_id: CORRELATION }, 401),
  );
}

/** Starts unauthenticated, then reports the ordinary session a completed login established. */
function initialUnauthenticatedProbeOnly(): () => Promise<Response> {
  let probes = 0;
  return () => {
    probes += 1;
    return probes === 1 ? unauthenticatedProbe() : authenticatedProbe();
  };
}

function authenticatedProbe(): Promise<Response> {
  return Promise.resolve(
    jsonResponse(
      {
        result: { account_id: ACCOUNT, public_id: PUBLIC_ID, client_module: "web-ui" },
        correlation_id: CORRELATION,
      },
      200,
    ),
  );
}

function establishedLogin(): Promise<Response> {
  return Promise.resolve(
    jsonResponse({ result: { authenticated: true }, correlation_id: CORRELATION }, 200),
  );
}

/** Reads the request target the application supplied, which is always a path. */
function requestTarget(input: unknown): string {
  return typeof input === "string" ? input : "";
}

/**
 * Routes the session probe and the login submission independently so a test can
 * assert exactly which requests the panel issued.
 */
function mockRoutedFetch(routes: {
  probe: () => Promise<Response>;
  login: () => Promise<Response>;
}) {
  return vi
    .spyOn(globalThis, "fetch")
    .mockImplementation((input: unknown) =>
      requestTarget(input) === "/api/v1/auth/login" ? routes.login() : routes.probe(),
    );
}

function continuationLogin(stage: string): () => Promise<Response> {
  return () =>
    Promise.resolve(
      jsonResponse(
        { result: { mfa: stage, continuation: CONTINUATION }, correlation_id: CORRELATION },
        202,
      ),
    );
}

function openedEnrollment(): Promise<Response> {
  return Promise.resolve(
    jsonResponse(
      {
        result: { secret: SECRET, provisioning_uri: PROVISIONING_URI, enrollment: ENROLLMENT },
        correlation_id: CORRELATION,
      },
      200,
    ),
  );
}

function refusal(): Promise<Response> {
  return Promise.resolve(
    jsonResponse({ error: "authentication_failed", correlation_id: CORRELATION }, 401),
  );
}

function gatewayTimeout(): Promise<Response> {
  return Promise.resolve(
    jsonResponse({ error: "gateway_timeout", correlation_id: CORRELATION }, 504),
  );
}

/**
 * Answers each route from its own handler and fails an unrouted request.
 *
 * Every request the panel makes is therefore either asserted here or a visible
 * failure, so a step that quietly called a route this scenario did not expect
 * cannot pass.
 */
function mockRoutes(routes: Readonly<Record<string, () => Promise<Response>>>) {
  return vi.spyOn(globalThis, "fetch").mockImplementation((input: unknown) => {
    const handler = routes[requestTarget(input)];
    return handler === undefined
      ? Promise.reject(new Error(`unrouted request: ${requestTarget(input)}`))
      : handler();
  });
}

function requestsTo(fetchMock: ReturnType<typeof mockRoutes>, path: string): RequestInit[] {
  return fetchMock.mock.calls
    .filter(([target]) => requestTarget(target) === path)
    .map(([, init]) => init!);
}

function panel(): HTMLElement {
  const section = document.querySelector("section[data-authentication-state]");
  if (section === null) {
    throw new Error("the sign-in panel is not rendered");
  }
  return section as HTMLElement;
}

function usernameField(): HTMLInputElement {
  return screen.getByLabelText("Username");
}

function passwordField(): HTMLInputElement {
  return screen.getByLabelText("Password");
}

interface ReactHookNode {
  memoizedState: unknown;
  next: ReactHookNode | null;
}

interface ReactFiberNode {
  alternate: ReactFiberNode | null;
  memoizedState: ReactHookNode | null;
  return: ReactFiberNode | null;
  type: unknown;
}

function hookValue(fiber: ReactFiberNode, index: number): unknown {
  let hook = fiber.memoizedState;
  for (let currentIndex = 0; currentIndex < index; currentIndex += 1) {
    if (hook === null) {
      throw new Error("LoginPanel state hook is unavailable");
    }
    hook = hook.next;
  }
  if (hook === null) {
    throw new Error("LoginPanel state hook is unavailable");
  }
  return hook.memoizedState;
}

/**
 * Reads controlled state after a factor transition unmounts the password
 * input. The rendered authentication state selects the current Fiber rather
 * than its previous alternate.
 */
function passwordState(): unknown {
  const loginPanel = panel();
  const fiberKey = Object.getOwnPropertyNames(loginPanel).find((key) =>
    key.startsWith("__reactFiber$"),
  );
  if (fiberKey === undefined) {
    throw new Error("LoginPanel Fiber is unavailable");
  }

  const hostFiber = (loginPanel as unknown as Record<string, ReactFiberNode | undefined>)[fiberKey];
  if (hostFiber === undefined) {
    throw new Error("LoginPanel Fiber is unavailable");
  }

  let fiber: ReactFiberNode | null = hostFiber;
  while (fiber !== null && fiber.type !== LoginPanel) {
    fiber = fiber.return;
  }
  if (fiber === null) {
    throw new Error("LoginPanel Fiber is unavailable");
  }

  const renderedState = loginPanel.dataset.authenticationState;
  const currentFiber = [fiber, fiber.alternate].find(
    (candidate) => candidate !== null && hookValue(candidate, 5) === renderedState,
  );
  if (currentFiber === undefined || currentFiber === null) {
    throw new Error("LoginPanel current Fiber is unavailable");
  }
  return hookValue(currentFiber, 1);
}

function submitButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: "Sign in" });
}

function checkAgainButton(): HTMLButtonElement {
  return screen.getByRole<HTMLButtonElement>("button", { name: "Check again" });
}

async function renderUnauthenticated(login: () => Promise<Response>) {
  const fetchMock = mockRoutedFetch({ probe: unauthenticatedProbe, login });
  render(<LoginPanel />);
  await waitFor(() => {
    expect(panel().dataset.authenticationState).toBe("unauthenticated");
  });
  return fetchMock;
}

async function renderUnauthenticatedThenAuthenticated(login: () => Promise<Response>) {
  let probes = 0;
  const fetchMock = mockRoutedFetch({
    probe: () => {
      probes += 1;
      return probes === 1 ? unauthenticatedProbe() : authenticatedProbe();
    },
    login,
  });
  render(<LoginPanel />);
  await waitFor(() => {
    expect(panel().dataset.authenticationState).toBe("unauthenticated");
  });
  return fetchMock;
}

function fillCredentials(): void {
  fireEvent.change(usernameField(), { target: { value: USERNAME } });
  fireEvent.change(passwordField(), { target: { value: PASSWORD } });
}

function codeField(): HTMLInputElement {
  return screen.getByLabelText("Authentication code");
}

function setupKeyField(): HTMLInputElement {
  return screen.getByLabelText("Setup key");
}

function setupLinkField(): HTMLInputElement {
  return screen.getByLabelText("Setup link");
}

/**
 * Renders the panel against exactly the supplied routes and signs in once.
 *
 * The session probe is always routed, because the panel renders nothing until
 * it settles; every other route belongs to the scenario under test.
 */
async function signInWith(routes: Readonly<Record<string, () => Promise<Response>>>) {
  let probes = 0;
  const session =
    routes[SESSION_PATH] ??
    (() => {
      probes += 1;
      return probes === 1 ? unauthenticatedProbe() : authenticatedProbe();
    });
  const fetchMock = mockRoutes({ ...routes, [SESSION_PATH]: session });
  render(<LoginPanel />);
  await waitFor(() => {
    expect(panel().dataset.authenticationState).toBe("unauthenticated");
  });
  fillCredentials();
  fireEvent.click(submitButton());
  return fetchMock;
}

async function reachState(state: string): Promise<void> {
  await waitFor(() => {
    expect(panel().dataset.authenticationState).toBe(state);
  });
}

afterEach(() => {
  vi.useRealTimers();
  Reflect.deleteProperty(globalThis.document, "cookie");
});

describe("LoginPanel surface presence", () => {
  it("renders nothing while the session probe is in flight", () => {
    vi.spyOn(globalThis, "fetch").mockReturnValue(new Promise<Response>(() => {}));

    const { container } = render(<LoginPanel />);

    expect(container.innerHTML).toBe("");
  });

  it("renders nothing when the deployment serves no authentication surface", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(jsonResponse({ error: "not_found" }, 404));

    const { container } = render(<LoginPanel />);

    await waitFor(() => {
      expect(globalThis.fetch).toHaveBeenCalledTimes(1);
    });
    await waitFor(() => {
      expect(container.innerHTML).toBe("");
    });
  });

  it("renders the authenticated state when the presented session already authenticates", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(authenticatedProbe);

    render(<LoginPanel />);

    await waitFor(() => {
      expect(panel().dataset.authenticationState).toBe("authenticated");
    });
    expect(screen.getByRole("status").textContent).toBe("You are signed in.");
    expect(screen.queryByLabelText("Password")).toBeNull();
  });
});

describe("LoginPanel form", () => {
  it("offers a username field and a masked password field", async () => {
    await renderUnauthenticated(establishedLogin);

    expect(usernameField().type).toBe("text");
    expect(passwordField().type).toBe("password");
    expect(passwordField().getAttribute("spellcheck")).toBe("false");
    expect(passwordField().getAttribute("autocomplete")).toBe("current-password");
  });

  it("keeps the action disabled until both fields are supplied", async () => {
    await renderUnauthenticated(establishedLogin);

    expect(submitButton().disabled).toBe(true);
    fireEvent.change(usernameField(), { target: { value: USERNAME } });
    expect(submitButton().disabled).toBe(true);
    fireEvent.change(passwordField(), { target: { value: PASSWORD } });
    expect(submitButton().disabled).toBe(false);
  });

  it("disables the control while a submission is in flight and submits once", async () => {
    const fetchMock = await renderUnauthenticated(() => new Promise<Response>(() => {}));

    fillCredentials();
    fireEvent.click(submitButton());

    await waitFor(() => {
      expect(panel().dataset.authenticationState).toBe("submitting");
    });
    expect(submitButton().disabled).toBe(true);
    fireEvent.click(submitButton());

    const loginCalls = fetchMock.mock.calls.filter(
      ([target]) => requestTarget(target) === "/api/v1/auth/login",
    );
    expect(loginCalls).toHaveLength(1);
  });

  it("reaches the authenticated state and discards the password on success", async () => {
    await renderUnauthenticatedThenAuthenticated(establishedLogin);

    fillCredentials();
    fireEvent.click(submitButton());

    await waitFor(() => {
      expect(panel().dataset.authenticationState).toBe("authenticated");
    });
    expect(screen.getByRole("status").textContent).toBe("You are signed in.");
    expect(document.body.textContent).not.toContain(PASSWORD);
    expect(document.body.innerHTML).not.toContain(PASSWORD);
  });
});

describe("LoginPanel failure presentation", () => {
  it("reconciles a login timeout without resubmitting credentials", async () => {
    let probes = 0;
    const fetchMock = mockRoutes({
      [SESSION_PATH]: () => {
        probes += 1;
        return probes <= 3 ? unauthenticatedProbe() : authenticatedProbe();
      },
      [LOGIN_PATH]: gatewayTimeout,
    });
    render(<LoginPanel />);
    await reachState("unauthenticated");
    fillCredentials();
    vi.useFakeTimers();
    fireEvent.click(submitButton());
    await act(async () => {});

    expect(panel().dataset.authenticationState).toBe("indeterminate");
    expect(document.body.innerHTML).not.toContain(PASSWORD);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });

    expect(panel().dataset.authenticationState).toBe("authenticated");
    expect(requestsTo(fetchMock, LOGIN_PATH)).toHaveLength(1);
    expect(requestsTo(fetchMock, SESSION_PATH)).toHaveLength(4);
  });

  it.each([
    [
      "a denied credential",
      () =>
        Promise.resolve(
          jsonResponse({ error: "authentication_failed", correlation_id: CORRELATION }, 401),
        ),
    ],
    [
      "a malformed submission",
      () =>
        Promise.resolve(jsonResponse({ error: "bad_request", correlation_id: CORRELATION }, 400)),
    ],
    [
      "a denied origin",
      () =>
        Promise.resolve(
          jsonResponse({ error: "request_origin_denied", correlation_id: CORRELATION }, 403),
        ),
    ],
    [
      "an unavailable service",
      () =>
        Promise.resolve(
          jsonResponse({ error: "service_unavailable", correlation_id: CORRELATION }, 503),
        ),
    ],
  ])("presents %s identically and discards the password", async (_label, login) => {
    await renderUnauthenticated(login);

    fillCredentials();
    fireEvent.click(submitButton());

    await waitFor(() => {
      expect(panel().dataset.authenticationState).toBe("failed");
    });
    expect(screen.getByRole("alert").textContent).toBe("Sign-in failed.");
    expect(passwordField().value).toBe("");
    // Neither the rejection code nor the transport diagnostic reaches the page.
    expect(document.body.innerHTML).not.toContain("authentication_failed");
    expect(document.body.innerHTML).not.toContain("bad_request");
    expect(document.body.innerHTML).not.toContain("request_origin_denied");
    expect(document.body.innerHTML).not.toContain("service_unavailable");
    expect(document.body.innerHTML).not.toContain("ECONNREFUSED");
    expect(document.body.innerHTML).not.toContain(CORRELATION);
    expect(document.body.innerHTML).not.toContain(PASSWORD);
  });

  it("keeps a login transport failure indeterminate without rendering its diagnostic", async () => {
    const fetchMock = await renderUnauthenticated(() =>
      Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443")),
    );

    fillCredentials();
    fireEvent.click(submitButton());
    await reachState("indeterminate");

    expect(screen.getByRole("status").textContent).toBe("Checking whether your sign-in completed.");
    expect(document.body.innerHTML).not.toContain("ECONNREFUSED");
    expect(document.body.innerHTML).not.toContain(PASSWORD);
    expect(requestsTo(fetchMock, LOGIN_PATH)).toHaveLength(1);
  });

  it("renders the same markup for two different denials", async () => {
    const denial = (code: string, status: number) => () =>
      Promise.resolve(jsonResponse({ error: code, correlation_id: CORRELATION }, status));

    await renderUnauthenticated(denial("authentication_failed", 401));
    fillCredentials();
    fireEvent.click(submitButton());
    await waitFor(() => {
      expect(panel().dataset.authenticationState).toBe("failed");
    });
    const first = panel().innerHTML;

    cleanup();
    await renderUnauthenticated(denial("service_unavailable", 503));
    fillCredentials();
    fireEvent.click(submitButton());
    await waitFor(() => {
      expect(panel().dataset.authenticationState).toBe("failed");
    });

    expect(panel().innerHTML).toBe(first);
  });
});

describe("LoginPanel storage discipline", () => {
  it("writes no credential or token to browser storage", async () => {
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    // A sanity check that these assertions can fail: spy on Storage to verify
    // the mechanism works, then assert no secrets are ever stored.
    const setItemSpy = vi.spyOn(Storage.prototype, "setItem");
    globalThis.sessionStorage.setItem("sanity-check", "safe-value");
    expect(setItemSpy).toHaveBeenCalledWith("sanity-check", "safe-value");
    expect(globalThis.sessionStorage.length).toBe(1);
    globalThis.sessionStorage.clear();
    setItemSpy.mockClear();

    await renderUnauthenticatedThenAuthenticated(establishedLogin);

    fillCredentials();
    fireEvent.click(submitButton());
    await waitFor(() => {
      expect(panel().dataset.authenticationState).toBe("authenticated");
    });

    expect(globalThis.localStorage.length).toBe(0);
    expect(globalThis.sessionStorage.length).toBe(0);
    expect(globalThis.document.cookie).not.toContain(PASSWORD);
    expect(globalThis.location.href).not.toContain(PASSWORD);
    // Assert that setItem was never called during the login workflow
    expect(setItemSpy).not.toHaveBeenCalled();
    setItemSpy.mockRestore();
  });
});

describe("LoginPanel credential disposal", () => {
  it.each([
    ["second-factor", { [LOGIN_PATH]: continuationLogin("mfa_required") }, "second-factor"],
    [
      "enrollment",
      {
        [LOGIN_PATH]: continuationLogin("mfa_enrollment_required"),
        [MFA_ENROLLMENT_PATH]: openedEnrollment,
      },
      "enrollment",
    ],
  ])("clears the password state on the %s transition", async (_transition, routes, state) => {
    await signInWith(routes);

    // This makes the state assertion below a real observation of the entered
    // credential, rather than an assertion against the component's empty
    // initial value.
    expect(passwordField().value).toBe(PASSWORD);
    await reachState(state);
    expect(passwordState()).toBe("");
  });
});

describe("LoginPanel second factor", () => {
  it("waits through delayed negative probes for a session that commits after MFA verification", async () => {
    let probes = 0;
    const fetchMock = await signInWith({
      [SESSION_PATH]: () => {
        probes += 1;
        return probes === 1 || probes === 2 || probes === 3
          ? unauthenticatedProbe()
          : authenticatedProbe();
      },
      [LOGIN_PATH]: continuationLogin("mfa_required"),
      [MFA_VERIFY_PATH]: gatewayTimeout,
    });

    await reachState("second-factor");
    vi.useFakeTimers();
    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Verify code" }));
    await act(async () => {});

    expect(panel().dataset.authenticationState).toBe("indeterminate");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });

    expect(panel().dataset.authenticationState).toBe("authenticated");
    expect(requestsTo(fetchMock, MFA_VERIFY_PATH)).toHaveLength(1);
    expect(requestsTo(fetchMock, SESSION_PATH)).toHaveLength(4);
  });

  it("presents the code step after a 202 mfa_required and completes the login", async () => {
    const fetchMock = await signInWith({
      [LOGIN_PATH]: continuationLogin("mfa_required"),
      [MFA_VERIFY_PATH]: establishedLogin,
    });

    await reachState("second-factor");
    // The credential controls are gone, so the step cannot be confused with a
    // second sign-in attempt.
    expect(screen.queryByLabelText("Username")).toBeNull();
    expect(screen.queryByLabelText("Password")).toBeNull();
    expect(codeField().getAttribute("autocomplete")).toBe("one-time-code");
    expect(codeField().getAttribute("inputmode")).toBe("numeric");
    expect(codeField().maxLength).toBe(6);

    const verify = screen.getByRole("button", { name: "Verify code" });
    expect(verify.hasAttribute("disabled")).toBe(true);
    fireEvent.change(codeField(), { target: { value: "12345" } });
    expect(verify.hasAttribute("disabled")).toBe(true);
    fireEvent.change(codeField(), { target: { value: CODE } });
    expect(verify.hasAttribute("disabled")).toBe(false);

    fireEvent.click(verify);
    await reachState("authenticated");
    expect(screen.getByRole("status").textContent).toBe("You are signed in.");

    const submitted = requestsTo(fetchMock, MFA_VERIFY_PATH);
    expect(submitted).toHaveLength(1);
    expect(submitted[0]?.body).toBe(JSON.stringify({ continuation: CONTINUATION, code: CODE }));
    // The continuation travelled in the body of that one request and is not
    // rendered, kept in the target, or written anywhere else.
    expect(document.body.innerHTML).not.toContain(CONTINUATION);
    expect(globalThis.location.href).not.toContain(CONTINUATION);
  });

  it("refuses to submit a code the routes would reject as malformed", async () => {
    const fetchMock = await signInWith({
      [LOGIN_PATH]: continuationLogin("mfa_required"),
      [MFA_VERIFY_PATH]: establishedLogin,
    });

    await reachState("second-factor");
    const verify = screen.getByRole("button", { name: "Verify code" });
    for (const rejected of ["", "1234", "1234567", "12345a", "  1234"]) {
      fireEvent.change(codeField(), { target: { value: rejected } });
      expect(verify.hasAttribute("disabled")).toBe(true);
      fireEvent.click(verify);
    }

    expect(requestsTo(fetchMock, MFA_VERIFY_PATH)).toHaveLength(0);
  });

  it("ends the attempt when the submitted code is refused", async () => {
    const fetchMock = await signInWith({
      [SESSION_PATH]: initialUnauthenticatedProbeOnly(),
      [LOGIN_PATH]: continuationLogin("mfa_required"),
      [MFA_VERIFY_PATH]: refusal,
    });

    await reachState("second-factor");
    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Verify code" }));

    await reachState("attempt-ended");
    // The continuation is spent, so the panel states the attempt is over and
    // offers the only thing that can follow it: a fresh sign-in.
    expect(screen.getByRole("alert").textContent).toBe(ATTEMPT_ENDED_MESSAGE);
    expect(screen.queryByLabelText("Authentication code")).toBeNull();
    expect(usernameField().value).toBe(USERNAME);
    expect(passwordField().value).toBe("");
    expect(submitButton().disabled).toBe(true);
    expect(requestsTo(fetchMock, MFA_VERIFY_PATH)).toHaveLength(1);
    expect(requestsTo(fetchMock, SESSION_PATH)).toHaveLength(1);
    expect(document.body.innerHTML).not.toContain(CONTINUATION);
    expect(document.body.innerHTML).not.toContain(CODE);
    expect(document.body.innerHTML).not.toContain("authentication_failed");
    expect(document.body.innerHTML).not.toContain(CORRELATION);
  });

  it.each([
    [
      "an unreadable success response",
      () => Promise.resolve(new Response("not json", { status: 200 })),
    ],
    ["a malformed success response", () => Promise.resolve(jsonResponse({ result: {} }, 200))],
  ])("reconciles %s through an authenticated session probe", async (_label, verify) => {
    let probes = 0;
    const fetchMock = await signInWith({
      [SESSION_PATH]: () => {
        probes += 1;
        return probes === 1 ? unauthenticatedProbe() : authenticatedProbe();
      },
      [LOGIN_PATH]: continuationLogin("mfa_required"),
      [MFA_VERIFY_PATH]: verify,
    });

    await reachState("second-factor");
    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Verify code" }));

    await reachState("authenticated");
    expect(requestsTo(fetchMock, MFA_VERIFY_PATH)).toHaveLength(1);
    expect(requestsTo(fetchMock, SESSION_PATH)).toHaveLength(2);
  });

  it("keeps an unresolved second-factor attempt indeterminate and rechecks without resubmitting", async () => {
    let probes = 0;
    const fetchMock = await signInWith({
      [SESSION_PATH]: () => {
        probes += 1;
        return probes <= 4 ? unauthenticatedProbe() : authenticatedProbe();
      },
      [LOGIN_PATH]: continuationLogin("mfa_required"),
      [MFA_VERIFY_PATH]: () => Promise.reject(new Error("connection reset")),
    });

    await reachState("second-factor");
    vi.useFakeTimers();
    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Verify code" }));
    await act(async () => {});
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });

    expect(panel().dataset.authenticationState).toBe("indeterminate");
    expect(checkAgainButton().disabled).toBe(false);
    expect(screen.queryByLabelText("Authentication code")).toBeNull();
    fireEvent.click(checkAgainButton());
    await act(async () => {});

    expect(panel().dataset.authenticationState).toBe("authenticated");
    expect(requestsTo(fetchMock, MFA_VERIFY_PATH)).toHaveLength(1);
    expect(requestsTo(fetchMock, SESSION_PATH)).toHaveLength(5);
  });

  it("keeps an absent second-factor reconciliation visibly indeterminate", async () => {
    let probes = 0;
    const fetchMock = await signInWith({
      [SESSION_PATH]: () => {
        probes += 1;
        return probes === 1
          ? unauthenticatedProbe()
          : Promise.resolve(jsonResponse({ error: "not_found" }, 404));
      },
      [LOGIN_PATH]: continuationLogin("mfa_required"),
      [MFA_VERIFY_PATH]: () => Promise.reject(new Error("connection reset")),
    });

    await reachState("second-factor");
    vi.useFakeTimers();
    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Verify code" }));
    await act(async () => {});
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });

    expect(panel().dataset.authenticationState).toBe("indeterminate");
    expect(checkAgainButton().disabled).toBe(false);
    expect(requestsTo(fetchMock, MFA_VERIFY_PATH)).toHaveLength(1);
    expect(requestsTo(fetchMock, SESSION_PATH)).toHaveLength(4);
  });
});

describe("LoginPanel enrollment", () => {
  it("discloses the setup key and link exactly once and completes the login", async () => {
    const fetchMock = await signInWith({
      [LOGIN_PATH]: continuationLogin("mfa_enrollment_required"),
      [MFA_ENROLLMENT_PATH]: openedEnrollment,
      [MFA_CONFIRM_PATH]: establishedLogin,
    });

    await reachState("enrollment");
    expect(setupKeyField().value).toBe(SECRET);
    expect(setupKeyField().readOnly).toBe(true);
    expect(setupLinkField().value).toBe(PROVISIONING_URI);
    expect(setupLinkField().readOnly).toBe(true);
    // The single disclosure is stated as such, so the operator knows the values
    // cannot be recovered after this step.
    expect(screen.getByRole("alert").textContent).toContain("shown once");
    // Nothing renders the disclosed values as a navigation target.
    expect(document.querySelectorAll("a")).toHaveLength(0);

    const opened = requestsTo(fetchMock, MFA_ENROLLMENT_PATH);
    expect(opened, "the disclosure was requested exactly once").toHaveLength(1);
    expect(opened[0]?.body).toBe(JSON.stringify({ continuation: CONTINUATION }));

    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm authenticator app" }));
    await reachState("authenticated");

    const confirmed = requestsTo(fetchMock, MFA_CONFIRM_PATH);
    expect(confirmed).toHaveLength(1);
    expect(confirmed[0]?.body).toBe(JSON.stringify({ enrollment: ENROLLMENT, code: CODE }));
    // The disclosure is still requested exactly once, and nothing it carried
    // outlives the enrollment it belonged to.
    expect(requestsTo(fetchMock, MFA_ENROLLMENT_PATH)).toHaveLength(1);
    expect(screen.queryByLabelText("Setup key")).toBeNull();
    expect(screen.queryByLabelText("Setup link")).toBeNull();
    expect(document.body.innerHTML).not.toContain(SECRET);
    expect(document.body.innerHTML).not.toContain(PROVISIONING_URI);
    expect(document.body.innerHTML).not.toContain(ENROLLMENT);
  });

  it("ends the attempt when the enrollment confirmation is refused", async () => {
    const fetchMock = await signInWith({
      [SESSION_PATH]: initialUnauthenticatedProbeOnly(),
      [LOGIN_PATH]: continuationLogin("mfa_enrollment_required"),
      [MFA_ENROLLMENT_PATH]: openedEnrollment,
      [MFA_CONFIRM_PATH]: refusal,
    });

    await reachState("enrollment");
    expect(setupKeyField().value).toBe(SECRET);
    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm authenticator app" }));

    await reachState("attempt-ended");
    expect(screen.getByRole("alert").textContent).toBe(ATTEMPT_ENDED_MESSAGE);
    expect(requestsTo(fetchMock, MFA_CONFIRM_PATH)).toHaveLength(1);
    expect(requestsTo(fetchMock, SESSION_PATH)).toHaveLength(1);
    // The refused enrollment is gone: the secret it disclosed is not held for a
    // retry, because the Server has already spent the value that confirms it.
    expect(screen.queryByLabelText("Setup key")).toBeNull();
    expect(document.body.innerHTML).not.toContain(SECRET);
    expect(document.body.innerHTML).not.toContain(PROVISIONING_URI);
    expect(document.body.innerHTML).not.toContain(ENROLLMENT);
    expect(usernameField().value).toBe(USERNAME);
  });

  it.each([
    [
      "an unreadable success response",
      () => Promise.resolve(new Response("not json", { status: 200 })),
    ],
    ["a malformed success response", () => Promise.resolve(jsonResponse({ result: {} }, 200))],
  ])("reconciles %s through an authenticated session probe", async (_label, confirm) => {
    let probes = 0;
    const fetchMock = await signInWith({
      [SESSION_PATH]: () => {
        probes += 1;
        return probes === 1 ? unauthenticatedProbe() : authenticatedProbe();
      },
      [LOGIN_PATH]: continuationLogin("mfa_enrollment_required"),
      [MFA_ENROLLMENT_PATH]: openedEnrollment,
      [MFA_CONFIRM_PATH]: confirm,
    });

    await reachState("enrollment");
    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm authenticator app" }));

    await reachState("authenticated");
    expect(requestsTo(fetchMock, MFA_CONFIRM_PATH)).toHaveLength(1);
    expect(requestsTo(fetchMock, SESSION_PATH)).toHaveLength(2);
  });

  it("keeps an unresolved enrollment confirmation indeterminate", async () => {
    let probes = 0;
    const fetchMock = await signInWith({
      [SESSION_PATH]: () => {
        probes += 1;
        return probes === 1 ? unauthenticatedProbe() : unauthenticatedProbe();
      },
      [LOGIN_PATH]: continuationLogin("mfa_enrollment_required"),
      [MFA_ENROLLMENT_PATH]: openedEnrollment,
      [MFA_CONFIRM_PATH]: () => Promise.reject(new Error("connection reset")),
    });

    await reachState("enrollment");
    vi.useFakeTimers();
    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm authenticator app" }));
    await act(async () => {});
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30_000);
    });

    expect(panel().dataset.authenticationState).toBe("indeterminate");
    expect(checkAgainButton().disabled).toBe(false);
    expect(screen.queryByLabelText("Setup key")).toBeNull();
    expect(requestsTo(fetchMock, MFA_CONFIRM_PATH)).toHaveLength(1);
    expect(requestsTo(fetchMock, SESSION_PATH)).toHaveLength(4);
  });

  it("ends the attempt when the enrollment cannot be opened", async () => {
    const fetchMock = await signInWith({
      [LOGIN_PATH]: continuationLogin("mfa_enrollment_required"),
      [MFA_ENROLLMENT_PATH]: refusal,
    });

    await reachState("attempt-ended");
    expect(screen.getByRole("alert").textContent).toBe(ATTEMPT_ENDED_MESSAGE);
    expect(requestsTo(fetchMock, MFA_ENROLLMENT_PATH)).toHaveLength(1);
    expect(screen.queryByLabelText("Setup key")).toBeNull();
  });

  it("keeps the disclosed values out of every browser store and the URL", async () => {
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    // A sanity check that these assertions can fail against a real store.
    // Use spy to verify storage works without storing secrets.
    const setItemSpy = vi.spyOn(Storage.prototype, "setItem");
    globalThis.sessionStorage.setItem("sanity-check", "safe-value");
    expect(setItemSpy).toHaveBeenCalledWith("sanity-check", "safe-value");
    expect(globalThis.sessionStorage.length).toBe(1);
    globalThis.sessionStorage.clear();
    setItemSpy.mockClear();

    await signInWith({
      [LOGIN_PATH]: continuationLogin("mfa_enrollment_required"),
      [MFA_ENROLLMENT_PATH]: openedEnrollment,
      [MFA_CONFIRM_PATH]: establishedLogin,
    });

    await reachState("enrollment");
    // Asserted while the values are actually on screen, so this is an
    // observation about a disclosure that really happened.
    expect(setupKeyField().value).toBe(SECRET);
    for (const disclosed of [SECRET, PROVISIONING_URI, ENROLLMENT, CONTINUATION]) {
      expect(globalThis.localStorage.getItem("weavelit")).toBeNull();
      expect(JSON.stringify(globalThis.localStorage)).not.toContain(disclosed);
      expect(JSON.stringify(globalThis.sessionStorage)).not.toContain(disclosed);
      expect(globalThis.document.cookie).not.toContain(disclosed);
      expect(globalThis.location.href).not.toContain(disclosed);
    }
    expect(globalThis.localStorage.length).toBe(0);
    expect(globalThis.sessionStorage.length).toBe(0);
    // Assert that setItem was never called during the enrollment workflow
    expect(setItemSpy).not.toHaveBeenCalled();

    fireEvent.change(codeField(), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm authenticator app" }));
    await reachState("authenticated");

    expect(globalThis.localStorage.length).toBe(0);
    expect(globalThis.sessionStorage.length).toBe(0);
    for (const disclosed of [SECRET, PROVISIONING_URI, ENROLLMENT, CONTINUATION]) {
      expect(globalThis.document.cookie).not.toContain(disclosed);
      expect(globalThis.location.href).not.toContain(disclosed);
      expect(document.body.innerHTML).not.toContain(disclosed);
    }
    // Assert that setItem was never called with secrets
    expect(setItemSpy).not.toHaveBeenCalled();
    setItemSpy.mockRestore();
  });
});
