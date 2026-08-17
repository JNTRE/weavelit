/**
 * Same-origin client for the Web UI Client Module two-request Init submission.
 *
 * The contract is owned by
 * `docs/client-modules/web-ui/pre-operational-init-design.md`.
 */

/** The route that submits the complete request and receives the one-time key. */
export const INIT_RECOVERY_KEY_PATH = "/api/v1/init/recovery-key";

/** The route that resubmits the request with the proof and completes Init. */
export const INIT_PATH = "/api/v1/init";

/** The one Log Module identifier this build compiles in. */
export const SQLITE_LOG_MODULE = "sqlite";

/**
 * The configuration names the two log assignments are submitted under.
 *
 * The Server refuses one configuration serving both log types, so the two
 * assignments are always two named configurations even when they name the same
 * module.
 */
export const SYSTEM_LOG_CONFIGURATION = "system-log";
export const AUDIT_LOG_CONFIGURATION = "audit-log";

/** The status both Init routes answer an accepted submission with. */
const ACCEPTED_STATUS = 200;

/** The lifecycle value a completed Init reports. */
const LIFECYCLE_INITIALIZED = "initialized";

/**
 * The code presented for a failure that carried no stable code of its own.
 *
 * A transport failure, an unreadable body, and a response outside the contract
 * all present as this single documented code. It is deliberately not one of the
 * Server's own codes, so an unexpected response cannot be mistaken for a
 * documented rejection or place arbitrary text on the page.
 */
export const UNREPORTED_FAILURE_CODE = "init_failed";

/** The closed shape of a stable, lowercase, dependency-neutral error code. */
const STABLE_CODE_PATTERN = /^[a-z0-9_]{1,48}$/;

/** The closed shape of one canonical uppercase age identity line. */
const RECOVERY_KEY_PATTERN = /^AGE-SECRET-KEY-1[0-9A-Z]{6,110}$/;

/** The closed shape of the delivery nonce: unpadded URL-safe Base64, bounded. */
const DELIVERY_NONCE_PATTERN = /^[A-Za-z0-9_-]{1,48}$/;

/** The closed shape of a submission-bound reconciliation capability. */
const RECONCILIATION_CAPABILITY_PATTERN = /^[A-Za-z0-9_-]{1,48}$/;

/** Everything a person supplies before a recovery key can be prepared. */
export interface InitDetails {
  readonly username: string;
  /** Empty when the optional display name is omitted from the request. */
  readonly displayName: string;
  readonly password: string;
  readonly systemLogModule: string;
  readonly auditLogModule: string;
}

/** The one response that ever carries a private recovery key. */
export interface DeliveredRecoveryKey {
  readonly recoveryKey: string;
  readonly deliveryNonce: string;
  /** Held only until a finalization outcome is known. */
  readonly reconciliationCapability: string;
}

/**
 * Failure of an Init submission, carrying only a stable Server error code.
 *
 * The message is fixed. The `code` is either an error code the Server's own
 * closed vocabulary permits or {@link UNREPORTED_FAILURE_CODE}, so no server
 * message, field path, transport diagnostic, or response detail can reach the
 * browser presentation layer through this type. No submitted password, no
 * delivered key, and no proof is ever carried on it.
 */
export class InitFailedError extends Error {
  readonly code: string;

  /**
   * Whether this failure reports no outcome for the request that produced it.
   *
   * Set only when this client never read the route's answer at all, which is
   * the one condition under which the route's own work may still run to
   * completion after the request was abandoned. A rejection the route reported
   * is a determinate answer whatever it rejected for, and is never marked.
   *
   * The distinction is carried here rather than in the code, because the code
   * is a fixed presentation value a caller can also reach for a failure that
   * never issued a request at all.
   */
  readonly indeterminate: boolean;

  constructor(code: string, indeterminate = false) {
    super("init_failed");
    this.name = "InitFailedError";
    this.code = code;
    this.indeterminate = indeterminate;
  }
}

/** Builds the failure of a request whose outcome this client never read. */
function unreported(): InitFailedError {
  return new InitFailedError(UNREPORTED_FAILURE_CODE, true);
}

function objectPayload(payload: unknown): Record<string, unknown> | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return null;
  }
  return payload as Record<string, unknown>;
}

/**
 * Returns the stable error code a rejection body reported, or `null`.
 *
 * Any value outside the closed stable-code shape is discarded rather than
 * rendered, so a body that is not the documented rejection contract cannot
 * place arbitrary text on the page. `null` distinguishes a body that reported
 * no code at all from one that reported a code of its own.
 */
function reportedStableErrorCode(payload: unknown): string | null {
  const error = objectPayload(payload)?.error;
  return typeof error === "string" && STABLE_CODE_PATTERN.test(error) ? error : null;
}

