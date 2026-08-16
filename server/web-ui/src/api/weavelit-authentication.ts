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

/** The route that submits a second-factor code against a login continuation. */
export const AUTH_MFA_VERIFY_PATH = "/api/v1/auth/mfa/verify";

/** The route that opens an enrollment from a login continuation. */
export const AUTH_MFA_ENROLLMENT_PATH = "/api/v1/auth/mfa/enrollment";

/** The route that confirms an opened enrollment with a code from its secret. */
export const AUTH_MFA_ENROLLMENT_CONFIRM_PATH = "/api/v1/auth/mfa/enrollment/confirm";

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

/**
 * The closed shape of a disclosed provisioning URI.
 *
 * It restates the character set and bound the Server's typed response profile
 * already enforces, so a value outside that profile is discarded here rather
 * than rendered.
 */
const PROVISIONING_URI_PATTERN = /^[A-Za-z0-9\-._~%:/?&=]{1,288}$/;

/** The status a denied credential or an unusable session answers with. */
const UNAUTHORIZED_STATUS = 401;

/** The status a login that verified a password but issued no session answers with. */
const CONTINUATION_STATUS = 202;

/** The status the listener returns when a request handler outlives its deadline. */
const GATEWAY_TIMEOUT_STATUS = 504;

/** The stable error code the listener returns with a gateway timeout. */
const GATEWAY_TIMEOUT_CODE = "gateway_timeout";

/** The closed shape of a stable, dependency-neutral Server error code. */
const STABLE_ERROR_CODE_PATTERN = /^[a-z0-9_]{1,48}$/;

/** The stage naming a login that must present an already enrolled factor. */
export const MFA_REQUIRED_STAGE = "mfa_required";

/** The stage naming a login that must enroll a factor before it may proceed. */
export const MFA_ENROLLMENT_REQUIRED_STAGE = "mfa_enrollment_required";

/** Decimal digits in one submitted second-factor code. */
export const MFA_CODE_DIGITS = 6;

/** The only code shape the second-factor routes accept. */
export const MFA_CODE_PATTERN = /^[0-9]{6}$/;

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
 * A session-establishing authentication request whose outcome is not known.
 *
 * This holds no response or transport detail. The presentation layer may only
 * reconcile it through the ordinary session route, because a session cookie
 * could have been committed before the browser could read the response.
 */
export class IndeterminateAuthenticationError extends Error {
  constructor() {
    super("authentication_indeterminate");
    this.name = "IndeterminateAuthenticationError";
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

/**
 * What a verified password was admitted to.
 *
 * `session` means the login is complete. The other two carry a continuation,
 * which is the only thing binding a following second-factor request to this
 * verified password. A continuation is spent by the first request that presents
 * it, whether or not that request was accepted, so it resumes exactly one
 * attempt and never a retry.
 */
export type LoginOutcome =
  | { readonly kind: "session" }
  | { readonly kind: "mfa_required"; readonly continuation: string }
  | { readonly kind: "mfa_enrollment_required"; readonly continuation: string };

/**
 * The provisioning data an opened enrollment discloses.
 *
 * The Server returns these in exactly one response and can never return them
 * again, so a caller that loses them has no way to recover them and must open a
 * new enrollment. They are returned to the caller for immediate presentation
 * and are never stored, placed in a URL, or written to any browser storage by
 * this module.
 */
export interface EnrollmentOpened {
  readonly secret: string;
  readonly provisioningUri: string;
  /** The one-time value that confirms this enrollment, spent by one request. */
  readonly enrollment: string;
}

function objectPayload(payload: unknown): Record<string, unknown> | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return null;
  }
  return payload as Record<string, unknown>;
}

function typedResult(payload: unknown): Record<string, unknown> | null {
  return objectPayload(objectPayload(payload)?.result);
}

/** Returns a documented stable error code, or `null` for any other body. */
function reportedStableErrorCode(payload: unknown): string | null {
  const error = objectPayload(payload)?.error;
  return typeof error === "string" && STABLE_ERROR_CODE_PATTERN.test(error) ? error : null;
}

/** Reports whether the listener's stable timeout envelope leaves a commit unknown. */
async function isGatewayTimeout(response: Response): Promise<boolean> {
  if (response.status !== GATEWAY_TIMEOUT_STATUS) {
    return false;
  }
  try {
    return reportedStableErrorCode(await response.json()) === GATEWAY_TIMEOUT_CODE;
  } catch {
    return false;
  }
}

/** Reports whether a `200` envelope is the documented established-session one. */
export function isSessionEstablished(payload: unknown): boolean {
  return typedResult(payload)?.authenticated === true;
}

function opaqueToken(value: unknown): string | null {
  return typeof value === "string" && OPAQUE_TOKEN_PATTERN.test(value) ? value : null;
}

/**
 * Reads the documented `202` envelope, or `null` for anything else.
 *
 * Only the two documented stages are accepted, so an envelope naming a stage
 * this application cannot present is a failure rather than a step it guesses at.
 */
export function readLoginContinuation(payload: unknown): LoginOutcome | null {
  const result = typedResult(payload);
  if (result === null) {
    return null;
  }
  const continuation = opaqueToken(result.continuation);
  if (continuation === null) {
    return null;
  }
  if (result.mfa === MFA_REQUIRED_STAGE) {
    return { kind: "mfa_required", continuation };
  }
  if (result.mfa === MFA_ENROLLMENT_REQUIRED_STAGE) {
    return { kind: "mfa_enrollment_required", continuation };
  }
  return null;
}

