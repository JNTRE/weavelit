/**
 * Same-origin client for the Web UI Client Module two-step Restore submission.
 *
 * The contract is owned by
 * `docs/client-modules/web-ui/pre-operational-restore-design.md`.
 */

/** The route that submits a recovery key and receives a one-time ticket. */
export const RESTORE_KEY_PATH = "/api/v1/restore";

/** The route that uploads the encrypted artifact against an issued ticket. */
export const RESTORE_ARTIFACT_PATH = "/api/v1/restore/artifact";

/** The only place an issued ticket is ever sent back to the Server. */
export const RESTORE_TICKET_HEADER = "X-Weavelit-Restore-Ticket";

/** The exact request media type of an encrypted artifact upload. */
export const ARTIFACT_MEDIA_TYPE = "application/octet-stream";

/** The status a recovery-key submission answers with when a ticket is issued. */
const TICKET_ISSUED_STATUS = 202;

/** The status a completed Restore answers with. */
const RESTORE_COMPLETED_STATUS = 200;

/** The lifecycle value a completed Restore reports. */
const LIFECYCLE_INITIALIZED = "initialized";

/**
 * The code presented for a failure that carried no stable code of its own.
 *
 * A transport failure, an unreadable body, and a response outside the contract
 * all present as this single documented code, so an unexpected response cannot
 * become a distinguishing signal or reach the browser as rendered detail.
 */
export const UNREPORTED_FAILURE_CODE = "restore_failed";

/** The closed shape of a stable, lowercase, dependency-neutral error code. */
const STABLE_CODE_PATTERN = /^[a-z0-9_]{1,48}$/;

/** The closed shape of an issued ticket: unpadded URL-safe Base64, bounded. */
const OPAQUE_TOKEN_PATTERN = /^[A-Za-z0-9_-]{1,48}$/;

/**
 * Failure of a Restore submission, carrying only a stable Server error code.
 *
 * The message is fixed. The `code` is either an error code the Server's own
 * closed vocabulary permits or {@link UNREPORTED_FAILURE_CODE}, so no server
 * message, field path, transport diagnostic, or response detail can reach the
 * browser presentation layer through this type.
 */
export class RestoreFailedError extends Error {
  readonly code: string;

  /**
   * Whether this failure reports no outcome for the request that produced it.
   *
   * Set only when this client never read the route's answer at all, which is
   * the one condition under which the route's own work may still run to
   * completion after the request was abandoned. A rejection the route reported
   * is a determinate answer whatever it rejected for, and is never marked.
   *
   * The distinction cannot be carried in the code, because
   * {@link UNREPORTED_FAILURE_CODE} is also a code the Server itself reports
   * for a determinate internal failure.
   */
  readonly indeterminate: boolean;

  /**
   * The issued capability for an artifact outcome the client could not read.
   *
   * It is absent for every determinate rejection and for failures before the
   * recovery-key route returned a valid issuance envelope.
   */
  readonly reconciliationCapability: string | undefined;

  constructor(code: string, indeterminate = false, reconciliationCapability?: string) {
    super("restore_failed");
    this.name = "RestoreFailedError";
    this.code = code;
    this.indeterminate = indeterminate;
    this.reconciliationCapability = reconciliationCapability;
  }
}

