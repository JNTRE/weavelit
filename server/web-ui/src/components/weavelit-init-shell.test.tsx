import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentType } from "react";
import { describe, expect, it, vi } from "vitest";

import { ACCOUNTS_MFA_REQUIREMENT_PATH, MFA_POLICY_STEP_UP_PATH } from "../api/weavelit-mfa-policy";
import { ApplicationShell } from "./weavelit-init-shell";

const AUTH_CORRELATION = "authentication-correlation";

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function statusRegion(): HTMLElement {
  return screen.getByRole("status");
}

function selectionButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: "Select SQLite" });
}

/**
 * Routes the status request and the selection request independently so a test
 * can assert exactly which requests the shell issued.
 */
function mockRoutedFetch(routes: {
  status: () => Promise<Response>;
  selection: () => Promise<Response>;
}) {
  return vi
    .spyOn(globalThis, "fetch")
    .mockImplementation((_input: unknown, init?: RequestInit) =>
      init?.method === "PUT" ? routes.selection() : routes.status(),
    );
}

function unselectedStatus(): Promise<Response> {
  return Promise.resolve(jsonResponse({ lifecycle: "uninitialized", database_selected: false }));
}

function selectedStatus(): Promise<Response> {
  return Promise.resolve(jsonResponse({ lifecycle: "uninitialized", database_selected: true }));
}

/** Waits for the status projection, then takes one first-launch path. */
async function choose(path: "init" | "restore"): Promise<void> {
  await waitFor(() => {
    expect(statusRegion().dataset.statusState).toBe("available");
  });
  fireEvent.click(
    screen.getByRole("button", {
      name: path === "init" ? "Set up a new deployment" : "Restore from a backup",
    }),
  );
}

describe("ApplicationShell", () => {
  it("renders the loading state while the status request is pending", () => {
    vi.spyOn(globalThis, "fetch").mockReturnValue(new Promise<Response>(() => {}));

    render(<ApplicationShell />);

    expect(statusRegion().dataset.statusState).toBe("loading");
    expect(statusRegion().textContent).toBe("Checking the deployment status.");
  });

  it("renders the selected state when an Application Database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ lifecycle: "uninitialized", database_selected: true }),
    );

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("available");
    });
    expect(statusRegion().textContent).toBe(
      "An Application Database is selected for this deployment.",
    );
  });

  it("renders the unselected state when no Application Database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ lifecycle: "uninitialized", database_selected: false }),
    );

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("available");
    });
    expect(statusRegion().textContent).toBe(
      "No Application Database is selected for this deployment.",
    );
  });

  it("renders the unavailable state when the status request fails", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("ECONNREFUSED 127.0.0.1:8443"));

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("unavailable");
    });
    expect(statusRegion().textContent).toBe("The deployment status is unavailable.");
  });

  it("renders the unavailable state without leaking a malformed payload", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ lifecycle: "uninitialized", database_selected: "yes", detail: "secret" }),
    );

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("unavailable");
    });
    expect(statusRegion().textContent).toBe("The deployment status is unavailable.");
    expect(document.body.textContent).not.toContain("secret");
    expect(document.body.textContent).not.toContain("yes");
  });
});

describe("ApplicationShell first-launch choice", () => {
  function choiceSection(): HTMLElement | null {
    return document.querySelector("section.shell__choice");
  }

  it("offers exactly the two first-launch paths once the status projection arrives", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => unselectedStatus());

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(choiceSection()).not.toBeNull();
    });
    expect(screen.getByRole("heading", { name: "First launch" })).toBeTruthy();
    expect(screen.queryAllByRole("button")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Set up a new deployment" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Restore from a backup" })).toBeTruthy();
  });

  it("offers no first-launch choice while the status request is pending", () => {
    vi.spyOn(globalThis, "fetch").mockReturnValue(new Promise<Response>(() => {}));

    render(<ApplicationShell />);

    expect(choiceSection()).toBeNull();
  });

  it("offers no first-launch choice when the status projection is no longer served", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("ECONNREFUSED 127.0.0.1:8443"));

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("unavailable");
    });
    expect(choiceSection()).toBeNull();
  });

  it.each([
    ["init", "section.shell__init", "section.shell__restore"],
    ["restore", "section.shell__restore", "section.shell__init"],
  ] as const)("withdraws the other path once %s is chosen", async (path, chosen, withdrawn) => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => selectedStatus());

    render(<ApplicationShell />);
    await choose(path);

    await waitFor(() => {
      expect(document.querySelector(chosen)).not.toBeNull();
    });
    expect(document.querySelector(withdrawn)).toBeNull();
    expect(choiceSection()).toBeNull();
  });

  it("reuses the existing selection surface for either path rather than duplicating it", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => unselectedStatus());

    render(<ApplicationShell />);
    await choose("init");

    expect(document.querySelectorAll("section.shell__selection")).toHaveLength(1);
    expect(document.querySelector("section.shell__init")).toBeNull();
  });
});

