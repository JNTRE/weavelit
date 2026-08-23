import { readAccountProjection, type AccountProjection } from "./weavelit-administration-accounts";
import { CSRF_HEADER_NAME, readCsrfToken } from "./weavelit-authentication";

export const MFA_POLICY_STEP_UP_PATH = "/api/v1/administration/step-up/totp";
export const ACCOUNTS_MFA_REQUIREMENT_PATH = "/api/v1/administration/accounts/mfa-requirement";
export const ACCOUNTS_MFA_RESET_PATH = "/api/v1/administration/accounts/mfa-reset";

const CANONICAL_PUBLIC_ID_FINAL_BASE64URL_CHARACTER = "[AQgw]";
const TICKET_FINAL_BASE64URL_CHARACTER = "[AEIMQUYcgkosw048]";
const TICKET_PATTERN = new RegExp(`^[A-Za-z0-9_-]{42}${TICKET_FINAL_BASE64URL_CHARACTER}$`);
const PUBLIC_ID_PATTERN = new RegExp(
  `^[A-Za-z0-9_-]{21}${CANONICAL_PUBLIC_ID_FINAL_BASE64URL_CHARACTER}$`,
);
const ZERO_PUBLIC_ID = "AAAAAAAAAAAAAAAAAAAAAA";
const CORRELATION_PATTERN = /^[a-z0-9-]{1,64}$/;
const TICKET_RESULT_FIELDS = new Set(["totp_step_up_ticket"]);
const ENVELOPE_FIELDS = new Set(["result", "correlation_id"]);
type TerminalAccessFamily = "mfa_policy" | "grant_mutation";

const REPORTED_REFUSALS = new Map<number, ReadonlySet<string>>([
  [400, new Set(["bad_request"])],
  [401, new Set(["session_invalid"])],
  [403, new Set(["request_origin_denied", "authorization_denied", "mfa_policy_denied"])],
  [404, new Set(["not_found"])],
  [405, new Set(["method_not_allowed"])],
  [503, new Set(["service_unavailable"])],
]);

export class MfaPolicyRefusedError extends Error {
  constructor() {
    super("mfa_policy_refused");
    this.name = "MfaPolicyRefusedError";
  }
}

export class MfaPolicySessionInvalidError extends MfaPolicyRefusedError {
  constructor() {
    super();
    this.name = "MfaPolicySessionInvalidError";
  }
}

export class MfaPolicyAccessDeniedError extends MfaPolicyRefusedError {
  constructor() {
    super();
    this.name = "MfaPolicyAccessDeniedError";
  }
}

export class MfaPolicyGrantMutationAccessDeniedError extends MfaPolicyRefusedError {
  constructor() {
    super();
    this.name = "MfaPolicyGrantMutationAccessDeniedError";
  }
}

export class MfaPolicyIndeterminateError extends Error {
  constructor() {
    super("mfa_policy_indeterminate");
    this.name = "MfaPolicyIndeterminateError";
  }
}

function objectPayload(payload: unknown): Record<string, unknown> | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return null;
  }
  return payload as Record<string, unknown>;
}

function hasExactFields(value: Record<string, unknown>, fields: ReadonlySet<string>): boolean {
  const keys = Object.keys(value);
  return keys.length === fields.size && keys.every((key) => fields.has(key));
}

function strictEnvelope(payload: unknown): Record<string, unknown> | null {
  const envelope = objectPayload(payload);
  if (
    envelope === null ||
    !hasExactFields(envelope, ENVELOPE_FIELDS) ||
    typeof envelope.correlation_id !== "string" ||
    !CORRELATION_PATTERN.test(envelope.correlation_id)
  ) {
    return null;
  }
  return envelope;
}

export function readMfaPolicyTicket(payload: unknown): string | null {
  const result = objectPayload(strictEnvelope(payload)?.result);
  if (result === null || !hasExactFields(result, TICKET_RESULT_FIELDS)) {
    return null;
  }
  const ticket = result.totp_step_up_ticket;
  return typeof ticket === "string" && TICKET_PATTERN.test(ticket) ? ticket : null;
}

