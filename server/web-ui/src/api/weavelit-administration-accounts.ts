import { CSRF_HEADER_NAME, readCsrfToken } from "./weavelit-authentication";

export const ACCOUNTS_LIST_PATH = "/api/v1/administration/accounts/list";
export const ACCOUNTS_VIEW_PATH = "/api/v1/administration/accounts/view";

const PUBLIC_ID_PATTERN = /^[A-Za-z0-9_-]{22}$/;
const CURSOR_PATTERN = /^[A-Za-z0-9_-]{1,416}$/;
const MAX_NAME_BYTES = 256;
const MAX_PAGE_ITEMS = 100;

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

function objectPayload(payload: unknown): Record<string, unknown> | null {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) {
    return null;
  }
  return payload as Record<string, unknown>;
}

function safeName(value: unknown): string | null {
  if (typeof value !== "string" || value === "" || /[\u0000-\u001f\u007f]/.test(value)) {
    return null;
  }
  return new TextEncoder().encode(value).length <= MAX_NAME_BYTES ? value : null;
}

function accountProjection(payload: unknown): AccountProjection | null {
  const value = objectPayload(payload);
  if (value === null) {
    return null;
  }
  const publicId = value.public_id;
  const username = safeName(value.username);
  const displayName = value.display_name === null ? null : safeName(value.display_name);
  if (
    typeof publicId !== "string" ||
    !PUBLIC_ID_PATTERN.test(publicId) ||
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
  const result = objectPayload(objectPayload(payload)?.result);
  if (result === null || !Array.isArray(result.items) || result.items.length > MAX_PAGE_ITEMS) {
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

export function readAccountProjection(payload: unknown): AccountProjection | null {
  return accountProjection(objectPayload(payload)?.result);
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
  if (!PUBLIC_ID_PATTERN.test(publicId)) {
    throw new AccountsUnavailableError();
  }
  const payload = await accountRequest(ACCOUNTS_VIEW_PATH, { public_id: publicId });
  const account = readAccountProjection(payload);
  if (account?.publicId !== publicId) {
    throw new AccountsUnavailableError();
  }
  return account;
}
