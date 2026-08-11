import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { LoginPanel } from "./weavelit-login-form";

const CORRELATION = "0123456789abcdef0123456789abcdef";
const ACCOUNT = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const USERNAME = "administrator";
const PASSWORD = "fixture-administrator-password";

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

function authenticatedProbe(): Promise<Response> {
  return Promise.resolve(
    jsonResponse(
      { result: { account_id: ACCOUNT, client_module: "web-ui" }, correlation_id: CORRELATION },
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

function submitButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: "Sign in" });
}

async function renderUnauthenticated(login: () => Promise<Response>) {
  const fetchMock = mockRoutedFetch({ probe: unauthenticatedProbe, login });
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

afterEach(() => {
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
    await renderUnauthenticated(establishedLogin);

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
    ["a transport failure", () => Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443"))],
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
    // A sanity check that these assertions can fail: a written key is visible
    // here, so an empty jar afterwards is a real observation rather than a
    // storage object that never records anything.
    globalThis.sessionStorage.setItem("probe", PASSWORD);
    expect(globalThis.sessionStorage.length).toBe(1);
    globalThis.sessionStorage.clear();

    await renderUnauthenticated(establishedLogin);

    fillCredentials();
    fireEvent.click(submitButton());
    await waitFor(() => {
      expect(panel().dataset.authenticationState).toBe("authenticated");
    });

    expect(globalThis.localStorage.length).toBe(0);
    expect(globalThis.sessionStorage.length).toBe(0);
    expect(globalThis.document.cookie).not.toContain(PASSWORD);
    expect(globalThis.location.href).not.toContain(PASSWORD);
  });
});
