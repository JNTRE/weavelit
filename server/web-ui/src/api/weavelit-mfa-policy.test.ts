import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME, CSRF_HEADER_NAME } from "./weavelit-authentication";
import {
  ACCOUNTS_MFA_REQUIREMENT_PATH,
  ACCOUNTS_MFA_RESET_PATH,
  MFA_POLICY_STEP_UP_PATH,
  MfaPolicyAccessDeniedError,
  MfaPolicyGrantMutationAccessDeniedError,
  MfaPolicyIndeterminateError,
  MfaPolicyRefusedError,
  MfaPolicySessionInvalidError,
  changeMfaRequirement,
  issueGrantMutationStepUp,
  issueMfaPolicyStepUp,
  readMfaPolicyTicket,
  resetMfaEnrollment,
} from "./weavelit-mfa-policy";

const CSRF = "csrf-token";
const CORRELATION = "mfa-policy-correlation";
const CODE = "123456";
const TICKET = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const PUBLIC_ID = "QUFBQUFBQUFBQUFBQUFBQQ";

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function success(result: Record<string, unknown>): Response {
  return jsonResponse({ result, correlation_id: CORRELATION });
}

function projection(required: boolean): Record<string, unknown> {
  return {
    public_id: PUBLIC_ID,
    username: "administrator",
    display_name: "Administrator",
    active: true,
    mfa_required: required,
  };
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

describe("MFA policy response parsing", () => {
  it("accepts only the exact canonical ticket envelope", () => {
    expect(
      readMfaPolicyTicket({
        result: { totp_step_up_ticket: TICKET },
        correlation_id: CORRELATION,
      }),
    ).toBe(TICKET);
    expect(
      readMfaPolicyTicket({
        result: { totp_step_up_ticket: TICKET, extra: "secret" },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
    expect(
      readMfaPolicyTicket({
        result: { totp_step_up_ticket: `${"A".repeat(42)}B` },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
    expect(readMfaPolicyTicket({ result: { totp_step_up_ticket: TICKET } })).toBeNull();
  });
});

describe("MFA policy requests", () => {
  it("sends the code only in one fixed-family no-store same-origin request", async () => {
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    const storageWrite = vi.spyOn(Storage.prototype, "setItem");
    globalThis.sessionStorage.setItem("sanity-check", "safe-value");
    expect(storageWrite).toHaveBeenCalledWith("sanity-check", "safe-value");
    expect(globalThis.sessionStorage.length).toBe(1);
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    storageWrite.mockClear();

    const cookieWrite = vi.fn();
    withCsrfCookie(cookieWrite);
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(success({ totp_step_up_ticket: TICKET }));

    await expect(issueMfaPolicyStepUp(CODE)).resolves.toBe(TICKET);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).toBe(MFA_POLICY_STEP_UP_PATH);
    expect(target).not.toContain(CODE);
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
    expect(requestBody(init)).toEqual({ family: "mfa_policy", code: CODE });
    expect(JSON.stringify(init.headers)).not.toContain(CODE);
    expect(cookieWrite).not.toHaveBeenCalled();
    expect(storageWrite).not.toHaveBeenCalled();
  });

  it("submits requirement and reset with body-only tickets and exact targets", async () => {
    withCsrfCookie();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(success(projection(true)))
      .mockResolvedValueOnce(success(projection(true)));

    await expect(changeMfaRequirement(PUBLIC_ID, true, TICKET)).resolves.toMatchObject({
      publicId: PUBLIC_ID,
      mfaRequired: true,
    });
    await expect(resetMfaEnrollment(PUBLIC_ID, TICKET)).resolves.toMatchObject({
      publicId: PUBLIC_ID,
    });

    expect(fetchMock.mock.calls[0]![0]).toBe(ACCOUNTS_MFA_REQUIREMENT_PATH);
    expect(requestBody(fetchMock.mock.calls[0]![1])).toEqual({
      public_id: PUBLIC_ID,
      required: true,
      totp_step_up_ticket: TICKET,
    });
    expect(fetchMock.mock.calls[1]![0]).toBe(ACCOUNTS_MFA_RESET_PATH);
    expect(requestBody(fetchMock.mock.calls[1]![1])).toEqual({
      public_id: PUBLIC_ID,
      totp_step_up_ticket: TICKET,
    });
    for (const [target, init] of fetchMock.mock.calls as [string, RequestInit][]) {
      expect(target).not.toContain(TICKET);
      expect(JSON.stringify(init.headers)).not.toContain(TICKET);
      expect(init.cache).toBe("no-store");
    }
  });

  it.each([
    [400, "bad_request"],
    [403, "mfa_policy_denied"],
    [404, "not_found"],
    [405, "method_not_allowed"],
    [503, "service_unavailable"],
  ])("maps the reported %i %s response to one reason-free refusal", async (status, code) => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ error: code, correlation_id: CORRELATION }, status),
    );

    const error = await issueMfaPolicyStepUp(CODE).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(MfaPolicyRefusedError);
    expect(JSON.stringify(error)).toBe('{"name":"MfaPolicyRefusedError"}');
  });

  it("preserves an exact session-invalid GrantMutation step-up response", async () => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ error: "session_invalid", correlation_id: CORRELATION }, 401),
    );

    const error = await issueGrantMutationStepUp(CODE).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(MfaPolicySessionInvalidError);
    expect(error).toBeInstanceOf(MfaPolicyRefusedError);
    expect(JSON.stringify(error)).toBe('{"name":"MfaPolicySessionInvalidError"}');
  });

  it("maps exact GrantMutation authorization denial to a terminal access error", async () => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ error: "authorization_denied", correlation_id: CORRELATION }, 403),
    );

    const error = await issueGrantMutationStepUp(CODE).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(MfaPolicyGrantMutationAccessDeniedError);
    expect(error).toBeInstanceOf(MfaPolicyRefusedError);
    expect(JSON.stringify(error)).toBe('{"name":"MfaPolicyGrantMutationAccessDeniedError"}');
  });

  it.each([
    ["step-up", () => issueMfaPolicyStepUp(CODE)],
    ["requirement", () => changeMfaRequirement(PUBLIC_ID, true, TICKET)],
    ["reset", () => resetMfaEnrollment(PUBLIC_ID, TICKET)],
  ])(
    "maps exact MFA-policy authorization denial on %s to a terminal access error",
    async (_path, request) => {
      withCsrfCookie();
      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        jsonResponse({ error: "authorization_denied", correlation_id: CORRELATION }, 403),
      );

      const error = await request().catch((reason: unknown) => reason);

      expect(error).toBeInstanceOf(MfaPolicyAccessDeniedError);
      expect(error).not.toBeInstanceOf(MfaPolicyGrantMutationAccessDeniedError);
      expect(JSON.stringify(error)).toBe('{"name":"MfaPolicyAccessDeniedError"}');
    },
  );

  it.each([
    ["a transport failure", () => Promise.reject(new Error("ECONNRESET secret"))],
    ["a gateway timeout", () => Promise.resolve(jsonResponse({ error: "gateway_timeout" }, 504))],
    ["an unreadable success", () => Promise.resolve(new Response("not json", { status: 200 }))],
    [
      "an additive success",
      () => Promise.resolve(success({ totp_step_up_ticket: TICKET, extra: true })),
    ],
    ["an unknown rejection", () => Promise.resolve(jsonResponse({ error: "future" }, 503))],
    [
      "a malformed MFA-policy authorization denial",
      () =>
        Promise.resolve(
          jsonResponse(
            { error: "authorization_denied", correlation_id: CORRELATION, unexpected: true },
            403,
          ),
        ),
    ],
  ])("reports %s as indeterminate without retrying", async (_label, outcome) => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(outcome);

    const error = await issueMfaPolicyStepUp(CODE).catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(MfaPolicyIndeterminateError);
    expect(JSON.stringify(error)).toBe('{"name":"MfaPolicyIndeterminateError"}');
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("rejects malformed inputs and mismatched success without sending another request", async () => {
    withCsrfCookie();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(success({ ...projection(false), public_id: "QkJCQkJCQkJCQkJCQkJCQg" }));

    await expect(changeMfaRequirement("bad", true, TICKET)).rejects.toBeInstanceOf(
      MfaPolicyRefusedError,
    );
    await expect(changeMfaRequirement(PUBLIC_ID, true, TICKET)).rejects.toBeInstanceOf(
      MfaPolicyIndeterminateError,
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("rejects public ids with non-canonical base64url final characters", async () => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(success(projection(false)));

    await expect(
      changeMfaRequirement("QUFBQUFBQUFBQUFBQUFBUE", true, TICKET),
    ).rejects.toBeInstanceOf(MfaPolicyRefusedError);
    await expect(resetMfaEnrollment("QUFBQUFBQUFBQUFBQUFBUE", TICKET)).rejects.toBeInstanceOf(
      MfaPolicyRefusedError,
    );
  });

  it("accepts public ids with canonical base64url final characters", async () => {
    withCsrfCookie();
    const canonicalEndings = ["A", "Q", "g", "w"];
    const fetchMock = vi.spyOn(globalThis, "fetch");
    for (const ending of canonicalEndings) {
      const publicIdWithEnding = "QUFBQUFBQUFBQUFBQUFBU" + ending;
      fetchMock.mockResolvedValueOnce(
        success({ ...projection(true), public_id: publicIdWithEnding }),
      );
    }
    for (const ending of canonicalEndings) {
      const publicIdWithEnding = "QUFBQUFBQUFBQUFBQUFBU" + ending;
      await expect(changeMfaRequirement(publicIdWithEnding, true, TICKET)).resolves.toMatchObject({
        publicId: publicIdWithEnding,
      });
    }
  });
});
