import { describe, expect, it, vi } from "vitest";

import {
  AUDIT_LOG_CONFIGURATION,
  INIT_PATH,
  INIT_RECOVERY_KEY_PATH,
  InitFailedError,
  SQLITE_LOG_MODULE,
  SYSTEM_LOG_CONFIGURATION,
  UNREPORTED_FAILURE_CODE,
  finalizeInit,
  initRequestBody,
  isInitCompleted,
  parseDeliveredRecoveryKey,
  parseStableErrorCode,
  prepareRecoveryKey,
  type InitDetails,
} from "./weavelit-init-setup";

const RECOVERY_KEY = "AGE-SECRET-KEY-1QQQSYQCYQ5RQWZQFPG9SCRGWPUGPZYSNZS23V9CCRYDPK8QARC0SWRYDWG";
const DELIVERY_NONCE = "ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8";
const PROOF = "YiFd573c6n4sQEf_a7lPjRgmL8iz82SBNLt9RBWP-E0";
const CORRELATION = "0123456789abcdef0123456789abcdef";
const PASSWORD = "correct horse battery staple";

const DETAILS: InitDetails = {
  username: "administrator",
  displayName: "First Administrator",
  password: PASSWORD,
  systemLogModule: SQLITE_LOG_MODULE,
  auditLogModule: SQLITE_LOG_MODULE,
};

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function deliveryResponse(): Response {
  return jsonResponse(
    {
      result: { recovery_key: RECOVERY_KEY, delivery_nonce: DELIVERY_NONCE },
      correlation_id: CORRELATION,
    },
    200,
  );
}

function completionResponse(): Response {
  return jsonResponse({ result: { lifecycle: "initialized" }, correlation_id: CORRELATION }, 200);
}

function mockFetch(response: () => Promise<Response>) {
  return vi.spyOn(globalThis, "fetch").mockImplementation(response);
}

/**
 * Returns the exact request body text a submission sent.
 *
 * The body is asserted to be a string rather than coerced, because a non-string
 * body would stringify to something like `[object Blob]` and make every
 * "the key never reached the wire" assertion below pass without inspecting it.
 */
function submittedBodyText(mock: ReturnType<typeof mockFetch>, call = 0): string {
  const body = mock.mock.calls[call]?.[1]?.body;
  expect(typeof body).toBe("string");
  return body as string;
}

function submittedBody(mock: ReturnType<typeof mockFetch>): Record<string, unknown> {
  return JSON.parse(submittedBodyText(mock)) as Record<string, unknown>;
}

describe("parseStableErrorCode", () => {
  it("returns a documented stable code", () => {
    expect(parseStableErrorCode({ error: "recovery_key_confirmation_invalid" })).toBe(
      "recovery_key_confirmation_invalid",
    );
  });

  it.each([
    ["null", null],
    ["an array", [{ error: "bad_request" }]],
    ["a string", "bad_request"],
    ["a missing error field", { correlation_id: CORRELATION }],
    ["a non-string error", { error: 400 }],
    ["an uppercase code", { error: "Bad_Request" }],
    ["a code carrying a message", { error: "bad request: administrator.password" }],
    ["an over-long code", { error: "a".repeat(49) }],
    ["an empty code", { error: "" }],
  ])("discards %s", (_label, payload) => {
    expect(parseStableErrorCode(payload)).toBe(UNREPORTED_FAILURE_CODE);
  });
});