describe("ApplicationShell database selection control", () => {
  it("offers the selection control only when no Application Database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => unselectedStatus());

    render(<ApplicationShell />);

    expect(screen.queryAllByRole("button")).toHaveLength(0);
    await choose("init");
    expect(selectionButton().disabled).toBe(false);
    expect(screen.getByRole("heading", { name: "Application Database" })).toBeTruthy();
  });

  it("does not offer the selection control once a database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ lifecycle: "uninitialized", database_selected: true }),
    );

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("available");
    });
    expect(screen.queryByRole("button", { name: "Select SQLite" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "Application Database" })).toBeNull();
  });

  it("does not offer the selection control when the status is unavailable", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("ECONNREFUSED 127.0.0.1:8443"));

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("unavailable");
    });
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("disables the control while a submission is in flight and submits once", async () => {
    const fetchMock = mockRoutedFetch({
      status: unselectedStatus,
      selection: () => new Promise<Response>(() => {}),
    });

    render(<ApplicationShell />);
    await choose("init");

    fireEvent.click(selectionButton());

    await waitFor(() => {
      expect(selectionButton().disabled).toBe(true);
    });
    fireEvent.click(selectionButton());

    const selectionCalls = fetchMock.mock.calls.filter(([, init]) => init?.method === "PUT");
    expect(selectionCalls).toHaveLength(1);
    expect(selectionCalls[0]![0]).toBe("/api/v1/application-database");
    expect(
      screen.getByRole("button", { name: "Select SQLite" }).closest("section")?.dataset
        .selectionState,
    ).toBe("submitting");
  });

  it("applies the returned projection without issuing a second status request", async () => {
    const fetchMock = mockRoutedFetch({
      status: unselectedStatus,
      selection: () =>
        Promise.resolve(jsonResponse({ lifecycle: "uninitialized", database_selected: true })),
    });

    render(<ApplicationShell />);
    await choose("restore");
    expect(fetchMock).toHaveBeenCalledTimes(1);

    fireEvent.click(selectionButton());

    await waitFor(() => {
      expect(statusRegion().textContent).toBe(
        "An Application Database is selected for this deployment.",
      );
    });
    expect(screen.queryByRole("button", { name: "Select SQLite" })).toBeNull();

    // One status request on mount and one selection request: the success
    // projection is authoritative, so no status refetch is issued.
    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock.mock.calls.filter(([, init]) => init?.method === "PUT")).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([, init]) => init?.method === "GET")).toHaveLength(1);
  });

  it.each([
    [400, "bad_request"],
    [403, "request_origin_denied"],
    [405, "method_not_allowed"],
    [409, "database_selection_not_allowed"],
    [503, "service_unavailable"],
  ])("renders the fixed failure message without server detail on %i", async (status, code) => {
    mockRoutedFetch({
      status: unselectedStatus,
      selection: () => Promise.resolve(jsonResponse({ error: code }, status)),
    });

    render(<ApplicationShell />);
    await choose("init");

    fireEvent.click(selectionButton());

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("The Application Database was not selected. Try again.");
    expect(document.body.textContent).not.toContain(code);
    expect(document.body.textContent).not.toContain(String(status));
    expect(selectionButton().disabled).toBe(false);
    expect(statusRegion().textContent).toBe(
      "No Application Database is selected for this deployment.",
    );
  });

  it("renders the fixed failure message when the selection transport fails", async () => {
    mockRoutedFetch({
      status: unselectedStatus,
      selection: () => Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443")),
    });

    render(<ApplicationShell />);
    await choose("init");

    fireEvent.click(selectionButton());

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("The Application Database was not selected. Try again.");
    expect(document.body.textContent).not.toContain("ECONNREFUSED");
  });

  it("renders the fixed failure message when a success payload is malformed", async () => {
    mockRoutedFetch({
      status: unselectedStatus,
      selection: () =>
        Promise.resolve(
          jsonResponse({ lifecycle: "uninitialized", database_selected: "yes", detail: "secret" }),
        ),
    });

    render(<ApplicationShell />);
    await choose("init");

    fireEvent.click(selectionButton());

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toBe("The Application Database was not selected. Try again.");
    expect(document.body.textContent).not.toContain("secret");
    expect(statusRegion().textContent).toBe(
      "No Application Database is selected for this deployment.",
    );
  });
});

describe("ApplicationShell Restore form gating", () => {
  function restoreSection(): HTMLElement | null {
    return document.querySelector("section.shell__restore");
  }

  function loginSection(): HTMLElement | null {
    return document.querySelector("section[data-authentication-state]");
  }

  /**
   * The shell's own status region.
   *
   * It is addressed by class rather than by role, because a completed Restore
   * renders a live region of its own until the shell withdraws it.
   */
  function shellStatus(): HTMLElement {
    return document.querySelector<HTMLElement>("p.shell__status")!;
  }

  it("offers the Restore form once an Application Database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => selectedStatus());

    render(<ApplicationShell />);
    await choose("restore");

    await waitFor(() => {
      expect(restoreSection()).not.toBeNull();
    });
    expect(screen.getByRole("heading", { name: "Restore" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Restore backup" })).toBeTruthy();
  });

  it("does not offer the Restore form before a database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => unselectedStatus());

    render(<ApplicationShell />);
    await choose("restore");

    expect(restoreSection()).toBeNull();
  });

  it("does not offer the Restore form when the status is unavailable", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("ECONNREFUSED 127.0.0.1:8443"));

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("unavailable");
    });
    expect(restoreSection()).toBeNull();
  });

  it("offers the Restore form as soon as a selection succeeds", async () => {
    mockRoutedFetch({
      status: unselectedStatus,
      selection: () =>
        Promise.resolve(jsonResponse({ lifecycle: "uninitialized", database_selected: true })),
    });

    render(<ApplicationShell />);
    await choose("restore");
    expect(restoreSection()).toBeNull();

    fireEvent.click(selectionButton());

    await waitFor(() => {
      expect(restoreSection()).not.toBeNull();
    });
  });

  it("withdraws every setup control and offers sign-in once a Restore completes", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation((input: unknown, init?: RequestInit) => {
      if (input === "/api/v1/restore") {
        // The recovery-key route answers `202` with the one-time ticket.
        return Promise.resolve(
          jsonResponse(
            {
              result: {
                restore_ticket: "0123456789abcdefghijklmnopqrstuvwxyzABC-_",
                reconciliation_capability: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghi",
              },
            },
            202,
          ),
        );
      }
      if (input === "/api/v1/restore/artifact") {
        return Promise.resolve(jsonResponse({ result: { lifecycle: "initialized" } }));
      }
      if (typeof input === "string" && input.startsWith("/api/v1/auth/")) {
        return Promise.resolve(
          jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
        );
      }
      expect(init?.method).not.toBe("PUT");
      return selectedStatus();
    });

    render(<ApplicationShell />);
    await choose("restore");

    fireEvent.change(await screen.findByLabelText("Backup file"), {
      target: {
        files: [new File([new Uint8Array([0x57, 0x4c, 0x42, 0x4b])], "backup.wlitbackup")],
      },
    });
    fireEvent.change(screen.getByLabelText("Recovery key"), {
      target: { value: "AGE-SECRET-KEY-1EXAMPLEEXAMPLEEXAMPLEEXAMPLE" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Restore backup" }));

    // The page never re-reads status across the sealing boundary: completion is
    // adopted from the Restore completion response it already holds.
    await waitFor(() => {
      expect(shellStatus().dataset.statusState).toBe("initialized");
    });
    expect(shellStatus().textContent).toBe(
      "This deployment is initialized and now runs in normal operation.",
    );
    expect(restoreSection()).toBeNull();
    expect(document.querySelector("section.shell__init")).toBeNull();
    expect(document.querySelector("section.shell__selection")).toBeNull();
    expect(document.querySelector("section.shell__choice")).toBeNull();
    await waitFor(() => {
      expect(loginSection()?.dataset.authenticationState).toBe("unauthenticated");
    });

    const statusRequests = vi
      .mocked(globalThis.fetch)
      .mock.calls.filter(([target]) => target === "/api/v1/status");
    expect(statusRequests).toHaveLength(1);
  });
});