function readPolicyAccount(payload: unknown): AccountProjection | null {
  return strictEnvelope(payload) === null ? null : readAccountProjection(payload);
}

async function reportedRefusal(response: Response): Promise<string | null> {
  const allowedCodes = REPORTED_REFUSALS.get(response.status);
  if (allowedCodes === undefined) {
    return null;
  }
  try {
    const envelope = objectPayload(await response.json());
    if (
      envelope !== null &&
      Object.keys(envelope).length === 2 &&
      typeof envelope.error === "string" &&
      allowedCodes.has(envelope.error) &&
      typeof envelope.correlation_id === "string" &&
      CORRELATION_PATTERN.test(envelope.correlation_id)
    ) {
      return envelope.error;
    }
    return null;
  } catch {
    return null;
  }
}

async function mfaPolicyRequest(
  path: string,
  body: object,
  terminalAccessFamily: TerminalAccessFamily,
): Promise<unknown> {
  const csrf = readCsrfToken();
  if (csrf === null) {
    throw new MfaPolicyRefusedError();
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
    throw new MfaPolicyIndeterminateError();
  }
  if (response.status !== 200) {
    const refusal = await reportedRefusal(response);
    if (refusal === "session_invalid") {
      throw new MfaPolicySessionInvalidError();
    }
    if (refusal === "authorization_denied") {
      if (terminalAccessFamily === "mfa_policy") {
        throw new MfaPolicyAccessDeniedError();
      }
      if (terminalAccessFamily === "grant_mutation") {
        throw new MfaPolicyGrantMutationAccessDeniedError();
      }
    }
    if (refusal !== null) {
      throw new MfaPolicyRefusedError();
    }
    throw new MfaPolicyIndeterminateError();
  }
  try {
    return await response.json();
  } catch {
    throw new MfaPolicyIndeterminateError();
  }
}

export async function issueMfaPolicyStepUp(code: string): Promise<string> {
  return issueStepUp("mfa_policy", code);
}

export async function issueGrantMutationStepUp(code: string): Promise<string> {
  return issueStepUp("grant_mutation", code);
}

async function issueStepUp(family: "mfa_policy" | "grant_mutation", code: string): Promise<string> {
  const payload = await mfaPolicyRequest(MFA_POLICY_STEP_UP_PATH, { family, code }, family);
  const ticket = readMfaPolicyTicket(payload);
  if (ticket === null) {
    throw new MfaPolicyIndeterminateError();
  }
  return ticket;
}

export async function changeMfaRequirement(
  publicId: string,
  required: boolean,
  ticket: string,
): Promise<AccountProjection> {
  if (!validPublicId(publicId) || !TICKET_PATTERN.test(ticket)) {
    throw new MfaPolicyRefusedError();
  }
  const payload = await mfaPolicyRequest(
    ACCOUNTS_MFA_REQUIREMENT_PATH,
    {
      public_id: publicId,
      required,
      totp_step_up_ticket: ticket,
    },
    "mfa_policy",
  );
  const account = readPolicyAccount(payload);
  if (account?.publicId !== publicId || account.mfaRequired !== required) {
    throw new MfaPolicyIndeterminateError();
  }
  return account;
}

export async function resetMfaEnrollment(
  publicId: string,
  ticket: string,
): Promise<AccountProjection> {
  if (!validPublicId(publicId) || !TICKET_PATTERN.test(ticket)) {
    throw new MfaPolicyRefusedError();
  }
  const payload = await mfaPolicyRequest(
    ACCOUNTS_MFA_RESET_PATH,
    {
      public_id: publicId,
      totp_step_up_ticket: ticket,
    },
    "mfa_policy",
  );
  const account = readPolicyAccount(payload);
  if (account?.publicId !== publicId) {
    throw new MfaPolicyIndeterminateError();
  }
  return account;
}

function validPublicId(value: string): boolean {
  return PUBLIC_ID_PATTERN.test(value) && value !== ZERO_PUBLIC_ID;
}