describe("parseDeliveredRecoveryKey", () => {
  it("accepts the documented delivery envelope", () => {
    expect(
      parseDeliveredRecoveryKey({
        result: { recovery_key: RECOVERY_KEY, delivery_nonce: DELIVERY_NONCE },
        correlation_id: CORRELATION,
      }),
    ).toStrictEqual({ recoveryKey: RECOVERY_KEY, deliveryNonce: DELIVERY_NONCE });
  });

  it("ignores additive fields permitted by the versioned contract", () => {
    expect(
      parseDeliveredRecoveryKey({
        result: { recovery_key: RECOVERY_KEY, delivery_nonce: DELIVERY_NONCE, future_field: 1 },
        correlation_id: CORRELATION,
        future_field: "ignored",
      }),
    ).toStrictEqual({ recoveryKey: RECOVERY_KEY, deliveryNonce: DELIVERY_NONCE });
  });

  it.each([
    ["null", null],
    ["an array", [{ result: { recovery_key: RECOVERY_KEY, delivery_nonce: DELIVERY_NONCE } }]],
    ["a missing result", { correlation_id: CORRELATION }],
    ["a bare delivery", { recovery_key: RECOVERY_KEY, delivery_nonce: DELIVERY_NONCE }],
    ["a missing nonce", { result: { recovery_key: RECOVERY_KEY } }],
    ["a missing key", { result: { delivery_nonce: DELIVERY_NONCE } }],
    ["a non-string key", { result: { recovery_key: 1, delivery_nonce: DELIVERY_NONCE } }],
    [
      "a key outside the identity shape",
      { result: { recovery_key: "not-a-recovery-key", delivery_nonce: DELIVERY_NONCE } },
    ],
    [
      "a lowercase key",
      { result: { recovery_key: RECOVERY_KEY.toLowerCase(), delivery_nonce: DELIVERY_NONCE } },
    ],
    [
      "a nonce outside the token character set",
      { result: { recovery_key: RECOVERY_KEY, delivery_nonce: "abc def" } },
    ],
    [
      "an over-long nonce",
      { result: { recovery_key: RECOVERY_KEY, delivery_nonce: "a".repeat(49) } },
    ],
    ["an empty nonce", { result: { recovery_key: RECOVERY_KEY, delivery_nonce: "" } }],
  ])("rejects %s", (_label, payload) => {
    expect(parseDeliveredRecoveryKey(payload)).toBeNull();
  });
});

describe("isInitCompleted", () => {
  it("accepts the documented completion envelope", () => {
    expect(
      isInitCompleted({ result: { lifecycle: "initialized" }, correlation_id: CORRELATION }),
    ).toBe(true);
  });

  it.each([
    ["null", null],
    ["a missing result", { correlation_id: CORRELATION }],
    ["another lifecycle", { result: { lifecycle: "uninitialized" } }],
    ["a bare lifecycle", { lifecycle: "initialized" }],
  ])("rejects %s", (_label, payload) => {
    expect(isInitCompleted(payload)).toBe(false);
  });
});

describe("initRequestBody", () => {
  it("submits the database, the administrator, and two named log configurations", () => {
    expect(JSON.parse(initRequestBody(DETAILS, null))).toStrictEqual({
      database: { backend: "sqlite" },
      administrator: {
        username: "administrator",
        password: PASSWORD,
        display_name: "First Administrator",
      },
      log_modules: [
        {
          module: SQLITE_LOG_MODULE,
          name: SYSTEM_LOG_CONFIGURATION,
          enabled: true,
          settings: [],
          protected_settings: [],
        },
        {
          module: SQLITE_LOG_MODULE,
          name: AUDIT_LOG_CONFIGURATION,
          enabled: true,
          settings: [],
          protected_settings: [],
        },
      ],
      system_log: SYSTEM_LOG_CONFIGURATION,
      audit_log: AUDIT_LOG_CONFIGURATION,
    });
  });

  it("names the System Log and the Audit Log as two distinct configurations", () => {
    expect(SYSTEM_LOG_CONFIGURATION).not.toBe(AUDIT_LOG_CONFIGURATION);
  });

  it("omits the optional display name rather than submitting it empty", () => {
    const body = JSON.parse(initRequestBody({ ...DETAILS, displayName: "" }, null)) as {
      administrator: Record<string, unknown>;
    };

    expect("display_name" in body.administrator).toBe(false);
  });

  it("carries no proof on the preparation request", () => {
    expect(initRequestBody(DETAILS, null)).not.toContain("recovery_key_proof");
  });

  it("carries the proof on the finalization request", () => {
    const body = JSON.parse(initRequestBody(DETAILS, PROOF)) as Record<string, unknown>;

    expect(body.recovery_key_proof).toBe(PROOF);
  });

  it("stays within the route's request body bound for bounded field values", () => {
    const body = initRequestBody(
      {
        username: "u".repeat(64),
        displayName: "d".repeat(64),
        password: "p".repeat(192),
        systemLogModule: SQLITE_LOG_MODULE,
        auditLogModule: SQLITE_LOG_MODULE,
      },
      PROOF,
    );

    expect(new TextEncoder().encode(body).length).toBeLessThanOrEqual(1024);
  });
});

