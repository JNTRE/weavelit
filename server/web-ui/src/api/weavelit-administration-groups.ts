import { CSRF_HEADER_NAME, readCsrfToken } from "./weavelit-authentication";

export const GROUPS_LIST_PATH = "/api/v1/administration/groups/list";
export const GROUPS_VIEW_PATH = "/api/v1/administration/groups/view";
export const GROUPS_CREATE_PATH = "/api/v1/administration/groups/create";
export const GROUPS_UPDATE_PATH = "/api/v1/administration/groups/update";
export const GROUPS_DELETE_PATH = "/api/v1/administration/groups/delete";

const FINAL_BASE64URL_CHARACTER = "[AEIMQUYcgkosw048]";
const PUBLIC_ID_PATTERN = new RegExp(`^[A-Za-z0-9_-]{21}${FINAL_BASE64URL_CHARACTER}$`);
const TICKET_PATTERN = new RegExp(`^[A-Za-z0-9_-]{42}${FINAL_BASE64URL_CHARACTER}$`);
const ZERO_PUBLIC_ID = "AAAAAAAAAAAAAAAAAAAAAA";
const CURSOR_PATTERN = /^[A-Za-z0-9_-]{1,416}$/;
const CORRELATION_PATTERN = /^[a-z0-9-]{1,64}$/;
const PROJECTION_FIELDS = new Set(["public_id", "name", "description"]);
const PAGE_FIELDS = new Set(["items", "next_cursor"]);
const ENVELOPE_FIELDS = new Set(["result", "correlation_id"]);
const MAX_NAME_BYTES = 256;
const MAX_DESCRIPTION_BYTES = 1024;

export interface GroupProjection {
  readonly publicId: string;
  readonly name: string;
  readonly description: string | null;
}
export interface GroupsPage {
  readonly items: readonly GroupProjection[];
  readonly nextCursor: string | null;
}

export class GroupsUnavailableError extends Error {
  constructor() {
    super("groups_unavailable");
    this.name = "GroupsUnavailableError";
  }
}
export class GroupMutationRefusedError extends Error {
  constructor() {
    super("group_mutation_refused");
    this.name = "GroupMutationRefusedError";
  }
}
export class GroupMutationIndeterminateError extends Error {
  constructor() {
    super("group_mutation_indeterminate");
    this.name = "GroupMutationIndeterminateError";
  }
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}
function exact(value: Record<string, unknown>, fields: ReadonlySet<string>): boolean {
  const keys = Object.keys(value);
  return keys.length === fields.size && keys.every((key) => fields.has(key));
}
function safeText(value: unknown, max: number): string | null {
  return typeof value === "string" &&
    value !== "" &&
    !/[\u0000-\u001f\u007f]/.test(value) &&
    new TextEncoder().encode(value).length <= max
    ? value
    : null;
}
function validId(value: string): boolean {
  return PUBLIC_ID_PATTERN.test(value) && value !== ZERO_PUBLIC_ID;
}
function envelope(payload: unknown): Record<string, unknown> | null {
  const value = objectValue(payload);
  return value !== null &&
    exact(value, ENVELOPE_FIELDS) &&
    typeof value.correlation_id === "string" &&
    CORRELATION_PATTERN.test(value.correlation_id)
    ? value
    : null;
}
export function readGroupProjection(payload: unknown): GroupProjection | null {
  const value = objectValue(envelope(payload)?.result);
  if (value === null || !exact(value, PROJECTION_FIELDS)) return null;
  const publicId = value.public_id;
  const name = safeText(value.name, MAX_NAME_BYTES);
  const description =
    value.description === null ? null : safeText(value.description, MAX_DESCRIPTION_BYTES);
  if (
    typeof publicId !== "string" ||
    !validId(publicId) ||
    name === null ||
    (description === null && value.description !== null)
  )
    return null;
  return { publicId, name, description };
}
export function readGroupsPage(payload: unknown): GroupsPage | null {
  const result = objectValue(envelope(payload)?.result);
  if (
    result === null ||
    !exact(result, PAGE_FIELDS) ||
    !Array.isArray(result.items) ||
    result.items.length > 100
  )
    return null;
  const rawItems: unknown[] = result.items;
  const items: GroupProjection[] = [];
  for (const value of rawItems) {
    const projectionEnvelope = { result: value, correlation_id: "projection" };
    const projection = readGroupProjection(projectionEnvelope);
    if (projection === null) return null;
    items.push(projection);
  }
  const cursor = result.next_cursor;
  if (cursor !== null && (typeof cursor !== "string" || !CURSOR_PATTERN.test(cursor))) return null;
  return { items, nextCursor: cursor };
}

