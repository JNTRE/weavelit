import { CSRF_HEADER_NAME, readCsrfToken } from "./weavelit-authentication";

export const ACCOUNTS_LIST_PATH = "/api/v1/administration/accounts/list";
export const ACCOUNTS_VIEW_PATH = "/api/v1/administration/accounts/view";
export const ACCOUNTS_STATUS_PATH = "/api/v1/administration/accounts/status";

const CANONICAL_FINAL_BASE64URL_CHARACTER = "[AQgw]";
const PUBLIC_ID_PATTERN = new RegExp(`^[A-Za-z0-9_-]{21}${CANONICAL_FINAL_BASE64URL_CHARACTER}$`);
const ZERO_PUBLIC_ID = "AAAAAAAAAAAAAAAAAAAAAA";
const CURSOR_PATTERN = /^[A-Za-z0-9_-]{1,416}$/;
const CORRELATION_PATTERN = /^[a-z0-9-]{1,64}$/;
const MAX_NAME_BYTES = 256;
const MAX_PAGE_ITEMS = 100;
const RESULT_ENVELOPE_FIELDS = new Set(["result", "correlation_id"]);
const ERROR_ENVELOPE_FIELDS = new Set(["error", "correlation_id"]);
const PAGE_FIELDS = new Set(["items", "next_cursor"]);
const PROJECTION_FIELDS = new Set([
  "public_id",
  "username",
  "display_name",
  "active",
  "mfa_required",
]);
const REPORTED_STATUS_REFUSALS = new Map<number, ReadonlySet<string>>([
  [400, new Set(["bad_request"])],
  [401, new Set(["session_invalid"])],
  [403, new Set(["request_origin_denied", "authorization_denied"])],
  [404, new Set(["not_found"])],
  [405, new Set(["method_not_allowed"])],
  [503, new Set(["service_unavailable"])],
]);

export interface AccountProjection {
  readonly publicId: string;
  readonly username: string;
  readonly displayName: string | null;
  readonly active: boolean;
  readonly mfaRequired: boolean;
}

export interface AccountsPage {
  readonly items: readonly AccountProjection[];
  readonly nextCursor: string | null;
}

export class AccountsUnavailableError extends Error {
  constructor() {
    super("accounts_unavailable");
    this.name = "AccountsUnavailableError";
  }
}

export class AccountsSessionExpiredError extends Error {
  constructor() {
    super("accounts_session_expired");
    this.name = "AccountsSessionExpiredError";
  }
}

export class AccountStatusRefusedError extends Error {
  constructor() {
    super("account_status_refused");
    this.name = "AccountStatusRefusedError";
  }
}