describe("ApplicationShell Init workflow gating", () => {
  function initSection(): HTMLElement | null {
    return document.querySelector("section.shell__init");
  }

  function loginSection(): HTMLElement | null {
    return document.querySelector("section[data-authentication-state]");
  }

  it("offers the Init workflow once an Application Database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => selectedStatus());

    render(<ApplicationShell />);
    await choose("init");

    await waitFor(() => {
      expect(initSection()).not.toBeNull();
    });
    expect(screen.getByRole("heading", { name: "Set up this deployment" })).toBeTruthy();
  });

  it("does not offer the Init workflow before a database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => unselectedStatus());

    render(<ApplicationShell />);
    await choose("init");

    // A recovery key cannot be prepared before the database it would initialize
    // has been selected, so the workflow that prepares it is not offered at all.
    expect(initSection()).toBeNull();
  });

  it("offers the Init workflow as soon as a selection succeeds", async () => {
    mockRoutedFetch({
      status: unselectedStatus,
      selection: () =>
        Promise.resolve(jsonResponse({ lifecycle: "uninitialized", database_selected: true })),
    });

    render(<ApplicationShell />);
    await choose("init");
    expect(initSection()).toBeNull();

    fireEvent.click(selectionButton());

    await waitFor(() => {
      expect(initSection()).not.toBeNull();
    });
  });

  it("withdraws every setup control and offers sign-in once Init completes", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation((input: unknown, init?: RequestInit) => {
      if (input === "/api/v1/init/recovery-key") {
        return Promise.resolve(
          jsonResponse({
            result: {
              recovery_key:
                "AGE-SECRET-KEY-1QQQSYQCYQ5RQWZQFPG9SCRGWPUGPZYSNZS23V9CCRYDPK8QARC0SWRYDWG",
              delivery_nonce: "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8",
              reconciliation_capability: "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghi",
            },
          }),
        );
      }
      if (input === "/api/v1/init") {
        return Promise.resolve(jsonResponse({ result: { lifecycle: "initialized" } }));
      }
      if (typeof input === "string" && input.startsWith("/api/v1/auth/")) {
        return Promise.resolve(
          jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
        );
      }
      expect(init?.method).not.toBe("PUT");
      return selectedStatus();
    });

    render(<ApplicationShell />);
    await choose("init");

    fireEvent.change(await screen.findByLabelText("System Log module"), {
      target: { value: "sqlite" },
    });
    fireEvent.change(screen.getByLabelText("Audit Log module"), { target: { value: "sqlite" } });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "administrator" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "a-password" } });
    fireEvent.click(screen.getByRole("button", { name: "Prepare recovery key" }));

    fireEvent.click(await screen.findByLabelText("I have saved this recovery key."));
    fireEvent.click(screen.getByRole("button", { name: "Continue to review" }));
    fireEvent.click(await screen.findByRole("button", { name: "Complete setup" }));

    // The page never re-reads status across the sealing boundary: completion is
    // adopted from the finalization response it already holds.
    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("initialized");
    });
    expect(statusRegion().textContent).toBe(
      "This deployment is initialized and now runs in normal operation.",
    );
    expect(initSection()).toBeNull();
    expect(document.querySelector("section.shell__selection")).toBeNull();
    expect(document.querySelector("section.shell__restore")).toBeNull();
    expect(document.querySelector("section.shell__choice")).toBeNull();
    await waitFor(() => {
      expect(loginSection()?.dataset.authenticationState).toBe("unauthenticated");
    });

    const statusRequests = vi
      .mocked(globalThis.fetch)
      .mock.calls.filter(([target]) => target === "/api/v1/status");
    expect(statusRequests).toHaveLength(1);
  });
});

