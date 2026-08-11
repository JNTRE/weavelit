import { afterEach, describe, expect, it, vi } from "vitest";

import {
  AUTH_LOGIN_PATH,
  AUTH_MFA_ENROLLMENT_CONFIRM_PATH,
  AUTH_MFA_ENROLLMENT_PATH,
  AUTH_MFA_VERIFY_PATH,
  AUTH_SESSION_PATH,
  CSRF_COOKIE_NAME,
  CSRF_HEADER_NAME,
  LoginFailedError,
  confirmEnrollment,
  isSessionEstablished,
  isSessionIdentity,
  openEnrollment,
  probeSession,
  readCsrfToken,
  readEnrollmentOpened,
  readLoginContinuation,
  submitLogin,
  submitSecondFactor,
} from "./weavelit-authentication";

const CORRELATION = "0123456789abcdef0123456789abcdef";
const CSRF_TOKEN = "0123456789abcdefghijklmnopqrstuvwxyzABC-_";
const ACCOUNT = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const USERNAME = "administrator";
const PASSWORD = "fixture-administrator-password";
const CONTINUATION = "Y29udGludWF0aW9uLXZhbHVlLWZvci10ZXN0aW5n";
const ENROLLMENT = "ZW5yb2xsbWVudC12YWx1ZS1mb3ItdGVzdGluZw";
const SECRET = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
const PROVISIONING_URI =
  "otpauth://totp/Weavelit:administrator?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ" +
  "&issuer=Weavelit&algorithm=SHA1&digits=6&period=30";
const CODE = "123456";

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function establishedResponse(): Response {
  return jsonResponse({ result: { authenticated: true }, correlation_id: CORRELATION }, 200);
}

function identityResponse(): Response {
  return jsonResponse(
    { result: { account_id: ACCOUNT, client_module: "web-ui" }, correlation_id: CORRELATION },
    200,
  );
}

function rejectionResponse(code: string, status: number): Response {
  return jsonResponse({ error: code, correlation_id: CORRELATION }, status);
}

function continuationResponse(stage: string, continuation = CONTINUATION): Response {
  return jsonResponse({ result: { mfa: stage, continuation }, correlation_id: CORRELATION }, 202);
}

function enrollmentOpenedResponse(): Response {
  return jsonResponse(
    {
      result: { secret: SECRET, provisioning_uri: PROVISIONING_URI, enrollment: ENROLLMENT },
      correlation_id: CORRELATION,
    },
    200,
  );
}

/** Replaces the document's cookie jar for one test. */
function withCookies(cookies: string): void {
  Object.defineProperty(globalThis.document, "cookie", {
    configurable: true,
    get: () => cookies,
  });
}

afterEach(() => {
  Reflect.deleteProperty(globalThis.document, "cookie");
});

describe("readCsrfToken", () => {
  it("returns the issued token from the readable cookie", () => {
    withCookies(`${CSRF_COOKIE_NAME}=${CSRF_TOKEN}`);
    expect(readCsrfToken()).toBe(CSRF_TOKEN);
  });

  it("finds the token among other cookies", () => {
    withCookies(`unrelated=1; ${CSRF_COOKIE_NAME}=${CSRF_TOKEN}; another=2`);
    expect(readCsrfToken()).toBe(CSRF_TOKEN);
  });

  it.each([
    ["an empty jar", ""],
    ["an unrelated cookie", "unrelated=1"],
    ["an unprefixed cookie of the same name", `weavelit_csrf=${CSRF_TOKEN}`],
    ["a valueless pair", CSRF_COOKIE_NAME],
    ["a value outside the token character set", `${CSRF_COOKIE_NAME}=has space`],
    ["an over-long value", `${CSRF_COOKIE_NAME}=${"a".repeat(49)}`],
    ["an empty value", `${CSRF_COOKIE_NAME}=`],
  ])("returns null for %s", (_label, cookies) => {
    withCookies(cookies);
    expect(readCsrfToken()).toBeNull();
  });
});