export class AccountStatusIndeterminateError extends Error {
  constructor() {
    super("account_status_indeterminate");
    this.name = "AccountStatusIndeterminateError";
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

function resultPayload(payload: unknown): Record<string, unknown> | null {
  const envelope = objectPayload(payload);
  return envelope !== null &&
    hasExactFields(envelope, RESULT_ENVELOPE_FIELDS) &&
    typeof envelope.correlation_id === "string" &&
    CORRELATION_PATTERN.test(envelope.correlation_id)
    ? objectPayload(envelope.result)
    : null;
}

function safeName(value: unknown): string | null {
  if (typeof value !== "string" || value === "" || /[\u0000-\u001f\u007f]/.test(value)) {
    return null;
  }
  return new TextEncoder().encode(value).length <= MAX_NAME_BYTES ? value : null;
}

function accountProjection(payload: unknown): AccountProjection | null {
  const value = objectPayload(payload);
  if (
    value === null ||
    Object.keys(value).length !== PROJECTION_FIELDS.size ||
    Object.keys(value).some((field) => !PROJECTION_FIELDS.has(field))
  ) {
    return null;
  }
  const publicId = value.public_id;
  const username = safeName(value.username);
  const displayName = value.display_name === null ? null : safeName(value.display_name);
  if (
    typeof publicId !== "string" ||
    !PUBLIC_ID_PATTERN.test(publicId) ||
    publicId === ZERO_PUBLIC_ID ||
    username === null ||
    (displayName === null && value.display_name !== null) ||
    typeof value.active !== "boolean" ||
    typeof value.mfa_required !== "boolean"
  ) {
    return null;
  }
  return {
    publicId,
    username,
    displayName,
    active: value.active,
    mfaRequired: value.mfa_required,
  };
}

export function readAccountsPage(payload: unknown): AccountsPage | null {
  const result = resultPayload(payload);
  if (
    result === null ||
    !hasExactFields(result, PAGE_FIELDS) ||
    !Array.isArray(result.items) ||
    result.items.length > MAX_PAGE_ITEMS
  ) {
    return null;
  }
  const items: AccountProjection[] = [];
  for (const item of result.items) {
    const projection = accountProjection(item);
    if (projection === null) {
      return null;
    }
    items.push(projection);
  }
  const nextCursor = result.next_cursor;
  if (nextCursor !== null && (typeof nextCursor !== "string" || !CURSOR_PATTERN.test(nextCursor))) {
    return null;
  }
  return { items, nextCursor };
}

export function readAccountProjectionValue(payload: unknown): AccountProjection | null {
  return accountProjection(payload);
}

export function readAccountProjection(payload: unknown): AccountProjection | null {
  return accountProjection(resultPayload(payload));
}

async function isExpiredSession(response: Response): Promise<boolean> {
  if (response.status !== 401) {
    return false;
  }
  try {
    const envelope = objectPayload(await response.json());
    return (
      envelope !== null &&
      hasExactFields(envelope, ERROR_ENVELOPE_FIELDS) &&
      envelope.error === "session_invalid" &&
      typeof envelope.correlation_id === "string" &&
      CORRELATION_PATTERN.test(envelope.correlation_id)
    );
  } catch {
    return false;
  }
}

async function accountRequest(path: string, body: object): Promise<unknown> {
  const csrf = readCsrfToken();
  if (csrf === null) {
    throw new AccountsUnavailableError();
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
    throw new AccountsUnavailableError();
  }
  if (response.status !== 200) {
    if (await isExpiredSession(response)) {
      throw new AccountsSessionExpiredError();
    }
    throw new AccountsUnavailableError();
  }
  try {
    return await response.json();
  } catch {
    throw new AccountsUnavailableError();
  }
}

export async function listAccounts(cursor?: string): Promise<AccountsPage> {
  const payload = await accountRequest(ACCOUNTS_LIST_PATH, cursor === undefined ? {} : { cursor });
  const page = readAccountsPage(payload);
  if (page === null) {
    throw new AccountsUnavailableError();
  }
  return page;
}

export async function viewAccount(publicId: string): Promise<AccountProjection> {
  if (!PUBLIC_ID_PATTERN.test(publicId) || publicId === ZERO_PUBLIC_ID) {
    throw new AccountsUnavailableError();
  }
  const payload = await accountRequest(ACCOUNTS_VIEW_PATH, { public_id: publicId });
  const account = accountProjection(resultPayload(payload));
  if (account?.publicId !== publicId) {
    throw new AccountsUnavailableError();
  }
  return account;
}

async function reportedStatusRefusal(response: Response): Promise<string | null> {
  const allowedCodes = REPORTED_STATUS_REFUSALS.get(response.status);
  if (allowedCodes === undefined) {
    return null;
  }
  try {
    const envelope = objectPayload(await response.json());
    const code = envelope?.error;
    return envelope !== null &&
      Object.keys(envelope).length === 2 &&
      typeof code === "string" &&
      allowedCodes.has(code) &&
      typeof envelope.correlation_id === "string" &&
      CORRELATION_PATTERN.test(envelope.correlation_id)
      ? code
      : null;
  } catch {
    return null;
  }
}

export async function changeAccountStatus(
  publicId: string,
  active: boolean,
): Promise<AccountProjection> {
  if (!PUBLIC_ID_PATTERN.test(publicId) || publicId === ZERO_PUBLIC_ID) {
    throw new AccountStatusRefusedError();
  }
  const csrf = readCsrfToken();
  if (csrf === null) {
    throw new AccountStatusRefusedError();
  }

  let response: Response;
  try {
    response = await fetch(ACCOUNTS_STATUS_PATH, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        [CSRF_HEADER_NAME]: csrf,
      },
      body: JSON.stringify({ public_id: publicId, active }),
      credentials: "same-origin",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    throw new AccountStatusIndeterminateError();
  }

  if (response.status !== 200) {
    const refusal = await reportedStatusRefusal(response);
    if (response.status === 401 && refusal === "session_invalid") {
      throw new AccountsSessionExpiredError();
    }
    if (refusal !== null) {
      throw new AccountStatusRefusedError();
    }
    throw new AccountStatusIndeterminateError();
  }

  let payload: unknown;
  try {
    payload = await response.json();
  } catch {
    throw new AccountStatusIndeterminateError();
  }
  const account = accountProjection(resultPayload(payload));
  if (account?.publicId !== publicId || account.active !== active) {
    throw new AccountStatusIndeterminateError();
  }
  return account;
}