/** Builds the failure of a request whose outcome this client never read. */
function unreported(reconciliationCapability?: string): RestoreFailedError {
  return new RestoreFailedError(UNREPORTED_FAILURE_CODE, true, reconciliationCapability);
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
 * no code at all from one that reported {@link UNREPORTED_FAILURE_CODE}, which
 * the Server does report for a determinate internal failure of its own.
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

/** The two opaque values a valid recovery-key issuance returns. */
export interface IssuedRestoreTicket {
  readonly ticket: string;
  readonly reconciliationCapability: string;
}

/**
 * Returns the ticket and reconciliation capability a `202` envelope carries.
 *
 * Unknown additional fields are ignored, as the versioned contract permits
 * additive changes in `/api/v1/`.
 */
export function parseIssuedTicket(payload: unknown): IssuedRestoreTicket | null {
  const ticket = typedResult(payload)?.restore_ticket;
  const reconciliationCapability = typedResult(payload)?.reconciliation_capability;
  if (
    typeof ticket !== "string" ||
    !OPAQUE_TOKEN_PATTERN.test(ticket) ||
    typeof reconciliationCapability !== "string" ||
    !OPAQUE_TOKEN_PATTERN.test(reconciliationCapability)
  ) {
    return null;
  }
  return { ticket, reconciliationCapability };
}

/** Reports whether a `200` envelope is the documented completion envelope. */
export function isRestoreCompleted(payload: unknown): boolean {
  return typedResult(payload)?.lifecycle === LIFECYCLE_INITIALIZED;
}

async function rejection(
  response: Response,
  reconciliationCapability?: string,
): Promise<RestoreFailedError> {
  let reported: string | null;
  try {
    reported = reportedStableErrorCode(await response.json());
  } catch {
    return unreported(reconciliationCapability);
  }
  // A body outside the rejection contract reports no code of its own, so what
  // the route did with the request was never read from it either.
  if (reported === null) {
    return unreported(reconciliationCapability);
  }
  return reported === "gateway_timeout" && reconciliationCapability !== undefined
    ? new RestoreFailedError(reported, true, reconciliationCapability)
    : new RestoreFailedError(reported);
}

/**
 * Submits the recovery key alone and returns the one-time ticket it earns.
 *
 * The key travels in the request body of this request only. It is never placed
 * in the URL, a query string, a header, or a cookie, and it never travels with
 * the artifact.
 */
async function submitRecoveryKey(recoveryKey: string): Promise<IssuedRestoreTicket> {
  let response: Response;
  try {
    response = await fetch(RESTORE_KEY_PATH, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        "X-Weavelit-CSRF": "1",
      },
      body: JSON.stringify({ recovery_key: recoveryKey }),
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    throw unreported();
  }

  if (response.status !== TICKET_ISSUED_STATUS) {
    throw await rejection(response);
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    throw unreported();
  }

  const issued = parseIssuedTicket(payload);
  if (issued === null) {
    throw unreported();
  }
  return issued;
}

/**
 * Uploads the encrypted artifact against an issued ticket.
 *
 * The selected `File` is handed to `fetch` as the request body itself. It is
 * never read into a string, an `ArrayBuffer`, or an array first, so the
 * approved 256 MiB artifact bound is streamed from the browser's file-backed
 * storage instead of being copied through the JavaScript heap.
 *
 * `Content-Type` is set explicitly rather than inherited from the file, because
 * the route accepts exactly one unparameterized `application/octet-stream` and
 * a browser derives a file's type from its name.
 */
async function uploadArtifact(
  ticket: string,
  artifact: File,
  reconciliationCapability: string,
): Promise<void> {
  let response: Response;
  try {
    response = await fetch(RESTORE_ARTIFACT_PATH, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": ARTIFACT_MEDIA_TYPE,
        "X-Weavelit-CSRF": "1",
        [RESTORE_TICKET_HEADER]: ticket,
      },
      body: artifact,
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    throw unreported(reconciliationCapability);
  }

  if (response.status !== RESTORE_COMPLETED_STATUS) {
    throw await rejection(response, reconciliationCapability);
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    throw unreported(reconciliationCapability);
  }

  if (!isRestoreCompleted(payload)) {
    throw unreported(reconciliationCapability);
  }
}

/**
 * Runs one complete two-step Restore submission.
 *
 * The ticket exists only as a local binding of this call, so it is released
 * with the call and is never stored, rendered, or reused for a later attempt.
 *
 * Rejects with {@link RestoreFailedError} carrying the stable code of whichever
 * request failed.
 */
export async function submitRestore(recoveryKey: string, artifact: File): Promise<void> {
  const issued = await submitRecoveryKey(recoveryKey);
  await uploadArtifact(issued.ticket, artifact, issued.reconciliationCapability);
}