describe("isSessionEstablished", () => {
  it("accepts the documented established-session envelope", () => {
    expect(
      isSessionEstablished({ result: { authenticated: true }, correlation_id: CORRELATION }),
    ).toBe(true);
  });

  it.each([
    ["null", null],
    ["an array", [{ result: { authenticated: true } }]],
    ["a bare flag", { authenticated: true }],
    ["a missing result", { correlation_id: CORRELATION }],
    ["a false flag", { result: { authenticated: false } }],
    ["a truthy non-boolean flag", { result: { authenticated: "true" } }],
  ])("rejects %s", (_label, payload) => {
    expect(isSessionEstablished(payload)).toBe(false);
  });
});

describe("isSessionIdentity", () => {
  it("accepts the documented identity envelope", () => {
    expect(
      isSessionIdentity({
        result: { account_id: ACCOUNT, client_module: "web-ui" },
        correlation_id: CORRELATION,
      }),
    ).toBe(true);
  });

  it("ignores additive fields permitted by the versioned contract", () => {
    expect(
      isSessionIdentity({
        result: { account_id: ACCOUNT, client_module: "web-ui", future_field: 1 },
        correlation_id: CORRELATION,
      }),
    ).toBe(true);
  });

  it.each([
    ["null", null],
    ["an array result", { result: [ACCOUNT] }],
    ["a missing account", { result: { client_module: "web-ui" } }],
    ["a missing Client Module", { result: { account_id: ACCOUNT } }],
    ["an empty account", { result: { account_id: "", client_module: "web-ui" } }],
    ["a non-string account", { result: { account_id: 1, client_module: "web-ui" } }],
  ])("rejects %s", (_label, payload) => {
    expect(isSessionIdentity(payload)).toBe(false);
  });
});

describe("submitLogin", () => {
  it("submits the credentials in the request body of a same-origin PUT", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(establishedResponse());

    await submitLogin(USERNAME, PASSWORD);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).toBe(AUTH_LOGIN_PATH);
    expect(init.method).toBe("PUT");
    expect(init.credentials).toBe("same-origin");
    expect(init.cache).toBe("no-store");
    expect(init.redirect).toBe("error");
    expect(init.body).toBe(
      JSON.stringify({ username: USERNAME, password: PASSWORD, client_module: "web-ui" }),
    );
    // The bootstrap request carries the pre-session literal, because no
    // per-session token exists yet to echo.
    expect((init.headers as Record<string, string>)[CSRF_HEADER_NAME]).toBe("1");
    expect((init.headers as Record<string, string>)["Content-Type"]).toBe("application/json");
  });

  it("places no credential in the request target", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(establishedResponse());

    await submitLogin(USERNAME, PASSWORD);

    const [target] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).not.toContain(PASSWORD);
    expect(target).not.toContain(USERNAME);
  });

  it.each([
    ["a denied credential", () => Promise.resolve(rejectionResponse("authentication_failed", 401))],
    ["a malformed submission", () => Promise.resolve(rejectionResponse("bad_request", 400))],
    ["a denied origin", () => Promise.resolve(rejectionResponse("request_origin_denied", 403))],
    [
      "an unavailable service",
      () => Promise.resolve(rejectionResponse("service_unavailable", 503)),
    ],
    ["an unparseable body", () => Promise.resolve(new Response("not json", { status: 200 }))],
    ["an undocumented success body", () => Promise.resolve(jsonResponse({ result: {} }, 200))],
    ["a transport failure", () => Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443"))],
  ])("rejects %s with one indistinguishable failure", async (_label, response) => {
    vi.spyOn(globalThis, "fetch").mockImplementation(response);

    const failure = await submitLogin(USERNAME, PASSWORD).catch((reason: unknown) => reason);

    expect(failure).toBeInstanceOf(LoginFailedError);
    const error = failure as LoginFailedError;
    expect(error.message).toBe("login_failed");
    // No cause reaches the caller: every own field of the failure is a fixed
    // constant, so nothing derived from the response can be recovered from it.
    expect(Object.keys(error)).toEqual(["name"]);
    expect(JSON.stringify(error)).toBe('{"name":"LoginFailedError"}');
  });

  it("reports an established session for the documented 200", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(establishedResponse());

    expect(await submitLogin(USERNAME, PASSWORD)).toEqual({ kind: "session" });
  });

  it.each([
    ["mfa_required", "mfa_required"],
    ["mfa_enrollment_required", "mfa_enrollment_required"],
  ])("reports the %s continuation the 202 carries", async (_label, stage) => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(continuationResponse(stage));

    expect(await submitLogin(USERNAME, PASSWORD)).toEqual({
      kind: stage,
      continuation: CONTINUATION,
    });
  });

  it.each([
    ["an unknown stage", () => Promise.resolve(continuationResponse("password_required"))],
    [
      "a missing stage",
      () => Promise.resolve(jsonResponse({ result: { continuation: "a" } }, 202)),
    ],
    ["a missing continuation", () => Promise.resolve(jsonResponse({ result: { mfa: "x" } }, 202))],
    [
      "a continuation outside the issued token shape",
      () => Promise.resolve(continuationResponse("mfa_required", "not a token")),
    ],
    ["an unparseable 202 body", () => Promise.resolve(new Response("not json", { status: 202 }))],
  ])("rejects %s with the same failure a denial produces", async (_label, response) => {
    vi.spyOn(globalThis, "fetch").mockImplementation(response);

    await expect(submitLogin(USERNAME, PASSWORD)).rejects.toBeInstanceOf(LoginFailedError);
  });
});

