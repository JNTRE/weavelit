/**
 * Same-origin client for submission-bound lifecycle reconciliation.
 *
 * The capability is delivered by an Init or Restore submission and remains in
 * the calling workflow's transient memory. This module only sends it back in
 * the reconciliation request body; it does not retain or present it.
 */

/** The route that confirms one submitted Init or Restore capability. */
export const LIFECYCLE_RECONCILIATION_PATH = "/api/v1/lifecycle/reconciliation";

/** The one result value a matching completed submission returns. */
const RECONCILIATION_CONFIRMED = "reconciliation_confirmed";

/** The bounded opaque token shape the Server accepts. */
const OPAQUE_CAPABILITY_PATTERN = /^[A-Za-z0-9_-]{1,48}$/;

/** The three reconciliation outcomes a caller can act on. */
export type ReconciliationResult = "confirmed" | "not_found" | "absent";

function objectPayload(payload: unknown): Record<string, unknown> | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return null;
  }
  return payload as Record<string, unknown>;
}

function isConfirmed(payload: unknown): boolean {
  return objectPayload(payload)?.result === RECONCILIATION_CONFIRMED;
}

function isNotFound(payload: unknown): boolean {
  return objectPayload(payload)?.error === "not_found";
}

/**
 * Asks the Server whether one exact submitted lifecycle operation completed.
 *
 * A valid fixed `200` confirmation is the only result that proves completion.
 * A fixed `404` says this capability does not match this deployment, which is
 * intentionally not completion evidence. Every other response, invalid body,
 * or transport failure remains absent and therefore indeterminate.
 */
export async function reconcileLifecycle(
  reconciliationCapability: string,
): Promise<ReconciliationResult> {
  if (!OPAQUE_CAPABILITY_PATTERN.test(reconciliationCapability)) {
    return "absent";
  }

  let response: Response;
  try {
    response = await fetch(LIFECYCLE_RECONCILIATION_PATH, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        "X-Weavelit-CSRF": "1",
      },
      body: JSON.stringify({ reconciliation_capability: reconciliationCapability }),
      credentials: "omit",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    return "absent";
  }

  if (response.status !== 200 && response.status !== 404) {
    return "absent";
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    return "absent";
  }

  if (response.status === 200) {
    return isConfirmed(payload) ? "confirmed" : "absent";
  }
  return isNotFound(payload) ? "not_found" : "absent";
}