describe("ApplicationShell sign-in panel gating", () => {
  function loginSection(): HTMLElement | null {
    return document.querySelector("section[data-authentication-state]");
  }

  /** Reports whether a recorded request target is an authentication route. */
  function authenticationTarget(input: unknown): boolean {
    return typeof input === "string" && input.startsWith("/api/v1/auth/");
  }

  function routedFetch(routes: {
    status: () => Promise<Response>;
    session: () => Promise<Response>;
  }) {
    return vi
      .spyOn(globalThis, "fetch")
      .mockImplementation((input: unknown) =>
        authenticationTarget(input) ? routes.session() : routes.status(),
      );
  }

  function sessionRejected(): Promise<Response> {
    return Promise.resolve(
      jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
    );
  }

  function mockAuthenticatedAdministrationFetch() {
    Object.defineProperty(globalThis.document, "cookie", {
      configurable: true,
      get: () => "__Host-weavelit_csrf=csrf-token",
    });
    return vi.spyOn(globalThis, "fetch").mockImplementation((target: unknown) => {
      if (target === "/api/v1/status") {
        return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
      }
      if (target === "/api/v1/auth/session") {
        return Promise.resolve(
          jsonResponse({
            result: {
              account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
              public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
              client_module: "web-ui",
              password_change_required: false,
            },
            correlation_id: AUTH_CORRELATION,
          }),
        );
      }
      if (target === "/api/v1/administration/accounts/list") {
        return Promise.resolve(
          jsonResponse({
            result: { items: [], next_cursor: null },
            correlation_id: "accounts-list-correlation",
          }),
        );
      }
      return Promise.reject(new Error("unexpected request"));
    });
  }

  it.each([
    ["no database is selected", () => unselectedStatus()],
    [
      "a database is selected",
      () => Promise.resolve(jsonResponse({ lifecycle: "uninitialized", database_selected: true })),
    ],
  ])(
    "issues no authentication request while the status projection is served (%s)",
    async (_label, status) => {
      const fetchMock = routedFetch({ status, session: sessionRejected });

      render(<ApplicationShell />);

      await waitFor(() => {
        expect(statusRegion().dataset.statusState).toBe("available");
      });
      expect(loginSection()).toBeNull();
      expect(fetchMock.mock.calls.filter(([target]) => authenticationTarget(target))).toHaveLength(
        0,
      );
    },
  );

  it("offers the sign-in panel once the status projection is no longer served", async () => {
    routedFetch({
      status: () => Promise.resolve(jsonResponse({ error: "not_found" }, 404)),
      session: sessionRejected,
    });

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(loginSection()?.dataset.authenticationState).toBe("unauthenticated");
    });
    expect(screen.getByLabelText("Username")).toBeTruthy();
    expect(screen.getByLabelText("Password")).toBeTruthy();
  });

  it("offers no sign-in panel when the authentication surface is absent too", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("ECONNREFUSED 127.0.0.1:8443"));

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("unavailable");
    });
    await waitFor(() => {
      expect(loginSection()).toBeNull();
    });
  });

  it("opens the Accounts workspace when the Server session probe authenticates", async () => {
    Object.defineProperty(globalThis.document, "cookie", {
      configurable: true,
      get: () => "__Host-weavelit_csrf=csrf-token",
    });
    vi.spyOn(globalThis, "fetch").mockImplementation((target: unknown) => {
      if (target === "/api/v1/status") {
        return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
      }
      if (target === "/api/v1/auth/session") {
        return Promise.resolve(
          jsonResponse({
            result: {
              account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
              public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
              client_module: "web-ui",
              password_change_required: false,
            },
            correlation_id: AUTH_CORRELATION,
          }),
        );
      }
      if (target === "/api/v1/administration/accounts/list") {
        return Promise.resolve(
          jsonResponse({
            result: {
              items: [
                {
                  public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
                  username: "administrator",
                  display_name: "First Administrator",
                  active: true,
                  mfa_required: false,
                },
              ],
              next_cursor: null,
            },
            correlation_id: "accounts-list-correlation",
          }),
        );
      }
      return Promise.reject(new Error("unexpected request"));
    });

    render(<ApplicationShell />);

    expect(await screen.findByRole("heading", { name: "Accounts" })).toBeTruthy();
    expect(await screen.findByRole("rowheader", { name: "administrator" })).toBeTruthy();
    expect(screen.getByText("Administration")).toBeTruthy();
    expect(loginSection()).toBeNull();
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("returns to neutral sign-in when an Accounts read reports an expired session", async () => {
    Object.defineProperty(globalThis.document, "cookie", {
      configurable: true,
      get: () => "__Host-weavelit_csrf=csrf-token",
    });
    let sessionProbes = 0;
    let listRequests = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation((target: unknown) => {
      if (target === "/api/v1/status") {
        return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
      }
      if (target === "/api/v1/auth/session") {
        sessionProbes += 1;
        if (sessionProbes === 1) {
          return Promise.resolve(
            jsonResponse({
              result: {
                account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
                public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
                client_module: "web-ui",
                password_change_required: false,
              },
              correlation_id: AUTH_CORRELATION,
            }),
          );
        }
        return Promise.resolve(
          jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
        );
      }
      if (target === "/api/v1/administration/accounts/list") {
        listRequests += 1;
        return Promise.resolve(
          jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
        );
      }
      return Promise.reject(new Error("unexpected request"));
    });

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(loginSection()?.dataset.authenticationState).toBe("unauthenticated");
    });
    expect(screen.queryByRole("heading", { name: "Accounts" })).toBeNull();
    expect(screen.queryByText("Accounts are unavailable.")).toBeNull();
    expect(screen.getByLabelText("Username")).toHaveProperty("value", "");
    expect(screen.getByLabelText("Password")).toHaveProperty("value", "");
    expect(listRequests).toBe(1);
    expect(sessionProbes).toBe(2);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("ends Accounts administration after an MFA-policy denial without re-adopting the session", async () => {
    Object.defineProperty(globalThis.document, "cookie", {
      configurable: true,
      get: () => "__Host-weavelit_csrf=csrf-token",
    });
    let sessionProbes = 0;
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target: unknown) => {
      if (target === "/api/v1/status") {
        return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
      }
      if (target === "/api/v1/auth/session") {
        sessionProbes += 1;
        return Promise.resolve(
          jsonResponse({
            result: {
              account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
              public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
              client_module: "web-ui",
              password_change_required: false,
            },
            correlation_id: AUTH_CORRELATION,
          }),
        );
      }
      if (target === "/api/v1/administration/accounts/list") {
        listRequests += 1;
        return Promise.resolve(
          jsonResponse({
            result: {
              items: [
                {
                  public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
                  username: "administrator",
                  display_name: "First Administrator",
                  active: true,
                  mfa_required: false,
                },
              ],
              next_cursor: null,
            },
            correlation_id: "accounts-list-correlation",
          }),
        );
      }
      if (target === MFA_POLICY_STEP_UP_PATH) {
        return Promise.resolve(
          jsonResponse({
            result: {
              totp_step_up_ticket: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            },
            correlation_id: "mfa-policy-correlation",
          }),
        );
      }
      if (target === ACCOUNTS_MFA_REQUIREMENT_PATH) {
        return Promise.resolve(
          jsonResponse(
            { error: "authorization_denied", correlation_id: "mfa-policy-correlation" },
            403,
          ),
        );
      }
      return Promise.reject(new Error("unexpected request"));
    });

    render(<ApplicationShell />);

    await screen.findByRole("heading", { name: "Accounts" });
    fireEvent.click(screen.getByRole("switch", { name: "Require MFA for administrator" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm policy action" }));
    fireEvent.change(screen.getByLabelText("Authentication code"), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify and apply" }));

    await waitFor(() => {
      expect(loginSection()?.dataset.authenticationState).toBe("unauthenticated");
    });
    expect(screen.queryByRole("heading", { name: "Accounts" })).toBeNull();
    expect(screen.queryByText("MFA policy was not changed.")).toBeNull();
    expect(screen.queryByText(/The MFA policy outcome is unknown\./)).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(screen.getByLabelText("Username")).toHaveProperty("value", "");
    expect(screen.getByLabelText("Password")).toHaveProperty("value", "");
    expect(sessionProbes).toBe(1);
    expect(listRequests).toBe(1);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === MFA_POLICY_STEP_UP_PATH),
    ).toHaveLength(1);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_MFA_REQUIREMENT_PATH),
    ).toHaveLength(1);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("loads the Groups workspace only after authenticated navigation selects it", async () => {
    const fetchMock = mockAuthenticatedAdministrationFetch();
    let completeLoad: (module: { GroupsWorkspace: ComponentType }) => void = () => {};
    const loadGroupsWorkspace = vi.fn(
      () =>
        new Promise<{ GroupsWorkspace: ComponentType }>((resolve) => {
          completeLoad = resolve;
        }),
    );

    render(<ApplicationShell loadGroupsWorkspace={loadGroupsWorkspace} />);

    expect(await screen.findByRole("heading", { name: "Accounts" })).toBeTruthy();
    expect(loadGroupsWorkspace).not.toHaveBeenCalled();
    expect(
      fetchMock.mock.calls.filter(([target]) => target === "/api/v1/administration/groups/list"),
    ).toHaveLength(0);

    fireEvent.click(screen.getByRole("button", { name: "Groups" }));

    expect(loadGroupsWorkspace).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toBe("Loading Groups.");
    completeLoad({ GroupsWorkspace: () => <h2>Groups</h2> });
    expect(await screen.findByRole("heading", { name: "Groups" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Groups" }).getAttribute("aria-current")).toBe(
      "page",
    );
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("returns Groups access loss to neutral sign-in without re-adopting the session", async () => {
    const fetchMock = mockAuthenticatedAdministrationFetch();
    const GroupsWorkspace = ({
      onAdministrationEnded,
    }: {
      readonly onAdministrationEnded?: () => void;
    }) => (
      <button type="button" onClick={onAdministrationEnded}>
        End Groups administration
      </button>
    );

    render(<ApplicationShell loadGroupsWorkspace={() => Promise.resolve({ GroupsWorkspace })} />);
    await screen.findByRole("heading", { name: "Accounts" });
    fireEvent.click(screen.getByRole("button", { name: "Groups" }));
    fireEvent.click(await screen.findByRole("button", { name: "End Groups administration" }));

    expect(await screen.findByLabelText("Username")).toBeTruthy();
    expect(screen.getByLabelText("Password")).toBeTruthy();
    expect(screen.queryByText("Administration")).toBeNull();
    expect(
      fetchMock.mock.calls.filter(([target]) => target === "/api/v1/auth/session"),
    ).toHaveLength(1);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("offers a redacted retry when the Groups workspace loader fails", async () => {
    mockAuthenticatedAdministrationFetch();
    const loadGroupsWorkspace = vi
      .fn<() => Promise<{ GroupsWorkspace: ComponentType }>>()
      .mockRejectedValueOnce(new Error("private chunk delivery detail"))
      .mockResolvedValueOnce({ GroupsWorkspace: () => <h2>Groups</h2> });

    render(<ApplicationShell loadGroupsWorkspace={loadGroupsWorkspace} />);
    await screen.findByRole("heading", { name: "Accounts" });
    fireEvent.click(screen.getByRole("button", { name: "Groups" }));

    const failure = await screen.findByRole("alert");
    expect(failure.textContent).toBe("Groups could not be loaded.");
    expect(document.body.textContent).not.toContain("private chunk delivery detail");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("heading", { name: "Groups" })).toBeTruthy();
    expect(loadGroupsWorkspace).toHaveBeenCalledTimes(2);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("loads Configuration only after authenticated navigation selects it", async () => {
    mockAuthenticatedAdministrationFetch();
    let completeLoad: (module: {
      ConfigurationWorkspace: ComponentType<{ readonly onAdministrationEnded?: () => void }>;
    }) => void = () => {};
    const loadConfigurationWorkspace = vi.fn(
      () =>
        new Promise<{
          ConfigurationWorkspace: ComponentType<{
            readonly onAdministrationEnded?: () => void;
          }>;
        }>((resolve) => {
          completeLoad = resolve;
        }),
    );

    render(<ApplicationShell loadConfigurationWorkspace={loadConfigurationWorkspace} />);
    expect(await screen.findByRole("heading", { name: "Accounts" })).toBeTruthy();
    expect(loadConfigurationWorkspace).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Configuration" }));
    expect(loadConfigurationWorkspace).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("status").textContent).toBe("Loading Configuration.");
    completeLoad({ ConfigurationWorkspace: () => <h2>Configuration</h2> });
    expect(await screen.findByRole("heading", { name: "Configuration" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Configuration" }).getAttribute("aria-current")).toBe(
      "page",
    );
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("returns Configuration access loss to neutral sign-in without re-adopting the session", async () => {
    const fetchMock = mockAuthenticatedAdministrationFetch();
    const ConfigurationWorkspace = ({
      onAdministrationEnded,
    }: {
      readonly onAdministrationEnded?: () => void;
    }) => (
      <button type="button" onClick={onAdministrationEnded}>
        End Configuration administration
      </button>
    );

    render(
      <ApplicationShell
        loadConfigurationWorkspace={() => Promise.resolve({ ConfigurationWorkspace })}
      />,
    );
    await screen.findByRole("heading", { name: "Accounts" });
    fireEvent.click(screen.getByRole("button", { name: "Configuration" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "End Configuration administration" }),
    );

    expect(await screen.findByLabelText("Username")).toBeTruthy();
    expect(screen.getByLabelText("Password")).toBeTruthy();
    expect(screen.queryByText("Administration")).toBeNull();
    expect(
      fetchMock.mock.calls.filter(([target]) => target === "/api/v1/auth/session"),
    ).toHaveLength(1);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("offers a redacted retry when the Configuration chunk fails", async () => {
    mockAuthenticatedAdministrationFetch();
    const loadConfigurationWorkspace = vi
      .fn<
        () => Promise<{
          ConfigurationWorkspace: ComponentType<{
            readonly onAdministrationEnded?: () => void;
          }>;
        }>
      >()
      .mockRejectedValueOnce(new Error("private Configuration chunk detail"))
      .mockResolvedValueOnce({ ConfigurationWorkspace: () => <h2>Configuration</h2> });

    render(<ApplicationShell loadConfigurationWorkspace={loadConfigurationWorkspace} />);
    await screen.findByRole("heading", { name: "Accounts" });
    fireEvent.click(screen.getByRole("button", { name: "Configuration" }));

    const failure = await screen.findByRole("alert");
    expect(failure.textContent).toBe("Configuration could not be loaded.");
    expect(document.body.textContent).not.toContain("private Configuration chunk detail");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));

    expect(await screen.findByRole("heading", { name: "Configuration" })).toBeTruthy();
    expect(loadConfigurationWorkspace).toHaveBeenCalledTimes(2);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("returns to sign-in after self-disable succeeds without retrying the mutation", async () => {
    const publicId = "QUFBQUFBQUFBQUFBQUFBQQ";
    Object.defineProperty(globalThis.document, "cookie", {
      configurable: true,
      get: () => "__Host-weavelit_csrf=csrf-token",
    });
    let sessionProbes = 0;
    let listRequests = 0;
    let statusRequests = 0;
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation((target: unknown, init?: RequestInit) => {
        if (target === "/api/v1/status") {
          return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
        }
        if (target === "/api/v1/auth/session") {
          sessionProbes += 1;
          if (sessionProbes === 1) {
            return Promise.resolve(
              jsonResponse({
                result: {
                  account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
                  public_id: publicId,
                  client_module: "web-ui",
                  password_change_required: false,
                },
                correlation_id: AUTH_CORRELATION,
              }),
            );
          }
          return Promise.resolve(
            jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
          );
        }
        if (target === "/api/v1/administration/accounts/list") {
          listRequests += 1;
          return Promise.resolve(
            jsonResponse({
              result: {
                items: [
                  {
                    public_id: publicId,
                    username: "administrator",
                    display_name: "First Administrator",
                    active: true,
                    mfa_required: false,
                  },
                ],
                next_cursor: null,
              },
              correlation_id: "accounts-list-correlation",
            }),
          );
        }
        if (target === "/api/v1/administration/accounts/status") {
          statusRequests += 1;
          if (typeof init?.body !== "string") {
            throw new Error("the status request body must be a JSON string");
          }
          expect(JSON.parse(init.body)).toEqual({ public_id: publicId, active: false });
          return Promise.resolve(
            jsonResponse({
              result: {
                public_id: publicId,
                username: "administrator",
                display_name: "First Administrator",
                active: false,
                mfa_required: false,
              },
              correlation_id: "account-status-correlation",
            }),
          );
        }
        return Promise.reject(new Error("unexpected request"));
      });

    render(<ApplicationShell />);
    await screen.findByRole("rowheader", { name: "administrator" });
    fireEvent.click(screen.getByRole("button", { name: "Disable administrator" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm disable" }));

    await waitFor(() => {
      expect(loginSection()?.dataset.authenticationState).toBe("unauthenticated");
    });
    expect(screen.queryByRole("heading", { name: "Accounts" })).toBeNull();
    expect(statusRequests).toBe(1);
    expect(listRequests).toBe(1);
    expect(sessionProbes).toBe(3);
    expect(
      fetchMock.mock.calls.filter(
        ([target]) => target === "/api/v1/administration/accounts/status",
      ),
    ).toHaveLength(1);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("returns to neutral sign-in when a status mutation reports an expired session", async () => {
    const publicId = "QUFBQUFBQUFBQUFBQUFBQQ";
    Object.defineProperty(globalThis.document, "cookie", {
      configurable: true,
      get: () => "__Host-weavelit_csrf=csrf-token",
    });
    let sessionProbes = 0;
    let listRequests = 0;
    let statusRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target: unknown) => {
      if (target === "/api/v1/status") {
        return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
      }
      if (target === "/api/v1/auth/session") {
        sessionProbes += 1;
        if (sessionProbes === 1) {
          return Promise.resolve(
            jsonResponse({
              result: {
                account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
                public_id: publicId,
                client_module: "web-ui",
                password_change_required: false,
              },
              correlation_id: AUTH_CORRELATION,
            }),
          );
        }
        return Promise.resolve(
          jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
        );
      }
      if (target === "/api/v1/administration/accounts/list") {
        listRequests += 1;
        return Promise.resolve(
          jsonResponse({
            result: {
              items: [
                {
                  public_id: publicId,
                  username: "administrator",
                  display_name: "First Administrator",
                  active: true,
                  mfa_required: false,
                },
              ],
              next_cursor: null,
            },
            correlation_id: "accounts-list-correlation",
          }),
        );
      }
      if (target === "/api/v1/administration/accounts/status") {
        statusRequests += 1;
        return Promise.resolve(
          jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
        );
      }
      return Promise.reject(new Error("unexpected request"));
    });

    render(<ApplicationShell />);
    await screen.findByRole("rowheader", { name: "administrator" });
    fireEvent.click(screen.getByRole("button", { name: "Disable administrator" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm disable" }));

    await waitFor(() => {
      expect(loginSection()?.dataset.authenticationState).toBe("unauthenticated");
    });
    expect(screen.queryByRole("heading", { name: "Accounts" })).toBeNull();
    expect(screen.getByLabelText("Username")).toHaveProperty("value", "");
    expect(screen.getByLabelText("Password")).toHaveProperty("value", "");
    expect(screen.queryByText("The account status was not changed.")).toBeNull();
    expect(screen.queryByText(/The account status outcome is unknown\./)).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(statusRequests).toBe(1);
    expect(listRequests).toBe(1);
    expect(
      fetchMock.mock.calls.filter(
        ([target]) => target === "/api/v1/administration/accounts/status",
      ),
    ).toHaveLength(1);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("returns an MFA-policy authorization denial to neutral sign-in without a follow-on mutation", async () => {
    const publicId = "QUFBQUFBQUFBQUFBQUFBQQ";
    const ticket = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    Object.defineProperty(globalThis.document, "cookie", {
      configurable: true,
      get: () => "__Host-weavelit_csrf=csrf-token",
    });
    let sessionProbes = 0;
    let listRequests = 0;
    let stepUpRequests = 0;
    let requirementRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target: unknown) => {
      if (target === "/api/v1/status") {
        return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
      }
      if (target === "/api/v1/auth/session") {
        sessionProbes += 1;
        if (sessionProbes === 1) {
          return Promise.resolve(
            jsonResponse({
              result: {
                account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
                public_id: publicId,
                client_module: "web-ui",
                password_change_required: false,
              },
              correlation_id: AUTH_CORRELATION,
            }),
          );
        }
        return Promise.resolve(
          jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
        );
      }
      if (target === "/api/v1/administration/accounts/list") {
        listRequests += 1;
        return Promise.resolve(
          jsonResponse({
            result: {
              items: [
                {
                  public_id: publicId,
                  username: "administrator",
                  display_name: "First Administrator",
                  active: true,
                  mfa_required: false,
                },
              ],
              next_cursor: null,
            },
            correlation_id: "accounts-list-correlation",
          }),
        );
      }
      if (target === "/api/v1/administration/step-up/totp") {
        stepUpRequests += 1;
        return Promise.resolve(
          jsonResponse({
            result: { totp_step_up_ticket: ticket },
            correlation_id: AUTH_CORRELATION,
          }),
        );
      }
      if (target === "/api/v1/administration/accounts/mfa-requirement") {
        requirementRequests += 1;
        return Promise.resolve(
          jsonResponse({ error: "authorization_denied", correlation_id: AUTH_CORRELATION }, 403),
        );
      }
      return Promise.reject(new Error("unexpected request"));
    });

    render(<ApplicationShell />);
    await screen.findByRole("rowheader", { name: "administrator" });
    fireEvent.click(screen.getByRole("switch", { name: "Require MFA for administrator" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm policy action" }));
    fireEvent.change(screen.getByLabelText("Authentication code"), { target: { value: "123456" } });
    fireEvent.click(screen.getByRole("button", { name: "Verify and apply" }));

    await waitFor(() => {
      expect(loginSection()?.dataset.authenticationState).toBe("unauthenticated");
    });
    expect(screen.queryByRole("heading", { name: "Accounts" })).toBeNull();
    expect(screen.queryByText("Administration")).toBeNull();
    expect(screen.getByLabelText("Username")).toHaveProperty("value", "");
    expect(screen.getByLabelText("Password")).toHaveProperty("value", "");
    expect(screen.queryByText("MFA policy was not changed.")).toBeNull();
    expect(screen.queryByText(/The MFA policy outcome is unknown\./)).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
    expect(listRequests).toBe(1);
    expect(stepUpRequests).toBe(1);
    expect(requirementRequests).toBe(1);
    expect(
      fetchMock.mock.calls.filter(
        ([target]) => target === "/api/v1/administration/accounts/mfa-requirement",
      ),
    ).toHaveLength(1);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("carries a self-reset disclosure to sign-in and clears it after fresh authentication", async () => {
    const publicId = "QUFBQUFBQUFBQUFBQUFBQQ";
    const temporaryPassword = "YWJjZGVmZ2hpamtsbW5vcHFy";
    const currentPassword = "current-administrator-password";
    const ticket = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    localStorage.clear();
    sessionStorage.clear();
    const storageWrite = vi.spyOn(Storage.prototype, "setItem");
    const originalUrl = location.href;
    Object.defineProperty(globalThis.document, "cookie", {
      configurable: true,
      get: () => "__Host-weavelit_csrf=csrf-token",
    });
    let sessionProbes = 0;
    let listRequests = 0;
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockImplementation((target: unknown, init?: RequestInit) => {
        if (target === "/api/v1/status") {
          return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
        }
        if (target === "/api/v1/auth/session") {
          sessionProbes += 1;
          if (sessionProbes === 1 || sessionProbes === 4) {
            return Promise.resolve(
              jsonResponse({
                result: {
                  account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
                  public_id: publicId,
                  client_module: "web-ui",
                  password_change_required: false,
                },
                correlation_id: AUTH_CORRELATION,
              }),
            );
          }
          return Promise.resolve(
            jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
          );
        }
        if (target === "/api/v1/administration/accounts/list") {
          listRequests += 1;
          return Promise.resolve(
            jsonResponse({
              result: {
                items: [
                  {
                    public_id: publicId,
                    username: "administrator",
                    display_name: "First Administrator",
                    active: true,
                    mfa_required: false,
                  },
                ],
                next_cursor: null,
              },
              correlation_id: "accounts-list-correlation",
            }),
          );
        }
        if (target === "/api/v1/administration/step-up/credential-issuance") {
          return Promise.resolve(
            jsonResponse({
              result: { credential_issuance_ticket: ticket },
              correlation_id: "credential-issuance-correlation",
            }),
          );
        }
        if (target === "/api/v1/administration/accounts/reset-password") {
          return Promise.resolve(
            jsonResponse({
              result: { public_id: publicId, temporary_password: temporaryPassword },
              correlation_id: "credential-issuance-correlation",
            }),
          );
        }
        if (target === "/api/v1/auth/login") {
          if (typeof init?.body !== "string") {
            throw new Error("the login request body must be a JSON string");
          }
          expect(JSON.parse(init.body)).toEqual({
            username: "administrator",
            password: temporaryPassword,
            client_module: "web-ui",
          });
          return Promise.resolve(
            jsonResponse({ result: { authenticated: true }, correlation_id: "login-correlation" }),
          );
        }
        return Promise.reject(new Error("unexpected request"));
      });

    render(<ApplicationShell />);
    await screen.findByRole("rowheader", { name: "administrator" });
    fireEvent.click(screen.getByRole("button", { name: "Reset password for administrator" }));
    fireEvent.change(screen.getByLabelText("Current password"), {
      target: { value: currentPassword },
    });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    await waitFor(() => {
      expect(loginSection()?.dataset.authenticationState).toBe("unauthenticated");
    });
    expect(screen.queryByRole("heading", { name: "Accounts" })).toBeNull();
    expect(screen.getAllByDisplayValue(temporaryPassword)).toHaveLength(1);
    expect(screen.getByLabelText("Username")).toHaveProperty("value", "");
    expect(screen.getByLabelText("Password")).toHaveProperty("value", "");
    expect(document.body.innerHTML).not.toContain(ticket);
    expect(document.body.innerHTML).not.toContain(currentPassword);
    expect(location.href).toBe(originalUrl);
    expect(storageWrite).not.toHaveBeenCalled();
    expect(listRequests).toBe(1);

    fireEvent.change(screen.getByLabelText("Username"), {
      target: { value: "administrator" },
    });
    fireEvent.change(screen.getByLabelText("Password"), {
      target: { value: temporaryPassword },
    });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    await waitFor(() => {
      expect(listRequests).toBe(2);
    });
    expect(screen.queryByRole("textbox", { name: "Temporary password" })).toBeNull();
    expect(document.body.innerHTML).not.toContain(temporaryPassword);
    expect(sessionProbes).toBe(4);
    expect(
      fetchMock.mock.calls.filter(
        ([target]) => target === "/api/v1/administration/accounts/reset-password",
      ),
    ).toHaveLength(1);
    expect(storageWrite).not.toHaveBeenCalled();
    storageWrite.mockRestore();
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it("withholds the Accounts workspace for a restricted password-change session", async () => {
    Object.defineProperty(globalThis.document, "cookie", {
      configurable: true,
      get: () => "__Host-weavelit_csrf=csrf-token",
    });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target: unknown) => {
      if (target === "/api/v1/status") {
        return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
      }
      if (target === "/api/v1/auth/session") {
        return Promise.resolve(
          jsonResponse({
            result: {
              account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
              public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
              client_module: "web-ui",
              password_change_required: true,
            },
            correlation_id: AUTH_CORRELATION,
          }),
        );
      }
      return Promise.reject(new Error("the restricted session must not read administration data"));
    });

    render(<ApplicationShell />);

    expect(await screen.findByRole("heading", { name: "Choose a new password" })).toBeTruthy();
    expect(screen.getByLabelText("New password")).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Accounts" })).toBeNull();
    expect(
      fetchMock.mock.calls.filter(([target]) => target === "/api/v1/administration/accounts/list"),
    ).toHaveLength(0);
    Reflect.deleteProperty(globalThis.document, "cookie");
  });

  it.each([
    [
      "completed",
      () =>
        jsonResponse({
          result: {
            account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
            public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
            client_module: "web-ui",
            password_change_required: false,
          },
          correlation_id: AUTH_CORRELATION,
        }),
      "accounts",
    ],
    [
      "unauthenticated",
      () => jsonResponse({ error: "session_invalid", correlation_id: AUTH_CORRELATION }, 401),
      "login",
    ],
    [
      "still restricted",
      () =>
        jsonResponse({
          result: {
            account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
            public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
            client_module: "web-ui",
            password_change_required: true,
          },
          correlation_id: AUTH_CORRELATION,
        }),
      "indeterminate",
    ],
    ["absent", () => jsonResponse({ error: "not_found" }, 404), "indeterminate"],
    [
      "unreadable",
      () =>
        new Response("{", {
          status: 200,
          headers: { "content-type": "application/json; charset=utf-8" },
        }),
      "indeterminate",
    ],
  ] as const)(
    "reconciles an indeterminate password change after a %s session probe without resubmitting",
    async (_outcome, reconciliationResponse, destination) => {
      Object.defineProperty(globalThis.document, "cookie", {
        configurable: true,
        get: () => "__Host-weavelit_csrf=csrf-token",
      });
      localStorage.clear();
      sessionStorage.clear();
      const storageWrite = vi.spyOn(Storage.prototype, "setItem");
      const originalUrl = location.href;
      const replacementPassword = "one-time replacement";
      let sessionProbes = 0;
      let resolveReconciliation: (response: Response) => void = () => {
        throw new Error("session reconciliation was not started");
      };
      const deferredReconciliation = new Promise<Response>((resolve) => {
        resolveReconciliation = resolve;
      });
      const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target: unknown) => {
        if (target === "/api/v1/status") {
          return Promise.resolve(jsonResponse({ error: "not_found" }, 404));
        }
        if (target === "/api/v1/auth/session") {
          sessionProbes += 1;
          if (sessionProbes === 1) {
            return Promise.resolve(
              jsonResponse({
                result: {
                  account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
                  public_id: "QUFBQUFBQUFBQUFBQUFBQQ",
                  client_module: "web-ui",
                  password_change_required: true,
                },
                correlation_id: AUTH_CORRELATION,
              }),
            );
          }
          if (sessionProbes > 2 && destination === "login") {
            return Promise.resolve(reconciliationResponse());
          }
          return deferredReconciliation;
        }
        if (target === "/api/v1/auth/password/change") {
          return Promise.resolve(
            jsonResponse({ error: "gateway_timeout", correlation_id: AUTH_CORRELATION }, 504),
          );
        }
        if (target === "/api/v1/administration/accounts/list") {
          return Promise.resolve(jsonResponse({ result: { accounts: [], next_cursor: null } }));
        }
        return Promise.reject(new Error("unexpected request"));
      });

      render(<ApplicationShell />);
      const passwordInput = await screen.findByLabelText("New password");
      fireEvent.change(passwordInput, { target: { value: replacementPassword } });
      fireEvent.click(screen.getByRole("button", { name: "Save password" }));

      await waitFor(() => {
        expect(
          document
            .querySelector("section.password-change")
            ?.getAttribute("data-password-change-state"),
        ).toBe("indeterminate");
      });
      const saveButton = screen.getByRole("button", { name: "Save password" });
      expect(passwordInput).toHaveProperty("disabled", true);
      expect(passwordInput).toHaveProperty("value", "");
      expect(saveButton).toHaveProperty("disabled", true);
      expect(document.body.innerHTML).not.toContain(replacementPassword);
      expect(location.href).toBe(originalUrl);
      expect(storageWrite).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("button", { name: "Check session" }));
      await waitFor(() => {
        expect(sessionProbes).toBe(2);
      });

      fireEvent.click(saveButton);
      expect(
        fetchMock.mock.calls.filter(([target]) => target === "/api/v1/auth/password/change"),
      ).toHaveLength(1);

      resolveReconciliation(reconciliationResponse());
      if (destination === "accounts") {
        expect(await screen.findByRole("heading", { name: "Accounts" })).toBeTruthy();
        expect(screen.queryByRole("heading", { name: "Choose a new password" })).toBeNull();
      } else if (destination === "login") {
        expect(await screen.findByRole("heading", { name: "Sign in" })).toBeTruthy();
        expect(screen.getByLabelText("Username")).toHaveProperty("value", "");
        expect(screen.getByLabelText("Password")).toHaveProperty("value", "");
        expect(screen.queryByRole("heading", { name: "Choose a new password" })).toBeNull();
        expect(screen.queryByRole("textbox", { name: "Temporary password" })).toBeNull();
      } else {
        await waitFor(() => {
          expect(screen.getByRole("button", { name: "Check session" })).toBeTruthy();
        });
        expect(
          document
            .querySelector("section.password-change")
            ?.getAttribute("data-password-change-state"),
        ).toBe("indeterminate");
      }
      expect(
        fetchMock.mock.calls.filter(([target]) => target === "/api/v1/auth/password/change"),
      ).toHaveLength(1);
      expect(
        fetchMock.mock.calls.every(
          ([target]) => typeof target !== "string" || !target.includes(replacementPassword),
        ),
      ).toBe(true);
      expect(document.body.innerHTML).not.toContain(replacementPassword);
      expect(location.href).toBe(originalUrl);
      expect(storageWrite).not.toHaveBeenCalled();
      storageWrite.mockRestore();
      Reflect.deleteProperty(globalThis.document, "cookie");
    },
  );
});