describe("readLoginContinuation", () => {
  it.each([["mfa_required"], ["mfa_enrollment_required"]])(
    "accepts the documented %s envelope",
    (stage) => {
      expect(
        readLoginContinuation({
          result: { mfa: stage, continuation: CONTINUATION },
          correlation_id: CORRELATION,
        }),
      ).toEqual({ kind: stage, continuation: CONTINUATION });
    },
  );

  it.each([
    ["null", null],
    ["an array", [{ result: { mfa: "mfa_required", continuation: CONTINUATION } }]],
    ["a bare result", { mfa: "mfa_required", continuation: CONTINUATION }],
    ["an unknown stage", { result: { mfa: "elevation_required", continuation: CONTINUATION } }],
    ["a non-string stage", { result: { mfa: 1, continuation: CONTINUATION } }],
    ["an empty continuation", { result: { mfa: "mfa_required", continuation: "" } }],
    [
      "a continuation outside the token character set",
      { result: { mfa: "mfa_required", continuation: "has space" } },
    ],
    [
      "an over-long continuation",
      { result: { mfa: "mfa_required", continuation: "a".repeat(49) } },
    ],
  ])("rejects %s", (_label, payload) => {
    expect(readLoginContinuation(payload)).toBeNull();
  });
});

describe("readEnrollmentOpened", () => {
  it("accepts the documented disclosure envelope", () => {
    expect(
      readEnrollmentOpened({
        result: { secret: SECRET, provisioning_uri: PROVISIONING_URI, enrollment: ENROLLMENT },
        correlation_id: CORRELATION,
      }),
    ).toEqual({ secret: SECRET, provisioningUri: PROVISIONING_URI, enrollment: ENROLLMENT });
  });

  it.each([
    ["null", null],
    [
      "a missing secret",
      { result: { provisioning_uri: PROVISIONING_URI, enrollment: ENROLLMENT } },
    ],
    ["a missing URI", { result: { secret: SECRET, enrollment: ENROLLMENT } }],
    ["a missing enrollment", { result: { secret: SECRET, provisioning_uri: PROVISIONING_URI } }],
    [
      "a secret outside the token character set",
      {
        result: { secret: "has space", provisioning_uri: PROVISIONING_URI, enrollment: ENROLLMENT },
      },
    ],
    [
      "a URI outside the provisioning character set",
      {
        result: {
          secret: SECRET,
          provisioning_uri: "otpauth://totp/<script>",
          enrollment: ENROLLMENT,
        },
      },
    ],
    [
      "an over-long URI",
      {
        result: {
          secret: SECRET,
          provisioning_uri: `otpauth://${"a".repeat(289)}`,
          enrollment: ENROLLMENT,
        },
      },
    ],
    [
      "a non-string URI",
      { result: { secret: SECRET, provisioning_uri: 1, enrollment: ENROLLMENT } },
    ],
  ])("rejects %s", (_label, payload) => {
    expect(readEnrollmentOpened(payload)).toBeNull();
  });
});

