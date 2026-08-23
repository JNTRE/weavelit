import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME, CSRF_HEADER_NAME } from "./weavelit-authentication";
import {
  ACCOUNTS_CREATE_PATH,
  ACCOUNTS_RESET_PASSWORD_PATH,
  CREDENTIAL_ISSUANCE_STEP_UP_PATH,
  CredentialIssuanceIndeterminateError,
  CredentialIssuanceRefusedError,
  CredentialIssuanceSessionInvalidError,
  createAccount,
  issueCredentialIssuanceTicket,
  readCredentialIssued,
  readCredentialIssuanceTicket,
  resetAccountPassword,
} from "./weavelit-credential-issuance";

const CSRF = "csrf-token";
const CORRELATION = "credential-issuance-correlation";
const PASSWORD = "current-password";
const TOTP = "123456";
const TICKET = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const PUBLIC_ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const TEMPORARY_PASSWORD = "YWJjZGVmZ2hpamtsbW5vcHFy";

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function success(result: Record<string, unknown>): Response {
  return jsonResponse({ result, correlation_id: CORRELATION });
}

function refusal(error: string, status: number): Response {
  return jsonResponse({ error, correlation_id: CORRELATION }, status);
}

function withCsrfCookie(onWrite: (value: string) => void = () => undefined): void {
  Object.defineProperty(globalThis.document, "cookie", {
    configurable: true,
    get: () => `${CSRF_COOKIE_NAME}=${CSRF}`,
    set: onWrite,
  });
}

function requestBody(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") {
    throw new Error("the request body must be JSON");
  }
  return JSON.parse(init.body) as Record<string, unknown>;
}

afterEach(() => {
  Reflect.deleteProperty(globalThis.document, "cookie");
  globalThis.localStorage.clear();
  globalThis.sessionStorage.clear();
});

