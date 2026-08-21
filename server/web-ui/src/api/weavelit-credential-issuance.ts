import { CSRF_HEADER_NAME, readCsrfToken } from "./weavelit-authentication";

export const CREDENTIAL_ISSUANCE_STEP_UP_PATH =
  "/api/v1/administration/step-up/credential-issuance";
export const ACCOUNTS_CREATE_PATH = "/api/v1/administration/accounts/create";
export const ACCOUNTS_RESET_PASSWORD_PATH = "/api/v1/administration/accounts/reset-password";

const CANONICAL_FINAL_BASE64URL_CHARACTER = "[AEIMQUYcgkosw048]";
const TICKET_PATTERN = new RegExp(`^[A-Za-z0-9_-]{42}${CANONICAL_FINAL_BASE64URL_CHARACTER}$`);
const PUBLIC_ID_PATTERN = new RegExp(`^[A-Za-z0-9_-]{21}${CANONICAL_FINAL_BASE64URL_CHARACTER}$`);
const ZERO_PUBLIC_ID = "AAAAAAAAAAAAAAAAAAAAAA";
const TEMPORARY_PASSWORD_PATTERN = /^[A-Za-z0-9_-]{24}$/;
const CORRELATION_PATTERN = /^[a-z0-9-]{1,64}$/;

const REPORTED_REFUSALS = new Map<number, ReadonlySet<string>>([
  [400, new Set(["bad_request"])],
  [401, new Set(["session_invalid"])],
  [403, new Set(["request_origin_denied", "authorization_denied", "credential_issuance_denied"])],
  [404, new Set(["not_found"])],
  [405, new Set(["method_not_allowed"])],
  [409, new Set(["conflict"])],
  [503, new Set(["service_unavailable"])],
]);

export interface CredentialIssued {
  readonly publicId: string;
  readonly temporaryPassword: string;
}

export class CredentialIssuanceRefusedError extends Error {
  constructor() {
    super("credential_issuance_refused");
    this.name = "CredentialIssuanceRefusedError";
  }
}

export class CredentialIssuanceIndeterminateError extends Error {
  constructor() {
    super("credential_issuance_indeterminate");
    this.name = "CredentialIssuanceIndeterminateError";
  }
}

function objectPayload(payload: unknown): Record<string, unknown> | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return null;
  }
  return payload as Record<string, unknown>;
}

function typedResult(payload: unknown): Record<string, unknown> | null {
  const envelope = objectPayload(payload);
  if (
    envelope === null ||
    typeof envelope.correlation_id !== "string" ||
    !CORRELATION_PATTERN.test(envelope.correlation_id)
  ) {
    return null;
  }
  return objectPayload(envelope.result);
}

export function readCredentialIssuanceTicket(payload: unknown): string | null {
  const ticket = typedResult(payload)?.credential_issuance_ticket;
  return typeof ticket === "string" && TICKET_PATTERN.test(ticket) ? ticket : null;
}

export function readCredentialIssued(payload: unknown): CredentialIssued | null {
  const result = typedResult(payload);
  if (result === null) {
    return null;
  }
  const publicId = result.public_id;
  const temporaryPassword = result.temporary_password;
  if (
    typeof publicId !== "string" ||
    !PUBLIC_ID_PATTERN.test(publicId) ||
    publicId === ZERO_PUBLIC_ID ||
    typeof temporaryPassword !== "string" ||
    !TEMPORARY_PASSWORD_PATTERN.test(temporaryPassword)
  ) {
    return null;
  }
  return { publicId, temporaryPassword };
}

async function isReportedRefusal(response: Response): Promise<boolean> {
  const allowedCodes = REPORTED_REFUSALS.get(response.status);
  if (allowedCodes === undefined) {
    return false;
  }
  try {
    const envelope = objectPayload(await response.json());
    return (
      envelope !== null &&
      typeof envelope.error === "string" &&
      allowedCodes.has(envelope.error) &&
      typeof envelope.correlation_id === "string" &&
      CORRELATION_PATTERN.test(envelope.correlation_id)
    );
  } catch {
    return false;
  }
}

async function credentialIssuanceRequest(path: string, body: object): Promise<unknown> {
  const csrf = readCsrfToken();
  if (csrf === null) {
    throw new CredentialIssuanceRefusedError();
  }

  let response: Response;
  try {
    response = await fetch(path, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        [CSRF_HEADER_NAME]: csrf,
      },
      body: JSON.stringify(body),
      credentials: "same-origin",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    throw new CredentialIssuanceIndeterminateError();
  }

  if (response.status !== 200) {
    if (await isReportedRefusal(response)) {
      throw new CredentialIssuanceRefusedError();
    }
    throw new CredentialIssuanceIndeterminateError();
  }

  try {
    return await response.json();
  } catch {
    throw new CredentialIssuanceIndeterminateError();
  }
}

export async function issueCredentialIssuanceTicket(
  password: string,
  totpCode?: string,
): Promise<string> {
  const payload = await credentialIssuanceRequest(
    CREDENTIAL_ISSUANCE_STEP_UP_PATH,
    totpCode === undefined ? { password } : { password, totp_code: totpCode },
  );
  const ticket = readCredentialIssuanceTicket(payload);
  if (ticket === null) {
    throw new CredentialIssuanceIndeterminateError();
  }
  return ticket;
}

export async function createAccount(
  username: string,
  displayName: string | undefined,
  credentialIssuanceTicket: string,
): Promise<CredentialIssued> {
  const payload = await credentialIssuanceRequest(
    ACCOUNTS_CREATE_PATH,
    displayName === undefined
      ? { username, credential_issuance_ticket: credentialIssuanceTicket }
      : {
          username,
          display_name: displayName,
          credential_issuance_ticket: credentialIssuanceTicket,
        },
  );
  const issued = readCredentialIssued(payload);
  if (issued === null) {
    throw new CredentialIssuanceIndeterminateError();
  }
  return issued;
}

export async function resetAccountPassword(
  publicId: string,
  credentialIssuanceTicket: string,
): Promise<CredentialIssued> {
  const payload = await credentialIssuanceRequest(ACCOUNTS_RESET_PASSWORD_PATH, {
    public_id: publicId,
    credential_issuance_ticket: credentialIssuanceTicket,
  });
  const issued = readCredentialIssued(payload);
  if (issued?.publicId !== publicId) {
    throw new CredentialIssuanceIndeterminateError();
  }
  return issued;
}
