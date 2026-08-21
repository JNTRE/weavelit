import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME, CSRF_HEADER_NAME } from "./weavelit-authentication";
import {
  ACCOUNTS_LIST_PATH,
  ACCOUNTS_VIEW_PATH,
  AccountsUnavailableError,
  listAccounts,
  readAccountProjection,
  readAccountsPage,
  viewAccount,
} from "./weavelit-administration-accounts";

const CSRF = "csrf-token";
const PUBLIC_ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const CORRELATION = "correlation-0123456789";

function projection(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    public_id: PUBLIC_ID,
    username: "administrator",
    display_name: "First Administrator",
    active: true,
    mfa_required: false,
    ...overrides,
  };
}

function jsonResponse(result: unknown, status = 200): Response {
  return new Response(JSON.stringify(result), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function withCsrfCookie(): void {
  Object.defineProperty(globalThis.document, "cookie", {
    configurable: true,
    get: () => `${CSRF_COOKIE_NAME}=${CSRF}`,
  });
}

function requestBody(init: RequestInit | undefined): unknown {
  if (typeof init?.body !== "string") {
    throw new Error("the request body must be a JSON string");
  }
  return JSON.parse(init.body) as unknown;
}

afterEach(() => {
  Reflect.deleteProperty(globalThis.document, "cookie");
});

describe("account response parsing", () => {
  it("accepts the exact safe list and view projections", () => {
    expect(
      readAccountsPage({
        result: { items: [projection()], next_cursor: null },
        correlation_id: CORRELATION,
      }),
    ).toEqual({
      items: [
        {
          publicId: PUBLIC_ID,
          username: "administrator",
          displayName: "First Administrator",
          active: true,
          mfaRequired: false,
        },
      ],
      nextCursor: null,
    });
    expect(
      readAccountProjection({ result: projection(), correlation_id: CORRELATION }),
    ).not.toBeNull();
  });

  it.each([
    ["a verifier", projection({ password_verifier: "secret", username: null })],
    ["a malformed public id", projection({ public_id: "internal-state-id" })],
    ["an overlong name", projection({ username: "a".repeat(257) })],
    ["a wrong active type", projection({ active: 1 })],
  ])("rejects %s without carrying response detail", (_label, item) => {
    expect(readAccountProjection({ result: item })).toBeNull();
  });

  it("rejects malformed and oversized collection results", () => {
    expect(readAccountsPage({ result: { items: [], next_cursor: "not*base64" } })).toBeNull();
    expect(
      readAccountsPage({
        result: { items: Array.from({ length: 101 }, () => projection()), next_cursor: null },
      }),
    ).toBeNull();
  });
});

describe("account requests", () => {
  it("lists through a same-origin session-bearing PUT", async () => {
    withCsrfCookie();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        jsonResponse({ result: { items: [projection()], next_cursor: "bmV4dA" } }),
      );

    await expect(listAccounts("cHJldmlvdXM")).resolves.toMatchObject({ nextCursor: "bmV4dA" });
    const [target, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).toBe(ACCOUNTS_LIST_PATH);
    expect(init).toMatchObject({
      method: "PUT",
      credentials: "same-origin",
      cache: "no-store",
      redirect: "error",
    });
    expect(init.headers).toEqual({
      Accept: "application/json",
      "Content-Type": "application/json",
      [CSRF_HEADER_NAME]: CSRF,
    });
    expect(requestBody(init)).toEqual({ cursor: "cHJldmlvdXM" });
  });

  it("views only the exact requested public identifier", async () => {
    withCsrfCookie();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(jsonResponse({ result: projection(), correlation_id: CORRELATION }));

    await expect(viewAccount(PUBLIC_ID)).resolves.toMatchObject({ publicId: PUBLIC_ID });
    expect(fetchMock.mock.calls[0]![0]).toBe(ACCOUNTS_VIEW_PATH);
    expect(requestBody(fetchMock.mock.calls[0]![1])).toEqual({ public_id: PUBLIC_ID });
  });

  it("maps missing CSRF, stable errors, transport failures, and malformed success to one error", async () => {
    await expect(listAccounts()).rejects.toBeInstanceOf(AccountsUnavailableError);

    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      jsonResponse({ error: "authorization_denied", detail: "secret" }, 403),
    );
    await expect(listAccounts()).rejects.toBeInstanceOf(AccountsUnavailableError);

    vi.mocked(globalThis.fetch).mockRejectedValueOnce(new Error("ECONNRESET secret"));
    await expect(listAccounts()).rejects.toBeInstanceOf(AccountsUnavailableError);

    vi.mocked(globalThis.fetch).mockResolvedValueOnce(
      jsonResponse({ result: { items: "secret" } }),
    );
    await expect(listAccounts()).rejects.toBeInstanceOf(AccountsUnavailableError);
  });
});