describe("credential issuance response parsing", () => {
  it("accepts only canonical bounded disclosure envelopes", () => {
    expect(
      readCredentialIssuanceTicket({
        result: { credential_issuance_ticket: TICKET },
        correlation_id: CORRELATION,
      }),
    ).toBe(TICKET);
    expect(
      readCredentialIssued({
        result: { public_id: PUBLIC_ID, temporary_password: TEMPORARY_PASSWORD },
        correlation_id: CORRELATION,
      }),
    ).toEqual({ publicId: PUBLIC_ID, temporaryPassword: TEMPORARY_PASSWORD });
  });

  it("rejects additive success envelope and result fields", () => {
    expect(
      readCredentialIssuanceTicket({
        result: { credential_issuance_ticket: TICKET, extra: true },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
    expect(
      readCredentialIssuanceTicket({
        result: { credential_issuance_ticket: TICKET },
        correlation_id: CORRELATION,
        extra: true,
      }),
    ).toBeNull();
    expect(
      readCredentialIssued({
        result: {
          public_id: PUBLIC_ID,
          temporary_password: TEMPORARY_PASSWORD,
          extra: true,
        },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
    expect(
      readCredentialIssued({
        result: { public_id: PUBLIC_ID, temporary_password: TEMPORARY_PASSWORD },
        correlation_id: CORRELATION,
        extra: true,
      }),
    ).toBeNull();
  });

  it.each([
    [
      "a noncanonical ticket",
      { result: { credential_issuance_ticket: `${"A".repeat(42)}B` }, correlation_id: CORRELATION },
    ],
    [
      "an overlong ticket",
      { result: { credential_issuance_ticket: `${TICKET}A` }, correlation_id: CORRELATION },
    ],
    ["a missing correlation", { result: { credential_issuance_ticket: TICKET } }],
    [
      "an invalid correlation",
      { result: { credential_issuance_ticket: TICKET }, correlation_id: "UPPERCASE" },
    ],
  ])("rejects %s", (_label, payload) => {
    expect(readCredentialIssuanceTicket(payload)).toBeNull();
  });

  it.each([
    ["the zero public identifier", "AAAAAAAAAAAAAAAAAAAAAA", TEMPORARY_PASSWORD],
    ["a noncanonical public identifier", "QUFBQUFBQUFBQUFBQUFBUE", TEMPORARY_PASSWORD],
    ["a short temporary password", PUBLIC_ID, TEMPORARY_PASSWORD.slice(1)],
    ["an invalid temporary password", PUBLIC_ID, `${TEMPORARY_PASSWORD.slice(1)}!`],
  ])("rejects %s", (_label, publicId, temporaryPassword) => {
    expect(
      readCredentialIssued({
        result: { public_id: publicId, temporary_password: temporaryPassword },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
  });

  it("accepts public ids with canonical base64url final characters", () => {
    const canonicalEndings = ["A", "Q", "g", "w"];
    for (const ending of canonicalEndings) {
      const publicIdWithEnding = "QUFBQUFBQUFBQUFBQUFBU" + ending;
      expect(
        readCredentialIssued({
          result: { public_id: publicIdWithEnding, temporary_password: TEMPORARY_PASSWORD },
          correlation_id: CORRELATION,
        }),
      ).not.toBeNull();
    }
  });
});

describe("credential issuance requests", () => {
  it("sends assurance secrets only in one no-store same-origin request body", async () => {
    const storageSpy = vi.spyOn(Storage.prototype, "setItem");
    globalThis.sessionStorage.setItem("sanity-check", "safe-value");
    expect(storageSpy).toHaveBeenCalledWith("sanity-check", "safe-value");
    expect(globalThis.sessionStorage.length).toBe(1);
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    storageSpy.mockClear();
    const cookieWrite = vi.fn();
    withCsrfCookie(cookieWrite);
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(success({ credential_issuance_ticket: TICKET }));

    await expect(issueCredentialIssuanceTicket(PASSWORD, TOTP)).resolves.toBe(TICKET);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).toBe(CREDENTIAL_ISSUANCE_STEP_UP_PATH);
    expect(target).not.toContain(PASSWORD);
    expect(target).not.toContain(TOTP);
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
    expect(JSON.stringify(init.headers)).not.toContain(PASSWORD);
    expect(JSON.stringify(init.headers)).not.toContain(TOTP);
    expect(requestBody(init)).toEqual({ password: PASSWORD, totp_code: TOTP });
    expect(cookieWrite).not.toHaveBeenCalled();
    expect(storageSpy).not.toHaveBeenCalled();
    expect(globalThis.localStorage.length).toBe(0);
    expect(globalThis.sessionStorage.length).toBe(0);
  });

  it("omits an absent optional TOTP value", async () => {
    withCsrfCookie();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(success({ credential_issuance_ticket: TICKET }));

    await issueCredentialIssuanceTicket(PASSWORD);

    expect(requestBody(fetchMock.mock.calls[0]![1])).toEqual({ password: PASSWORD });
  });

  it("submits create and reset with a body-only ticket and validates the reset target", async () => {
    withCsrfCookie();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(
        success({ public_id: PUBLIC_ID, temporary_password: TEMPORARY_PASSWORD }),
      )
      .mockResolvedValueOnce(
        success({ public_id: PUBLIC_ID, temporary_password: TEMPORARY_PASSWORD }),
      );

    await expect(createAccount("alice", "Alice", TICKET)).resolves.toMatchObject({
      publicId: PUBLIC_ID,
    });
    await expect(resetAccountPassword(PUBLIC_ID, TICKET)).resolves.toMatchObject({
      temporaryPassword: TEMPORARY_PASSWORD,
    });

    expect(fetchMock).toHaveBeenCalledTimes(2);
    expect(fetchMock.mock.calls[0]![0]).toBe(ACCOUNTS_CREATE_PATH);
    expect(requestBody(fetchMock.mock.calls[0]![1])).toEqual({
      username: "alice",
      display_name: "Alice",
      credential_issuance_ticket: TICKET,
    });
    expect(fetchMock.mock.calls[1]![0]).toBe(ACCOUNTS_RESET_PASSWORD_PATH);
    expect(requestBody(fetchMock.mock.calls[1]![1])).toEqual({
      public_id: PUBLIC_ID,
      credential_issuance_ticket: TICKET,
    });
    for (const [target, init] of fetchMock.mock.calls as [string, RequestInit][]) {
      expect(target).not.toContain(TICKET);
      expect(JSON.stringify(init.headers)).not.toContain(TICKET);
      expect(init.cache).toBe("no-store");
    }
  });

  it.each([
    [400, "bad_request"],
    [403, "request_origin_denied"],
    [403, "authorization_denied"],
    [403, "credential_issuance_denied"],
    [404, "not_found"],
    [405, "method_not_allowed"],
    [409, "conflict"],
    [503, "service_unavailable"],
  ])("maps the reported %i %s response to one reason-free refusal", async (status, code) => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(refusal(code, status));

    const error = await issueCredentialIssuanceTicket(PASSWORD).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(CredentialIssuanceRefusedError);
    expect(JSON.stringify(error)).toBe('{"name":"CredentialIssuanceRefusedError"}');
  });

  it("preserves an exact session-invalid response without retrying", async () => {
    withCsrfCookie();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(refusal("session_invalid", 401));

    const error = await issueCredentialIssuanceTicket(PASSWORD).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(CredentialIssuanceSessionInvalidError);
    expect(error).toBeInstanceOf(CredentialIssuanceRefusedError);
    expect(JSON.stringify(error)).toBe('{"name":"CredentialIssuanceSessionInvalidError"}');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it.each([
    ["a transport failure", () => Promise.reject(new Error("ECONNRESET secret"))],
    ["a gateway timeout", () => Promise.resolve(refusal("gateway_timeout", 504))],
    ["an unreadable success", () => Promise.resolve(new Response("not json", { status: 200 }))],
    [
      "a malformed success",
      () => Promise.resolve(success({ credential_issuance_ticket: "short" })),
    ],
    [
      "an additive success envelope",
      () =>
        Promise.resolve(
          jsonResponse({
            result: { credential_issuance_ticket: TICKET },
            correlation_id: CORRELATION,
            extra: true,
          }),
        ),
    ],
    [
      "an additive ticket result",
      () => Promise.resolve(success({ credential_issuance_ticket: TICKET, extra: true })),
    ],
    [
      "an additive error envelope",
      () =>
        Promise.resolve(
          jsonResponse({ error: "session_invalid", correlation_id: CORRELATION, extra: true }, 401),
        ),
    ],
    ["an unknown rejection", () => Promise.resolve(refusal("future_error", 503))],
  ])("reports %s as indeterminate without retrying", async (_label, outcome) => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(outcome);

    const error = await issueCredentialIssuanceTicket(PASSWORD).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(CredentialIssuanceIndeterminateError);
    expect(JSON.stringify(error)).toBe('{"name":"CredentialIssuanceIndeterminateError"}');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("reports an additive credential result as indeterminate without retrying", async () => {
    withCsrfCookie();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        success({ public_id: PUBLIC_ID, temporary_password: TEMPORARY_PASSWORD, extra: true }),
      );

    await expect(createAccount("alice", undefined, TICKET)).rejects.toBeInstanceOf(
      CredentialIssuanceIndeterminateError,
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("rejects a reset success for another public identifier without retrying", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      success({
        public_id: "QkJCQkJCQkJCQkJCQkJCQg",
        temporary_password: TEMPORARY_PASSWORD,
      }),
    );

    await expect(resetAccountPassword(PUBLIC_ID, TICKET)).rejects.toBeInstanceOf(
      CredentialIssuanceIndeterminateError,
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