describe("submitSecondFactor", () => {
  it("submits the continuation and the code in the body of a same-origin PUT", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(establishedResponse());

    await submitSecondFactor(CONTINUATION, CODE);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).toBe(AUTH_MFA_VERIFY_PATH);
    expect(init.method).toBe("PUT");
    expect(init.credentials).toBe("same-origin");
    expect(init.cache).toBe("no-store");
    expect(init.redirect).toBe("error");
    expect(init.body).toBe(JSON.stringify({ continuation: CONTINUATION, code: CODE }));
    // The route carries no session, so it echoes the same pre-session literal
    // the bootstrap login request does.
    expect((init.headers as Record<string, string>)[CSRF_HEADER_NAME]).toBe("1");
    expect(target).not.toContain(CONTINUATION);
    expect(target).not.toContain(CODE);
  });

  it.each([
    ["a refused code", () => Promise.resolve(rejectionResponse("authentication_failed", 401))],
    [
      "a spent continuation",
      () => Promise.resolve(rejectionResponse("authentication_failed", 401)),
    ],
    ["a malformed submission", () => Promise.resolve(rejectionResponse("bad_request", 400))],
    ["a continuation answer", () => Promise.resolve(continuationResponse("mfa_required"))],
    ["an undocumented success body", () => Promise.resolve(jsonResponse({ result: {} }, 200))],
    ["an unparseable body", () => Promise.resolve(new Response("not json", { status: 200 }))],
    ["a transport failure", () => Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443"))],
  ])("rejects %s with one indistinguishable failure", async (_label, response) => {
    vi.spyOn(globalThis, "fetch").mockImplementation(response);

    const failure = await submitSecondFactor(CONTINUATION, CODE).catch((reason: unknown) => reason);

    expect(failure).toBeInstanceOf(LoginFailedError);
    expect(JSON.stringify(failure)).toBe('{"name":"LoginFailedError"}');
  });
});

