/**
 * Same-origin client for the Server's shared authentication routes.
 *
 * The route contract, the two cookies, and the closed rejection vocabulary are
 * owned by `docs/server/authentication/authentication-design.md` and
 * `docs/server/api/api-contract-design.md`. This module restates none of them;
 * it issues exactly the two requests this application makes and reports only
 * what the presentation layer is permitted to know.
 */

/** The route that exchanges a username and password for a session. */
export const AUTH_LOGIN_PATH = "/api/v1/auth/login";

/** The route that reports the identity a presented session authenticates. */
export const AUTH_SESSION_PATH = "/api/v1/auth/session";

/** The Client Module this application requests a session for. */
export const CLIENT_MODULE = "web-ui";

/** The readable cookie the Server issues the per-session CSRF token in. */
export const CSRF_COOKIE_NAME = "__Host-weavelit_csrf";

/** The header a mutating request echoes the CSRF token in. */
export const CSRF_HEADER_NAME = "X-Weavelit-CSRF";

/**
 * The CSRF header value the login request carries.
 *
 * Login is the bootstrap request: no session exists yet, so no per-session
 * token exists to echo, and the route requires this literal instead. Every
 * later mutating request echoes the issued cookie.
 */
const PRE_SESSION_CSRF_VALUE = "1";

/** The closed shape of an issued opaque token, matching the cookie contract. */
const OPAQUE_TOKEN_PATTERN = /^[A-Za-z0-9_-]{1,48}$/;

/** The status a denied credential or an unusable session answers with. */
const UNAUTHORIZED_STATUS = 401;

/**
 * Failure of a login attempt, carrying no cause whatsoever.
 *
 * The Server answers every denied submission with one status and one stable
 * code, and deliberately cannot report which check failed. This type carries no
 * code, status, or diagnostic either, so the presentation layer has nothing to
 * branch on even if a future response tried to distinguish causes.
 */
export class LoginFailedError extends Error {
  constructor() {
    super("login_failed");
    this.name = "LoginFailedError";
  }
}

/**
 * What a session probe found, which is a fact about the served surface rather
 * than about any credential.
 *
 * `absent` means this deployment serves no authentication surface at all, which
 * is what a pre-operational deployment and an unreachable Server both look
 * like. It is never used to explain a denied login.
 */
export type SessionProbe =
  | { readonly kind: "authenticated" }
  | { readonly kind: "unauthenticated" }
  | { readonly kind: "absent" };

function objectPayload(payload: unknown): Record<string, unknown> | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return null;
  }
  return payload as Record<string, unknown>;
}

function typedResult(payload: unknown): Record<string, unknown> | null {
  return objectPayload(objectPayload(payload)?.result);
}

/** Reports whether a `200` envelope is the documented established-session one. */
export function isSessionEstablished(payload: unknown): boolean {
  return typedResult(payload)?.authenticated === true;
}

/**
 * Reports whether a `200` envelope is the documented session-identity one.
 *
 * The account identifier and the issuing Client Module are validated but not
 * returned: nothing in this application presents either, so neither is carried
 * any further than this check.
 */
export function isSessionIdentity(payload: unknown): boolean {
  const result = typedResult(payload);
  if (result === null) {
    return false;
  }
  const account = result.account_id;
  const clientModule = result.client_module;
  return (
    typeof account === "string" &&
    account.length > 0 &&
    typeof clientModule === "string" &&
    clientModule.length > 0
  );
}

/**
 * Returns the per-session CSRF token the Server issued in its readable cookie.
 *
 * The value is returned to the caller for one immediate use as a request
 * header. It is never stored, rendered, or placed in a URL. A cookie outside
 * the issued-token shape is discarded rather than echoed, so nothing a cookie
 * jar happens to hold can be reflected into a request header.
 */
export function readCsrfToken(): string | null {
  for (const pair of globalThis.document.cookie.split(";")) {
    const separator = pair.indexOf("=");
    if (separator < 0) {
      continue;
    }
    if (pair.slice(0, separator).trim() !== CSRF_COOKIE_NAME) {
      continue;
    }
    const value = pair.slice(separator + 1).trim();
    return OPAQUE_TOKEN_PATTERN.test(value) ? value : null;
  }
  return null;
}

/**
 * Submits a username and password and, on success, establishes a session.
 *
 * The credentials travel in the request body of this one request. Neither is
 * placed in the URL, a query string, a header, a cookie, or any browser
 * storage. The issued session and CSRF values are returned by the Server in
 * cookies alone and are never read from, or returned by, this function.
 *
 * Rejects with {@link LoginFailedError} for a transport failure, any response
 * other than the documented success, and every denial alike.
 */
export async function submitLogin(username: string, password: string): Promise<void> {
  let response: Response;
  try {
    response = await fetch(AUTH_LOGIN_PATH, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        [CSRF_HEADER_NAME]: PRE_SESSION_CSRF_VALUE,
      },
      body: JSON.stringify({ username, password, client_module: CLIENT_MODULE }),
      // The session and CSRF cookies the response issues are only stored when
      // the request participates in the cookie jar.
      credentials: "same-origin",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    throw new LoginFailedError();
  }

  if (response.status !== 200) {
    throw new LoginFailedError();
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    throw new LoginFailedError();
  }

  if (!isSessionEstablished(payload)) {
    throw new LoginFailedError();
  }
}

/**
 * Asks the Server whether the cookies this browser already holds authenticate.
 *
 * This is how a session that outlived a Server restart is recognised: the
 * answer comes from the Server's own durable session store rather than from
 * anything this application remembered.
 */
export async function probeSession(): Promise<SessionProbe> {
  const csrf = readCsrfToken();
  let response: Response;
  try {
    response = await fetch(AUTH_SESSION_PATH, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        ...(csrf === null ? {} : { [CSRF_HEADER_NAME]: csrf }),
      },
      credentials: "same-origin",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    return { kind: "absent" };
  }

  if (response.status === UNAUTHORIZED_STATUS) {
    return { kind: "unauthenticated" };
  }
  if (response.status !== 200) {
    return { kind: "absent" };
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    return { kind: "absent" };
  }

  return isSessionIdentity(payload) ? { kind: "authenticated" } : { kind: "absent" };
}