/** Returns the reported stable error code, or the fixed presented fallback. */
export function parseStableErrorCode(payload: unknown): string {
  return reportedStableErrorCode(payload) ?? UNREPORTED_FAILURE_CODE;
}

function typedResult(payload: unknown): Record<string, unknown> | null {
  return objectPayload(objectPayload(payload)?.result);
}

/**
 * Returns the delivered key a preparation envelope carries, or `null`.
 *
 * Unknown additional fields are ignored, as the versioned contract permits
 * additive changes in `/api/v1/`.
 */
export function parseDeliveredRecoveryKey(payload: unknown): DeliveredRecoveryKey | null {
  const result = typedResult(payload);
  const recoveryKey = result?.recovery_key;
  const deliveryNonce = result?.delivery_nonce;
  const reconciliationCapability = result?.reconciliation_capability;
  if (typeof recoveryKey !== "string" || !RECOVERY_KEY_PATTERN.test(recoveryKey)) {
    return null;
  }
  if (typeof deliveryNonce !== "string" || !DELIVERY_NONCE_PATTERN.test(deliveryNonce)) {
    return null;
  }
  if (
    typeof reconciliationCapability !== "string" ||
    !RECONCILIATION_CAPABILITY_PATTERN.test(reconciliationCapability)
  ) {
    return null;
  }
  return { recoveryKey, deliveryNonce, reconciliationCapability };
}

/** Reports whether a `200` envelope is the documented completion envelope. */
export function isInitCompleted(payload: unknown): boolean {
  return typedResult(payload)?.lifecycle === LIFECYCLE_INITIALIZED;
}

/**
 * Builds the exact request body both routes accept.
 *
 * The optional display name is omitted rather than sent empty, because the
 * route reports an absent display name and rejects an unusable one. The proof
 * member is present only on the finalization route, which is the only route
 * that accepts it.
 */
export function initRequestBody(details: InitDetails, proof: string | null): string {
  const administrator: Record<string, string> = {
    username: details.username,
    password: details.password,
  };
  if (details.displayName !== "") {
    administrator.display_name = details.displayName;
  }
  const body: Record<string, unknown> = {
    database: { backend: "sqlite" },
    administrator,
    log_modules: [
      {
        module: details.systemLogModule,
        name: SYSTEM_LOG_CONFIGURATION,
        enabled: true,
        settings: [],
        protected_settings: [],
      },
      {
        module: details.auditLogModule,
        name: AUDIT_LOG_CONFIGURATION,
        enabled: true,
        settings: [],
        protected_settings: [],
      },
    ],
    system_log: SYSTEM_LOG_CONFIGURATION,
    audit_log: AUDIT_LOG_CONFIGURATION,
  };
  if (proof !== null) {
    body.recovery_key_proof = proof;
  }
  return JSON.stringify(body);
}

async function rejection(response: Response): Promise<InitFailedError> {
  let reported: string | null;
  try {
    reported = reportedStableErrorCode(await response.json());
  } catch {
    return unreported();
  }
  // A body outside the rejection contract reports no code of its own, so what
  // the route did with the request was never read from it either.
  return reported === null ? unreported() : new InitFailedError(reported);
}

async function submit(path: string, body: string): Promise<unknown> {
  let response: Response;
  try {
    response = await fetch(path, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        "X-Weavelit-CSRF": "1",
      },
      body,
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    throw unreported();
  }

  if (response.status !== ACCEPTED_STATUS) {
    throw await rejection(response);
  }

  try {
    return await response.json();
  } catch {
    throw unreported();
  }
}

/**
 * Submits the complete initialization request and returns the delivered key.
 *
 * The password travels in this request body only. Nothing this call returns is
 * stored, and the delivered key is handed straight back to the caller that must
 * display it once.
 *
 * Rejects with {@link InitFailedError} carrying the route's stable code.
 */
export async function prepareRecoveryKey(details: InitDetails): Promise<DeliveredRecoveryKey> {
  const delivered = parseDeliveredRecoveryKey(
    await submit(INIT_RECOVERY_KEY_PATH, initRequestBody(details, null)),
  );
  if (delivered === null) {
    throw unreported();
  }
  return delivered;
}

/**
 * Resubmits the request with the proof of possession and completes Init.
 *
 * The private recovery key is never a parameter of this call: only the proof
 * derived from it is submitted, so no request this module issues can carry the
 * delivered key back to the Server.
 *
 * Rejects with {@link InitFailedError} carrying the route's stable code.
 */
export async function finalizeInit(details: InitDetails, proof: string): Promise<void> {
  if (!isInitCompleted(await submit(INIT_PATH, initRequestBody(details, proof)))) {
    throw unreported();
  }
}