describe("prepareRecoveryKey", () => {
  it("submits a same-origin JSON PUT that sends no credentials and follows no redirect", async () => {
    const fetchMock = mockFetch(() => Promise.resolve(deliveryResponse()));

    await prepareRecoveryKey(DETAILS);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls[0]?.[0]).toBe(INIT_RECOVERY_KEY_PATH);
    const init = fetchMock.mock.calls[0]?.[1];
    expect(init?.method).toBe("PUT");
    expect(init?.credentials).toBe("omit");
    expect(init?.cache).toBe("no-store");
    expect(init?.redirect).toBe("error");
    expect(init?.headers).toStrictEqual({
      Accept: "application/json",
      "Content-Type": "application/json",
      "X-Weavelit-CSRF": "1",
    });
    expect(submittedBody(fetchMock).system_log).toBe(SYSTEM_LOG_CONFIGURATION);
  });

  it("returns the delivered key and nonce", async () => {
    mockFetch(() => Promise.resolve(deliveryResponse()));

    await expect(prepareRecoveryKey(DETAILS)).resolves.toStrictEqual({
      recoveryKey: RECOVERY_KEY,
      deliveryNonce: DELIVERY_NONCE,
    });
  });

  it.each([
    ["bad_request", 400],
    ["service_unavailable", 503],
    ["already_initialized", 409],
    ["request_origin_denied", 403],
    ["not_found", 404],
    ["method_not_allowed", 405],
    ["initialization_failed", 500],
  ])("reports the stable code of a %s rejection", async (code, status) => {
    mockFetch(() => Promise.resolve(jsonResponse({ error: code }, status)));

    const failure = await prepareRecoveryKey(DETAILS).catch((reason: unknown) => reason);

    expect(failure).toBeInstanceOf(InitFailedError);
    expect((failure as InitFailedError).code).toBe(code);
  });

  it("reports the unreported code when the transport fails", async () => {
    mockFetch(() => Promise.reject(new TypeError("network")));

    const failure = await prepareRecoveryKey(DETAILS).catch((reason: unknown) => reason);

    expect((failure as InitFailedError).code).toBe(UNREPORTED_FAILURE_CODE);
  });

  it.each([
    ["an unreadable body", () => new Response("not json", { status: 200 })],
    ["an envelope outside the contract", () => jsonResponse({ result: {} }, 200)],
    ["a rejection body outside the contract", () => jsonResponse({ detail: "denied" }, 400)],
  ])("reports the unreported code for %s", async (_label, response) => {
    mockFetch(() => Promise.resolve(response()));

    const failure = await prepareRecoveryKey(DETAILS).catch((reason: unknown) => reason);

    expect((failure as InitFailedError).code).toBe(UNREPORTED_FAILURE_CODE);
  });

  it("carries no submitted password on a failure", async () => {
    mockFetch(() => Promise.resolve(jsonResponse({ error: "bad_request" }, 400)));

    const failure = await prepareRecoveryKey(DETAILS).catch((reason: unknown) => reason);

    expect(JSON.stringify(failure, Object.getOwnPropertyNames(failure))).not.toContain(PASSWORD);
  });
});

describe("finalizeInit", () => {
  it("submits the proof to the finalization route and never the key itself", async () => {
    const fetchMock = mockFetch(() => Promise.resolve(completionResponse()));

    await finalizeInit(DETAILS, PROOF);

    expect(fetchMock.mock.calls[0]?.[0]).toBe(INIT_PATH);
    const body = submittedBody(fetchMock);
    expect(body.recovery_key_proof).toBe(PROOF);
    expect(submittedBodyText(fetchMock)).not.toContain(RECOVERY_KEY);
    expect(submittedBodyText(fetchMock)).not.toContain(DELIVERY_NONCE);
  });

  it("resolves on the documented completion envelope", async () => {
    mockFetch(() => Promise.resolve(completionResponse()));

    await expect(finalizeInit(DETAILS, PROOF)).resolves.toBeUndefined();
  });

  it.each([
    ["recovery_key_confirmation_required", 400],
    ["recovery_key_confirmation_invalid", 400],
    ["initialization_failed", 500],
    ["not_found", 404],
  ])("reports the stable code of a %s rejection", async (code, status) => {
    mockFetch(() => Promise.resolve(jsonResponse({ error: code }, status)));

    const failure = await finalizeInit(DETAILS, PROOF).catch((reason: unknown) => reason);

    expect((failure as InitFailedError).code).toBe(code);
  });

  it("rejects a success status carrying another lifecycle", async () => {
    mockFetch(() => Promise.resolve(jsonResponse({ result: { lifecycle: "uninitialized" } }, 200)));

    const failure = await finalizeInit(DETAILS, PROOF).catch((reason: unknown) => reason);

    expect((failure as InitFailedError).code).toBe(UNREPORTED_FAILURE_CODE);
  });
});
