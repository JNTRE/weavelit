/**
 * Same-origin client for the Web UI Client Module Application Database
 * selection route.
 *
 * The contract is owned by
 * `docs/client-modules/web-ui/pre-operational-database-selection-design.md`.
 */

import { parsePreOperationalStatus, type PreOperationalStatus } from "./weavelit-init-status";

/** The selection route this application requests. */
export const DATABASE_SELECTION_PATH = "/api/v1/application-database";

/**
 * The exact request body the route accepts.
 *
 * It is a fixed literal rather than a serialized object so the bytes on the
 * wire cannot drift with a serializer change.
 */
export const SQLITE_SELECTION_BODY = '{"backend":"sqlite","settings":{}}';

/**
 * Failure of the selection request that carries no response detail.
 *
 * The message is fixed so no server error code, status number, or transport
 * diagnostic can reach the browser presentation layer. The route deliberately
 * returns no distinguishing detail, so the client surfaces none.
 */
export class DatabaseSelectionFailedError extends Error {
  constructor() {
    super("database_selection_failed");
    this.name = "DatabaseSelectionFailedError";
  }
}

/**
 * Parses a decoded success payload into the documented projection.
 *
 * The route returns the status surface's projection shape, so shape validation
 * is shared with the status client. A success payload must additionally report
 * a selected database; a `200` claiming otherwise contradicts the contract and
 * is rejected rather than trusted. Unknown additional fields are ignored, as
 * the versioned contract permits additive changes in `/api/v1/`.
 */
export function parseDatabaseSelectionResult(payload: unknown): PreOperationalStatus | null {
  const status = parsePreOperationalStatus(payload);
  if (!status?.databaseSelected) {
    return null;
  }
  return status;
}

/**
 * Selects SQLite as the Application Database on the same-origin Server.
 *
 * `Host` and `Origin` are forbidden header names that the browser sets itself
 * on a same-origin request, so this client never attempts to set them.
 *
 * Resolves with the authoritative projection carried by the success response,
 * which callers apply directly instead of re-requesting the status. Throws
 * {@link DatabaseSelectionFailedError} for a transport failure, any response
 * other than `200 OK`, or a payload that is not the documented shape.
 */
export async function selectSqliteDatabase(signal?: AbortSignal): Promise<PreOperationalStatus> {
  let response: Response;
  try {
    response = await fetch(DATABASE_SELECTION_PATH, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        "X-Weavelit-CSRF": "1",
      },
      body: SQLITE_SELECTION_BODY,
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
      ...(signal ? { signal } : {}),
    });
  } catch {
    throw new DatabaseSelectionFailedError();
  }

  if (response.status !== 200) {
    throw new DatabaseSelectionFailedError();
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    throw new DatabaseSelectionFailedError();
  }

  const status = parseDatabaseSelectionResult(payload);
  if (status === null) {
    throw new DatabaseSelectionFailedError();
  }
  return status;
}
