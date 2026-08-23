import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME, CSRF_HEADER_NAME } from "./weavelit-authentication";
import {
  ACCOUNTS_LIST_PATH,
  ACCOUNTS_STATUS_PATH,
  ACCOUNTS_VIEW_PATH,
  AccountStatusIndeterminateError,
  AccountStatusRefusedError,
  AccountsSessionExpiredError,
  AccountsUnavailableError,
  changeAccountStatus,
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
    ["an additive verifier", projection({ password_verifier: "secret" })],
    ["a malformed public id", projection({ public_id: "internal-state-id" })],
    ["a zero public id", projection({ public_id: "AAAAAAAAAAAAAAAAAAAAAA" })],
    [
      "a public id with non-canonical final character",
      projection({ public_id: "QUFBQUFBQUFBQUFBQUFBUE" }),
    ],
    ["an overlong name", projection({ username: "a".repeat(257) })],
    ["a wrong active type", projection({ active: 1 })],
  ])("rejects %s without carrying response detail", (_label, item) => {
    expect(readAccountProjection({ result: item, correlation_id: CORRELATION })).toBeNull();
  });

  it("accepts public ids with canonical base64url final characters", () => {
    const canonicalEndings = ["A", "Q", "g", "w"];
    for (const ending of canonicalEndings) {
      const publicIdWithEnding = "QUFBQUFBQUFBQUFBQUFBU" + ending;
      expect(
        readAccountProjection({
          result: projection({ public_id: publicIdWithEnding }),
          correlation_id: CORRELATION,
        }),
      ).not.toBeNull();
    }
  });

  it("rejects malformed and oversized collection results", () => {
    expect(
      readAccountsPage({
        result: { items: [], next_cursor: "not*base64" },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
    expect(
      readAccountsPage({
        result: { items: Array.from({ length: 101 }, () => projection()), next_cursor: null },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
  });

  it("rejects non-canonical result envelopes and page shapes", () => {
    const page = { items: [projection()], next_cursor: null };
    expect(readAccountsPage({ result: page })).toBeNull();
    expect(readAccountsPage({ result: page, correlation_id: "UPPERCASE" })).toBeNull();
    expect(
      readAccountsPage({ result: page, correlation_id: CORRELATION, unexpected: true }),
    ).toBeNull();
    expect(
      readAccountsPage({
        result: { ...page, unexpected: true },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();

    expect(readAccountProjection({ result: projection() })).toBeNull();
    expect(readAccountProjection({ result: projection(), correlation_id: "UPPERCASE" })).toBeNull();
    expect(
      readAccountProjection({
        result: projection(),
        correlation_id: CORRELATION,
        unexpected: true,
      }),
    ).toBeNull();
  });
});

describe("account requests", () => {
  it("lists through a same-origin session-bearing PUT", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({
        result: { items: [projection()], next_cursor: "bmV4dA" },
        correlation_id: CORRELATION,
      }),
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

  it("rejects a non-canonical view result envelope", async () => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({
        result: projection(),
        correlation_id: CORRELATION,
        unexpected: true,
      }),
    );

    await expect(viewAccount(PUBLIC_ID)).rejects.toBeInstanceOf(AccountsUnavailableError);
  });

  it("rejects account view request with non-canonical public identifier", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch");

    await expect(viewAccount("QUFBQUFBQUFBQUFBQUFBUE")).rejects.toBeInstanceOf(
      AccountsUnavailableError,
    );
    expect(fetchMock).not.toHaveBeenCalled();
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

  it("distinguishes only an exact expired-session read response", async () => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        jsonResponse({ error: "session_invalid", correlation_id: CORRELATION }, 401),
      )
      .mockResolvedValueOnce(
        jsonResponse(
          { error: "session_invalid", correlation_id: CORRELATION, unexpected: true },
          401,
        ),
      );

    await expect(listAccounts()).rejects.toBeInstanceOf(AccountsSessionExpiredError);
    await expect(listAccounts()).rejects.toBeInstanceOf(AccountsUnavailableError);
  });

  it("changes status through one exact same-origin request and accepts only the requested result", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({
        result: projection({ active: false }),
        correlation_id: CORRELATION,
      }),
    );

    await expect(changeAccountStatus(PUBLIC_ID, false)).resolves.toMatchObject({
      publicId: PUBLIC_ID,
      active: false,
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).toBe(ACCOUNTS_STATUS_PATH);
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
    expect(requestBody(init)).toEqual({ public_id: PUBLIC_ID, active: false });
  });

  it("rejects account status change request with non-canonical public identifier", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch");

    await expect(changeAccountStatus("QUFBQUFBQUFBQUFBQUFBUE", false)).rejects.toBeInstanceOf(
      AccountStatusRefusedError,
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("maps only exact canonical status session invalidation to an expired session", async () => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        jsonResponse({ error: "session_invalid", correlation_id: CORRELATION }, 401),
      )
      .mockResolvedValueOnce(
        jsonResponse(
          { error: "session_invalid", correlation_id: CORRELATION, unexpected: true },
          401,
        ),
      )
      .mockResolvedValueOnce(
        jsonResponse({ error: "unknown_refusal", correlation_id: CORRELATION }, 401),
      )
      .mockResolvedValueOnce(
        jsonResponse({ error: "session_invalid", correlation_id: CORRELATION }, 403),
      );

    await expect(changeAccountStatus(PUBLIC_ID, false)).rejects.toBeInstanceOf(
      AccountsSessionExpiredError,
    );
    await expect(changeAccountStatus(PUBLIC_ID, false)).rejects.toBeInstanceOf(
      AccountStatusIndeterminateError,
    );
    await expect(changeAccountStatus(PUBLIC_ID, false)).rejects.toBeInstanceOf(
      AccountStatusIndeterminateError,
    );
    await expect(changeAccountStatus(PUBLIC_ID, false)).rejects.toBeInstanceOf(
      AccountStatusIndeterminateError,
    );
  });

  it("distinguishes exact reported refusals from indeterminate outcomes without retrying", async () => {
    withCsrfCookie();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        jsonResponse({ error: "authorization_denied", correlation_id: CORRELATION }, 403),
      )
      .mockRejectedValueOnce(new Error("connection closed after request"))
      .mockResolvedValueOnce(
        jsonResponse({
          result: projection({ active: true }),
          correlation_id: CORRELATION,
        }),
      );

    await expect(changeAccountStatus(PUBLIC_ID, false)).rejects.toBeInstanceOf(
      AccountStatusRefusedError,
    );
    await expect(changeAccountStatus(PUBLIC_ID, false)).rejects.toBeInstanceOf(
      AccountStatusIndeterminateError,
    );
    await expect(changeAccountStatus(PUBLIC_ID, false)).rejects.toBeInstanceOf(
      AccountStatusIndeterminateError,
    );
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });
});
