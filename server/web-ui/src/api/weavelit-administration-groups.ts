import { CSRF_HEADER_NAME, readCsrfToken } from "./weavelit-authentication";
import {
  readAccountProjectionValue,
  type AccountProjection,
} from "./weavelit-administration-accounts";

export const GROUPS_LIST_PATH = "/api/v1/administration/groups/list";
export const GROUPS_VIEW_PATH = "/api/v1/administration/groups/view";
export const GROUPS_CREATE_PATH = "/api/v1/administration/groups/create";
export const GROUPS_UPDATE_PATH = "/api/v1/administration/groups/update";
export const GROUPS_DELETE_PATH = "/api/v1/administration/groups/delete";
export const GROUP_MEMBERS_LIST_PATH = "/api/v1/administration/groups/members/list";
export const GROUP_MEMBERS_CHANGE_PATH = "/api/v1/administration/groups/members/change";
export const GROUP_GRANTS_LIST_PATH = "/api/v1/administration/groups/grants/list";
export const GROUP_GRANTS_CHANGE_PATH = "/api/v1/administration/groups/grants/change";
export const ADMINISTRATION_CATALOG_PATH = "/api/v1/administration/catalog";

const CANONICAL_FINAL_BASE64URL_CHARACTER = "[AQgw]";
const TICKET_FINAL_BASE64URL_CHARACTER = "[AEIMQUYcgkosw048]";
const PUBLIC_ID_PATTERN = new RegExp(`^[A-Za-z0-9_-]{21}${CANONICAL_FINAL_BASE64URL_CHARACTER}$`);
const TICKET_PATTERN = new RegExp(`^[A-Za-z0-9_-]{42}${TICKET_FINAL_BASE64URL_CHARACTER}$`);
const ZERO_PUBLIC_ID = "AAAAAAAAAAAAAAAAAAAAAA";
const CURSOR_PATTERN = /^[A-Za-z0-9_-]{1,512}$/;
const CORRELATION_PATTERN = /^[a-z0-9-]{1,64}$/;
const PROJECTION_FIELDS = new Set(["public_id", "name", "description"]);
const PAGE_FIELDS = new Set(["items", "next_cursor"]);
const ENVELOPE_FIELDS = new Set(["result", "correlation_id"]);
const MAX_NAME_BYTES = 256;
const MAX_DESCRIPTION_BYTES = 1024;
const MAX_PAGE_ITEMS = 100;
const MAX_CATALOG_ITEMS = 256;
const GRANT_TYPE_FIELDS = new Set(["type"]);
const GRANT_VALUE_FIELDS = new Set(["type", "value"]);
const MEMBER_CHANGE_FIELDS = new Set(["account", "present"]);
const GRANT_CHANGE_FIELDS = new Set(["grant", "present"]);
const CATALOG_FIELDS = new Set(["client_modules", "service_modules", "operations"]);

export interface GroupProjection {
  readonly publicId: string;
  readonly name: string;
  readonly description: string | null;
}
export interface GroupsPage {
  readonly items: readonly GroupProjection[];
  readonly nextCursor: string | null;
}
export interface GroupMembersPage {
  readonly items: readonly AccountProjection[];
  readonly nextCursor: string | null;
}
export type GroupGrant =
  | { readonly type: "client_module"; readonly value: string }
  | { readonly type: "service_module"; readonly value: string }
  | { readonly type: "operation"; readonly value: string }
  | { readonly type: "server_administration" };
export interface GroupGrantsPage {
  readonly items: readonly GroupGrant[];
  readonly nextCursor: string | null;
}
export interface AdministrationCatalog {
  readonly clientModules: readonly string[];
  readonly serviceModules: readonly string[];
  readonly operations: readonly string[];
}