describe("openEnrollment", () => {
  it("submits the continuation alone and returns the one disclosure", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(enrollmentOpenedResponse());

    const opened = await openEnrollment(CONTINUATION);

    expect(opened).toEqual({
      secret: SECRET,
      provisioningUri: PROVISIONING_URI,
      enrollment: ENROLLMENT,
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).toBe(AUTH_MFA_ENROLLMENT_PATH);
    expect(init.method).toBe("PUT");
    expect(init.body).toBe(JSON.stringify({ continuation: CONTINUATION }));
    expect(target).not.toContain(CONTINUATION);
  });

  it("writes no disclosed value to browser storage or the cookie jar", async () => {
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(enrollmentOpenedResponse());

    await openEnrollment(CONTINUATION);

    expect(globalThis.localStorage.length).toBe(0);
    expect(globalThis.sessionStorage.length).toBe(0);
    expect(globalThis.document.cookie).not.toContain(SECRET);
    expect(globalThis.location.href).not.toContain(SECRET);
    expect(globalThis.location.href).not.toContain(PROVISIONING_URI);
  });

  it.each([
    [
      "a spent continuation",
      () => Promise.resolve(rejectionResponse("authentication_failed", 401)),
    ],
    ["a malformed submission", () => Promise.resolve(rejectionResponse("bad_request", 400))],
    [
      "a disclosure missing its enrollment value",
      () =>
        Promise.resolve(
          jsonResponse({ result: { secret: SECRET, provisioning_uri: PROVISIONING_URI } }, 200),
        ),
    ],
    ["an established-session body", () => Promise.resolve(establishedResponse())],
    ["an unparseable body", () => Promise.resolve(new Response("not json", { status: 200 }))],
    ["a transport failure", () => Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443"))],
  ])("rejects %s with one indistinguishable failure", async (_label, response) => {
    vi.spyOn(globalThis, "fetch").mockImplementation(response);

    const failure = await openEnrollment(CONTINUATION).catch((reason: unknown) => reason);

    expect(failure).toBeInstanceOf(LoginFailedError);
    expect(JSON.stringify(failure)).toBe('{"name":"LoginFailedError"}');
  });
});

describe("confirmEnrollment", () => {
  it("submits the enrollment value and the code in the body of a same-origin PUT", async () => {
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(establishedResponse());

    await confirmEnrollment(ENROLLMENT, CODE);

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).toBe(AUTH_MFA_ENROLLMENT_CONFIRM_PATH);
    expect(init.method).toBe("PUT");
    expect(init.credentials).toBe("same-origin");
    expect(init.body).toBe(JSON.stringify({ enrollment: ENROLLMENT, code: CODE }));
    expect((init.headers as Record<string, string>)[CSRF_HEADER_NAME]).toBe("1");
    expect(target).not.toContain(ENROLLMENT);
  });

  it.each([
    ["a refused code", () => Promise.resolve(rejectionResponse("authentication_failed", 401))],
    ["a malformed submission", () => Promise.resolve(rejectionResponse("bad_request", 400))],
    ["an undocumented success body", () => Promise.resolve(jsonResponse({ result: {} }, 200))],
    ["an unparseable body", () => Promise.resolve(new Response("not json", { status: 200 }))],
    ["a transport failure", () => Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443"))],
  ])("rejects %s with one indistinguishable failure", async (_label, response) => {
    vi.spyOn(globalThis, "fetch").mockImplementation(response);

    const failure = await confirmEnrollment(ENROLLMENT, CODE).catch((reason: unknown) => reason);

    expect(failure).toBeInstanceOf(LoginFailedError);
    expect(JSON.stringify(failure)).toBe('{"name":"LoginFailedError"}');
  });
});

describe("probeSession", () => {
  it("echoes the readable CSRF cookie in the request header", async () => {
    withCookies(`${CSRF_COOKIE_NAME}=${CSRF_TOKEN}`);
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(identityResponse());

    expect(await probeSession()).toEqual({ kind: "authenticated" });

    const [target, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(target).toBe(AUTH_SESSION_PATH);
    expect(init.method).toBe("PUT");
    expect(init.credentials).toBe("same-origin");
    expect(init.body).toBeUndefined();
    expect((init.headers as Record<string, string>)[CSRF_HEADER_NAME]).toBe(CSRF_TOKEN);
    // The token is echoed in a header alone: never in the target, and never
    // written back into any storage.
    expect(target).not.toContain(CSRF_TOKEN);
  });

  it("omits the header when no usable token cookie is readable", async () => {
    withCookies("unrelated=1");
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(rejectionResponse("session_invalid", 401));

    expect(await probeSession()).toEqual({ kind: "unauthenticated" });

    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect((init.headers as Record<string, string>)[CSRF_HEADER_NAME]).toBeUndefined();
  });

  it.each([
    [
      "an invalid session",
      () => Promise.resolve(rejectionResponse("session_invalid", 401)),
      { kind: "unauthenticated" },
    ],
    [
      "an absent route",
      () => Promise.resolve(jsonResponse({ error: "not_found" }, 404)),
      { kind: "absent" },
    ],
    [
      "a denied origin",
      () => Promise.resolve(rejectionResponse("request_origin_denied", 403)),
      { kind: "absent" },
    ],
    [
      "a transport failure",
      () => Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443")),
      { kind: "absent" },
    ],
    [
      "an unparseable body",
      () => Promise.resolve(new Response("not json", { status: 200 })),
      { kind: "absent" },
    ],
    [
      "an undocumented identity body",
      () => Promise.resolve(jsonResponse({ result: { account_id: 1 } }, 200)),
      { kind: "absent" },
    ],
  ])("reports %s", async (_label, response, expected) => {
    withCookies("");
    vi.spyOn(globalThis, "fetch").mockImplementation(response);

    expect(await probeSession()).toEqual(expected);
  });
});