async function request(path: string, body: object, mutation: boolean): Promise<unknown> {
  const csrf = readCsrfToken();
  if (csrf === null)
    throw mutation ? new GroupMutationRefusedError() : new GroupsUnavailableError();
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
    throw mutation ? new GroupMutationIndeterminateError() : new GroupsUnavailableError();
  }
  if (response.status !== 200) {
    if (!mutation) throw new GroupsUnavailableError();
    if (await reportedRefusal(response)) throw new GroupMutationRefusedError();
    throw new GroupMutationIndeterminateError();
  }
  try {
    return await response.json();
  } catch {
    throw mutation ? new GroupMutationIndeterminateError() : new GroupsUnavailableError();
  }
}
async function reportedRefusal(response: Response): Promise<boolean> {
  const allowed = new Map<number, ReadonlySet<string>>([
    [400, new Set(["bad_request"])],
    [401, new Set(["session_invalid"])],
    [403, new Set(["request_origin_denied", "authorization_denied", "grant_mutation_denied"])],
    [404, new Set(["not_found"])],
    [405, new Set(["method_not_allowed"])],
    [409, new Set(["conflict"])],
    [503, new Set(["service_unavailable"])],
  ]).get(response.status);
  if (allowed === undefined) return false;
  try {
    const value = objectValue(await response.json());
    return (
      value !== null &&
      Object.keys(value).length === 2 &&
      typeof value.error === "string" &&
      allowed.has(value.error) &&
      typeof value.correlation_id === "string" &&
      CORRELATION_PATTERN.test(value.correlation_id)
    );
  } catch {
    return false;
  }
}

export async function listGroups(cursor?: string): Promise<GroupsPage> {
  const page = readGroupsPage(
    await request(GROUPS_LIST_PATH, cursor === undefined ? {} : { cursor }, false),
  );
  if (page === null) throw new GroupsUnavailableError();
  return page;
}
export async function viewGroup(publicId: string): Promise<GroupProjection> {
  if (!validId(publicId)) throw new GroupsUnavailableError();
  const group = readGroupProjection(
    await request(GROUPS_VIEW_PATH, { public_id: publicId }, false),
  );
  if (group?.publicId !== publicId) throw new GroupsUnavailableError();
  return group;
}
export async function createGroup(
  name: string,
  description: string | null,
): Promise<GroupProjection> {
  const group = readGroupProjection(await request(GROUPS_CREATE_PATH, { name, description }, true));
  if (group?.name !== name || group.description !== description)
    throw new GroupMutationIndeterminateError();
  return group;
}
export async function updateGroup(
  publicId: string,
  name: string,
  description: string | null,
): Promise<GroupProjection> {
  if (!validId(publicId)) throw new GroupMutationRefusedError();
  const group = readGroupProjection(
    await request(GROUPS_UPDATE_PATH, { public_id: publicId, name, description }, true),
  );
  if (group?.publicId !== publicId || group.name !== name || group.description !== description)
    throw new GroupMutationIndeterminateError();
  return group;
}
export async function deleteGroup(publicId: string, ticket: string): Promise<void> {
  if (!validId(publicId) || !TICKET_PATTERN.test(ticket)) throw new GroupMutationRefusedError();
  const payload = envelope(
    await request(
      GROUPS_DELETE_PATH,
      { public_id: publicId, grant_mutation_step_up_ticket: ticket },
      true,
    ),
  );
  const result = objectValue(payload?.result);
  if (result === null || Object.keys(result).length !== 1 || result.public_id !== publicId)
    throw new GroupMutationIndeterminateError();
}
