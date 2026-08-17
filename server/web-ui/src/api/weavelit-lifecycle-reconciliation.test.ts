import { describe, expect, it, vi } from "vitest";

import {
  LIFECYCLE_RECONCILIATION_PATH,
  reconcileLifecycle,
} from "./weavelit-lifecycle-reconciliation";

const CAPABILITY = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghi";

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function mockFetch(response: () => Promise<Response>) {
  return vi.spyOn(globalThis, "fetch").mockImplementation(response);
}

describe("reconcileLifecycle", () => {
  it("submits the capability only in the exact same-origin JSON request body", async () => {
    const fetchMock = mockFetch(() =>
      Promise.resolve(jsonResponse({ result: "reconciliation_confirmed" }, 200)),
    );

    await expect(reconcileLifecycle(CAPABILITY)).resolves.toBe("confirmed");

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0]!;
    expect(target).toBe(LIFECYCLE_RECONCILIATION_PATH);
    expect(init?.method).toBe("PUT");
    expect(init?.headers).toStrictEqual({
      Accept: "application/json",
      "Content-Type": "application/json",
      "X-Weavelit-CSRF": "1",
    });
    expect(init?.body).toBe(JSON.stringify({ reconciliation_capability: CAPABILITY }));
    expect(init?.credentials).toBe("omit");
    expect(init?.cache).toBe("no-store");
    expect(init?.redirect).toBe("error");
    expect(target).not.toContain(CAPABILITY);
    expect(JSON.stringify(init?.headers)).not.toContain(CAPABILITY);
    expect(globalThis.localStorage.length).toBe(0);
    expect(globalThis.sessionStorage.length).toBe(0);
    expect(document.cookie).not.toContain(CAPABILITY);
  });

  it("reports a matching fixed confirmation", async () => {
    mockFetch(() => Promise.resolve(jsonResponse({ result: "reconciliation_confirmed" }, 200)));

    await expect(reconcileLifecycle(CAPABILITY)).resolves.toBe("confirmed");
  });

  it("reports a fixed mismatch without treating it as confirmation", async () => {
    mockFetch(() => Promise.resolve(jsonResponse({ error: "not_found" }, 404)));

    await expect(reconcileLifecycle(CAPABILITY)).resolves.toBe("not_found");
  });

  it.each([
    ["a transport failure", () => Promise.reject(new TypeError("network"))],
    ["an unreadable confirmation", () => Promise.resolve(new Response("<html>", { status: 200 }))],
    [
      "an invalid confirmation",
      () => Promise.resolve(jsonResponse({ result: "initialized" }, 200)),
    ],
    ["an unreadable mismatch", () => Promise.resolve(new Response("<html>", { status: 404 }))],
    [
      "an unavailable response",
      () => Promise.resolve(jsonResponse({ error: "service_unavailable" }, 503)),
    ],
  ])("keeps %s indeterminate", async (_label, response) => {
    mockFetch(response);

    await expect(reconcileLifecycle(CAPABILITY)).resolves.toBe("absent");
  });

  it("does not issue a request for a malformed capability", async () => {
    const fetchMock = mockFetch(() =>
      Promise.resolve(jsonResponse({ result: "reconciliation_confirmed" }, 200)),
    );

    await expect(reconcileLifecycle("not a capability")).resolves.toBe("absent");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
