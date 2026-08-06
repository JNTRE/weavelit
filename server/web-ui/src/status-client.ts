/**
 * Same-origin client for the Web UI Client Module pre-operational status route.
 *
 * The contract is owned by `docs/client-modules/web-ui/pre-operational-status-design.md`.
 */

/** The only path this application requests. */
export const STATUS_PATH = '/api/v1/status';

/** The documented pre-operational status projection. */
export interface PreOperationalStatus {
  readonly lifecycle: 'uninitialized';
  readonly databaseSelected: boolean;
}

/**
 * Failure of the status request that carries no response detail.
 *
 * The message is fixed so no server payload, status text, or transport
 * diagnostic can reach the browser presentation layer.
 */
export class StatusUnavailableError extends Error {
  constructor() {
    super('status_unavailable');
    this.name = 'StatusUnavailableError';
  }
}

/**
 * Parses a decoded payload into the documented projection.
 *
 * Returns `null` for any payload missing a documented field or carrying a
 * documented field with an unexpected type. Unknown additional fields are
 * ignored, as the versioned contract permits additive changes in `/api/v1/`.
 */
export function parsePreOperationalStatus(payload: unknown): PreOperationalStatus | null {
  if (typeof payload !== 'object' || payload === null || Array.isArray(payload)) {
    return null;
  }
  const candidate = payload as Record<string, unknown>;
  if (candidate['lifecycle'] !== 'uninitialized') {
    return null;
  }
  const databaseSelected = candidate['database_selected'];
  if (typeof databaseSelected !== 'boolean') {
    return null;
  }
  return { lifecycle: 'uninitialized', databaseSelected };
}

/**
 * Requests the current status projection from the same-origin Server.
 *
 * Throws {@link StatusUnavailableError} for a transport failure, any response
 * other than `200 OK`, or a payload that is not the documented shape.
 */
export async function fetchPreOperationalStatus(
  signal?: AbortSignal,
): Promise<PreOperationalStatus> {
  let response: Response;
  try {
    response = await fetch(STATUS_PATH, {
      method: 'GET',
      headers: { Accept: 'application/json' },
      credentials: 'omit',
      cache: 'no-store',
      redirect: 'error',
      ...(signal ? { signal } : {}),
    });
  } catch {
    throw new StatusUnavailableError();
  }

  if (response.status !== 200) {
    throw new StatusUnavailableError();
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    throw new StatusUnavailableError();
  }

  const status = parsePreOperationalStatus(payload);
  if (status === null) {
    throw new StatusUnavailableError();
  }
  return status;
}