export class GroupsUnavailableError extends Error {
  constructor() {
    super("groups_unavailable");
    this.name = "GroupsUnavailableError";
  }
}
export class GroupSessionInvalidError extends GroupsUnavailableError {
  constructor() {
    super();
    this.name = "GroupSessionInvalidError";
  }
}
export class GroupAdministrationAccessDeniedError extends GroupsUnavailableError {
  constructor() {
    super();
    this.name = "GroupAdministrationAccessDeniedError";
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
export class LastAdministratorRefusedError extends GroupMutationRefusedError {
  constructor() {
    super();
    this.name = "LastAdministratorRefusedError";
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
    result.items.length > MAX_PAGE_ITEMS
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

function readPage(
  payload: unknown,
  parse: (value: unknown) => AccountProjection | GroupGrant | null,
): {
  readonly items: readonly (AccountProjection | GroupGrant)[];
  readonly nextCursor: string | null;
} | null {
  const result = objectValue(envelope(payload)?.result);
  if (
    result === null ||
    !exact(result, PAGE_FIELDS) ||
    !Array.isArray(result.items) ||
    result.items.length > MAX_PAGE_ITEMS
  )
    return null;
  const items: (AccountProjection | GroupGrant)[] = [];
  for (const value of result.items) {
    const item = parse(value);
    if (item === null) return null;
    items.push(item);
  }
  const cursor = result.next_cursor;
  if (cursor !== null && (typeof cursor !== "string" || !CURSOR_PATTERN.test(cursor))) return null;
  return { items, nextCursor: cursor };
}

export function readGroupMembersPage(payload: unknown): GroupMembersPage | null {
  const page = readPage(payload, readAccountProjectionValue);
  return page === null
    ? null
    : { items: page.items as readonly AccountProjection[], nextCursor: page.nextCursor };
}

export function readGroupGrant(payload: unknown): GroupGrant | null {
  const value = objectValue(payload);
  if (value === null || typeof value.type !== "string") return null;
  if (value.type === "server_administration")
    return exact(value, GRANT_TYPE_FIELDS) ? { type: "server_administration" } : null;
  if (
    !exact(value, GRANT_VALUE_FIELDS) ||
    !["client_module", "service_module", "operation"].includes(value.type)
  )
    return null;
  const name = safeText(value.value, MAX_NAME_BYTES);
  if (name === null) return null;
  if (value.type === "client_module") return { type: "client_module", value: name };
  if (value.type === "service_module") return { type: "service_module", value: name };
  return { type: "operation", value: name };
}

export function readGroupGrantsPage(payload: unknown): GroupGrantsPage | null {
  const page = readPage(payload, readGroupGrant);
  return page === null
    ? null
    : { items: page.items as readonly GroupGrant[], nextCursor: page.nextCursor };
}

function sortedNames(value: unknown): readonly string[] | null {
  if (!Array.isArray(value) || value.length > MAX_CATALOG_ITEMS) return null;
  const names: string[] = [];
  for (const item of value) {
    const name = safeText(item, MAX_NAME_BYTES);
    const previous = names.at(-1);
    if (name === null || (previous !== undefined && previous >= name)) return null;
    names.push(name);
  }
  return names;
}

export function readAdministrationCatalog(payload: unknown): AdministrationCatalog | null {
  const result = objectValue(envelope(payload)?.result);
  if (result === null || !exact(result, CATALOG_FIELDS)) return null;
  const clientModules = sortedNames(result.client_modules);
  const serviceModules = sortedNames(result.service_modules);
  const operations = sortedNames(result.operations);
  return clientModules === null || serviceModules === null || operations === null
    ? null
    : { clientModules, serviceModules, operations };
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
    const refusal = await reportedRefusal(response);
    if (refusal === "session_invalid") throw new GroupSessionInvalidError();
    if (refusal === "authorization_denied") throw new GroupAdministrationAccessDeniedError();
    if (!mutation) {
      throw new GroupsUnavailableError();
    }
    if (refusal === "conflict") throw new LastAdministratorRefusedError();
    if (refusal !== null) throw new GroupMutationRefusedError();
    throw new GroupMutationIndeterminateError();
  }
  try {
    return await response.json();
  } catch {
    throw mutation ? new GroupMutationIndeterminateError() : new GroupsUnavailableError();
  }
}
async function reportedRefusal(response: Response): Promise<string | null> {
  const allowed = new Map<number, ReadonlySet<string>>([
    [400, new Set(["bad_request"])],
    [401, new Set(["session_invalid"])],
    [403, new Set(["request_origin_denied", "authorization_denied", "grant_mutation_denied"])],
    [404, new Set(["not_found"])],
    [405, new Set(["method_not_allowed"])],
    [409, new Set(["conflict"])],
    [503, new Set(["service_unavailable"])],
  ]).get(response.status);
  if (allowed === undefined) return null;
  try {
    const value = objectValue(await response.json());
    return value !== null &&
      Object.keys(value).length === 2 &&
      typeof value.error === "string" &&
      allowed.has(value.error) &&
      typeof value.correlation_id === "string" &&
      CORRELATION_PATTERN.test(value.correlation_id)
      ? value.error
      : null;
  } catch {
    return null;
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

export async function listGroupMembers(
  groupPublicId: string,
  cursor?: string,
): Promise<GroupMembersPage> {
  if (!validId(groupPublicId)) throw new GroupsUnavailableError();
  const page = readGroupMembersPage(
    await request(
      GROUP_MEMBERS_LIST_PATH,
      { group_public_id: groupPublicId, ...(cursor === undefined ? {} : { cursor }) },
      false,
    ),
  );
  if (page === null) throw new GroupsUnavailableError();
  return page;
}

export async function changeGroupMember(
  groupPublicId: string,
  accountPublicId: string,
  present: boolean,
  ticket: string,
): Promise<AccountProjection> {
  if (!validId(groupPublicId) || !validId(accountPublicId) || !TICKET_PATTERN.test(ticket))
    throw new GroupMutationRefusedError();
  const result = objectValue(
    envelope(
      await request(
        GROUP_MEMBERS_CHANGE_PATH,
        {
          group_public_id: groupPublicId,
          account_public_id: accountPublicId,
          present,
          grant_mutation_step_up_ticket: ticket,
        },
        true,
      ),
    )?.result,
  );
  if (result === null || !exact(result, MEMBER_CHANGE_FIELDS) || result.present !== present)
    throw new GroupMutationIndeterminateError();
  const account = readAccountProjectionValue(result.account);
  if (account?.publicId !== accountPublicId) throw new GroupMutationIndeterminateError();
  return account;
}

export async function listGroupGrants(
  groupPublicId: string,
  cursor?: string,
): Promise<GroupGrantsPage> {
  if (!validId(groupPublicId)) throw new GroupsUnavailableError();
  const page = readGroupGrantsPage(
    await request(
      GROUP_GRANTS_LIST_PATH,
      { group_public_id: groupPublicId, ...(cursor === undefined ? {} : { cursor }) },
      false,
    ),
  );
  if (page === null) throw new GroupsUnavailableError();
  return page;
}

function sameGrant(left: GroupGrant | null, right: GroupGrant): boolean {
  if (left?.type !== right.type) return false;
  switch (right.type) {
    case "server_administration":
      return left.type === "server_administration";
    case "client_module":
      return left.type === "client_module" && left.value === right.value;
    case "service_module":
      return left.type === "service_module" && left.value === right.value;
    case "operation":
      return left.type === "operation" && left.value === right.value;
  }
}

export async function changeGroupGrant(
  groupPublicId: string,
  grant: GroupGrant,
  present: boolean,
  ticket: string,
): Promise<void> {
  if (!validId(groupPublicId) || readGroupGrant(grant) === null || !TICKET_PATTERN.test(ticket))
    throw new GroupMutationRefusedError();
  const result = objectValue(
    envelope(
      await request(
        GROUP_GRANTS_CHANGE_PATH,
        {
          group_public_id: groupPublicId,
          grant,
          present,
          grant_mutation_step_up_ticket: ticket,
        },
        true,
      ),
    )?.result,
  );
  if (
    result === null ||
    !exact(result, GRANT_CHANGE_FIELDS) ||
    result.present !== present ||
    !sameGrant(readGroupGrant(result.grant), grant)
  )
    throw new GroupMutationIndeterminateError();
}

export async function loadAdministrationCatalog(): Promise<AdministrationCatalog> {
  const catalog = readAdministrationCatalog(await request(ADMINISTRATION_CATALOG_PATH, {}, false));
  if (catalog === null) throw new GroupsUnavailableError();
  return catalog;
}