/** Reads the documented opened-enrollment envelope, or `null` for anything else. */
export function readEnrollmentOpened(payload: unknown): EnrollmentOpened | null {
  const result = typedResult(payload);
  if (result === null) {
    return null;
  }
  const secret = opaqueToken(result.secret);
  const enrollment = opaqueToken(result.enrollment);
  const provisioningUri = result.provisioning_uri;
  if (
    secret === null ||
    enrollment === null ||
    typeof provisioningUri !== "string" ||
    !PROVISIONING_URI_PATTERN.test(provisioningUri)
  ) {
    return null;
  }
  return { secret, provisioningUri, enrollment };
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
 * Submits a username and password and reports what it was admitted to.
 *
 * The credentials travel in the request body of this one request. Neither is
 * placed in the URL, a query string, a header, a cookie, or any browser
 * storage. The issued session and CSRF values are returned by the Server in
 * cookies alone and are never read from, or returned by, this function.
 *
 * A verified password that still owes a second factor answers with a
 * continuation and no cookie at all, so this function returning anything other
 * than `session` means no session exists yet.
 *
 * A transport failure, the listener's documented gateway timeout, or an
 * unreadable or invalid session-establishing `200` rejects with
 * {@link IndeterminateAuthenticationError}; callers reconcile only through
 * {@link probeSession}. Every reported stable rejection remains a
 * {@link LoginFailedError}.
 */
export async function submitLogin(username: string, password: string): Promise<LoginOutcome> {
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
    throw new IndeterminateAuthenticationError();
  }

  if (response.status !== 200 && response.status !== CONTINUATION_STATUS) {
    if (await isGatewayTimeout(response)) {
      throw new IndeterminateAuthenticationError();
    }
    throw new LoginFailedError();
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    throw response.status === 200 ? new IndeterminateAuthenticationError() : new LoginFailedError();
  }

  if (response.status === CONTINUATION_STATUS) {
    const outcome = readLoginContinuation(payload);
    if (outcome === null) {
      throw new LoginFailedError();
    }
    return outcome;
  }

  if (!isSessionEstablished(payload)) {
    throw new IndeterminateAuthenticationError();
  }
  return { kind: "session" };
}

/**
 * Issues one continuation-bearing request and returns its parsed `200` body.
 *
 * These requests carry no session, exactly as login does not: the one-time
 * value in the body is the only thing binding them to an earlier verified
 * password, and it is carried in the body alone rather than in the target.
 * They therefore echo the same pre-session literal login does.
 */
async function submitContinuation(
  path: string,
  body: Readonly<Record<string, string>>,
): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(path, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        [CSRF_HEADER_NAME]: PRE_SESSION_CSRF_VALUE,
      },
      body: JSON.stringify(body),
      credentials: "same-origin",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    throw new IndeterminateAuthenticationError();
  }

  if (response.status !== 200) {
    if (await isGatewayTimeout(response)) {
      throw new IndeterminateAuthenticationError();
    }
    throw new LoginFailedError();
  }

  try {
    return await response.json();
  } catch {
    throw new IndeterminateAuthenticationError();
  }
}

/**
 * Submits a second-factor code against a login continuation.
 *
 * The continuation is spent by this request whether or not the code was
 * accepted, so a rejection ends the attempt it resumed rather than inviting
 * another code against the same continuation.
 *
 * A known non-200 refusal rejects with {@link LoginFailedError}; its cause
 * remains opaque. A transport failure or unreadable or invalid 200 success
 * envelope rejects with {@link IndeterminateAuthenticationError}; callers must
 * use {@link probeSession} to reconcile whether a session was established,
 * with the cause remaining opaque.
 */
export async function submitSecondFactor(continuation: string, code: string): Promise<void> {
  const payload = await submitContinuation(AUTH_MFA_VERIFY_PATH, { continuation, code });
  if (!isSessionEstablished(payload)) {
    throw new IndeterminateAuthenticationError();
  }
}

/**
 * Opens an enrollment against a login continuation and returns its one
 * disclosure.
 *
 * This is the only response that ever carries the secret and the provisioning
 * URI. The values are handed straight back to the caller and are neither
 * retained here nor recoverable by repeating this call, because the
 * continuation it spent is gone.
 *
 * Rejects with {@link LoginFailedError} for every refusal alike.
 */
export async function openEnrollment(continuation: string): Promise<EnrollmentOpened> {
  let payload: unknown;
  try {
    payload = await submitContinuation(AUTH_MFA_ENROLLMENT_PATH, { continuation });
  } catch {
    // Opening enrollment cannot establish a session, so preserving the
    // existing generic rejection is truthful and needs no reconciliation.
    throw new LoginFailedError();
  }
  const opened = readEnrollmentOpened(payload);
  if (opened === null) {
    throw new LoginFailedError();
  }
  return opened;
}

/**
 * Confirms an opened enrollment with a code from its new secret.
 *
 * The enrollment value is spent by this request whether or not the code was
 * accepted, so a rejection ends the attempt and leaves no factor behind.
 *
 * A known non-200 refusal rejects with {@link LoginFailedError}; its cause
 * remains opaque. A transport failure or unreadable or invalid 200 success
 * envelope rejects with {@link IndeterminateAuthenticationError}; callers must
 * use {@link probeSession} to reconcile whether a session was established,
 * with the cause remaining opaque.
 */
export async function confirmEnrollment(enrollment: string, code: string): Promise<void> {
  const payload = await submitContinuation(AUTH_MFA_ENROLLMENT_CONFIRM_PATH, {
    enrollment,
    code,
  });
  if (!isSessionEstablished(payload)) {
    throw new IndeterminateAuthenticationError();
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
