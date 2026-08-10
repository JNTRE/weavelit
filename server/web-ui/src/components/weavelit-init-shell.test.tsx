import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { ApplicationShell } from "./weavelit-init-shell";

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

describe("ApplicationShell database selection control", () => {
  it("offers the selection control only when no Application Database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => unselectedStatus());

    render(<ApplicationShell />);

    expect(screen.queryAllByRole("button")).toHaveLength(0);
    await waitFor(() => {
      expect(selectionButton().disabled).toBe(false);
    });
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
    await waitFor(() => {
      expect(selectionButton().disabled).toBe(false);
    });

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
    await waitFor(() => {
      expect(selectionButton().disabled).toBe(false);
    });
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
    await waitFor(() => {
      expect(selectionButton().disabled).toBe(false);
    });

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
    await waitFor(() => {
      expect(selectionButton().disabled).toBe(false);
    });

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
    await waitFor(() => {
      expect(selectionButton().disabled).toBe(false);
    });

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

  it("offers the Restore form once an Application Database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ lifecycle: "uninitialized", database_selected: true }),
    );

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(restoreSection()).not.toBeNull();
    });
    expect(screen.getByRole("heading", { name: "Restore" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Restore backup" })).toBeTruthy();
  });

  it("does not offer the Restore form before a database is selected", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(() => unselectedStatus());

    render(<ApplicationShell />);

    await waitFor(() => {
      expect(statusRegion().dataset.statusState).toBe("available");
    });
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
    await waitFor(() => {
      expect(selectionButton().disabled).toBe(false);
    });
    expect(restoreSection()).toBeNull();

    fireEvent.click(selectionButton());

    await waitFor(() => {
      expect(restoreSection()).not.toBeNull();
    });
  });
});
