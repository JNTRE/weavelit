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

  constructor(code: string) {
    super("restore_failed");
    this.name = "RestoreFailedError";
    this.code = code;
  }
}

function objectPayload(payload: unknown): Record<string, unknown> | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return null;
  }
  return payload as Record<string, unknown>;
}

/**
 * Returns the stable error code a rejection body reported.
 *
 * Any value outside the closed stable-code shape is discarded rather than
 * rendered, so a body that is not the documented rejection contract cannot
 * place arbitrary text on the page.
 */
export function parseStableErrorCode(payload: unknown): string {
  const error = objectPayload(payload)?.error;
  if (typeof error !== "string" || !STABLE_CODE_PATTERN.test(error)) {
    return UNREPORTED_FAILURE_CODE;
  }
  return error;
}

function typedResult(payload: unknown): Record<string, unknown> | null {
  return objectPayload(objectPayload(payload)?.result);
}

/**
 * Returns the ticket a `202` envelope carries, or `null` for any other shape.
 *
 * Unknown additional fields are ignored, as the versioned contract permits
 * additive changes in `/api/v1/`.
 */
export function parseIssuedTicket(payload: unknown): string | null {
  const ticket = typedResult(payload)?.restore_ticket;
  if (typeof ticket !== "string" || !OPAQUE_TOKEN_PATTERN.test(ticket)) {
    return null;
  }
  return ticket;
}

/** Reports whether a `200` envelope is the documented completion envelope. */
export function isRestoreCompleted(payload: unknown): boolean {
  return typedResult(payload)?.lifecycle === LIFECYCLE_INITIALIZED;
}

async function rejection(response: Response): Promise<RestoreFailedError> {
  try {
    return new RestoreFailedError(parseStableErrorCode(await response.json()));
  } catch {
    return new RestoreFailedError(UNREPORTED_FAILURE_CODE);
  }
}

/**
 * Submits the recovery key alone and returns the one-time ticket it earns.
 *
 * The key travels in the request body of this request only. It is never placed
 * in the URL, a query string, a header, or a cookie, and it never travels with
 * the artifact.
 */
async function submitRecoveryKey(recoveryKey: string): Promise<string> {
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
    throw new RestoreFailedError(UNREPORTED_FAILURE_CODE);
  }

  if (response.status !== TICKET_ISSUED_STATUS) {
    throw await rejection(response);
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    throw new RestoreFailedError(UNREPORTED_FAILURE_CODE);
  }

  const ticket = parseIssuedTicket(payload);
  if (ticket === null) {
    throw new RestoreFailedError(UNREPORTED_FAILURE_CODE);
  }
  return ticket;
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
async function uploadArtifact(ticket: string, artifact: File): Promise<void> {
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
    throw new RestoreFailedError(UNREPORTED_FAILURE_CODE);
  }

  if (response.status !== RESTORE_COMPLETED_STATUS) {
    throw await rejection(response);
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    throw new RestoreFailedError(UNREPORTED_FAILURE_CODE);
  }

  if (!isRestoreCompleted(payload)) {
    throw new RestoreFailedError(UNREPORTED_FAILURE_CODE);
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
  const ticket = await submitRecoveryKey(recoveryKey);
  await uploadArtifact(ticket, artifact);
}
