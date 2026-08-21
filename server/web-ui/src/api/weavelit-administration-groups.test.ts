import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME } from "./weavelit-authentication";
import {
  ADMINISTRATION_CATALOG_PATH,
  GROUP_GRANTS_CHANGE_PATH,
  GROUP_GRANTS_LIST_PATH,
  GROUP_MEMBERS_CHANGE_PATH,
  GROUP_MEMBERS_LIST_PATH,
  GROUPS_CREATE_PATH,
  GROUPS_DELETE_PATH,
  GROUPS_LIST_PATH,
  GroupMutationIndeterminateError,
  GroupMutationRefusedError,
  LastAdministratorRefusedError,
  changeGroupGrant,
  changeGroupMember,
  createGroup,
  deleteGroup,
  listGroups,
  listGroupGrants,
  listGroupMembers,
  loadAdministrationCatalog,
  readAdministrationCatalog,
  readGroupGrantsPage,
  readGroupMembersPage,
  readGroupProjection,
  readGroupsPage,
} from "./weavelit-administration-groups";

const ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const ACCOUNT_ID = "QkJCQkJCQkJCQkJCQkJCQg";
const TICKET = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const CORRELATION = "groups-correlation";
const CSRF = "csrf-token";
function projection(): Record<string, unknown> {
  return { public_id: ID, name: "Operators", description: null };
}
function account(): Record<string, unknown> {
  return {
    public_id: ACCOUNT_ID,
    username: "target-user",
    display_name: null,
    active: true,
    mfa_required: false,
  };
}
function response(result: unknown, status = 200): Response {
  return new Response(JSON.stringify(result), {
    status,
    headers: { "content-type": "application/json" },
  });
}
function csrf(): void {
  Object.defineProperty(document, "cookie", {
    configurable: true,
    get: () => `${CSRF_COOKIE_NAME}=${CSRF}`,
  });
}
function body(init?: RequestInit): Record<string, unknown> {
  const value = init?.body;
  if (typeof value !== "string") throw new TypeError("expected a string request body");
  return JSON.parse(value) as Record<string, unknown>;
}
afterEach(() => Reflect.deleteProperty(document, "cookie"));

describe("Group API", () => {
  it("accepts exact projections and rejects extra or internal fields", () => {
    expect(readGroupProjection({ result: projection(), correlation_id: CORRELATION })).toEqual({
      publicId: ID,
      name: "Operators",
      description: null,
    });
    expect(
      readGroupProjection({
        result: { ...projection(), group_id: "internal" },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
    expect(
      readGroupProjection({
        result: { ...projection(), audit_reference: "ar-secret" },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
    expect(
      readGroupsPage({
        result: { items: [projection()], next_cursor: null },
        correlation_id: CORRELATION,
      })?.items,
    ).toHaveLength(1);
  });

  it("rejects public ids with non-canonical base64url final characters", () => {
    expect(
      readGroupProjection({
        result: { public_id: "QUFBQUFBQUFBQUFBQUFBQUE", name: "Test", description: null },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
  });

  it("accepts public ids with canonical base64url final characters", () => {
    const canonicalEndings = ["A", "Q", "g", "w"];
    for (const ending of canonicalEndings) {
      const publicIdWithEnding = "QUFBQUFBQUFBQUFBQUFBU" + ending;
      expect(
        readGroupProjection({
          result: { public_id: publicIdWithEnding, name: "Test", description: null },
          correlation_id: CORRELATION,
        }),
      ).not.toBeNull();
    }
  });

  it("uses one same-origin PUT per operation with only documented fields", async () => {
    csrf();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUPS_LIST_PATH)
        return Promise.resolve(
          response({
            result: { items: [projection()], next_cursor: null },
            correlation_id: CORRELATION,
          }),
        );
      if (target === GROUPS_CREATE_PATH)
        return Promise.resolve(response({ result: projection(), correlation_id: CORRELATION }));
      return Promise.resolve(response({ result: { public_id: ID }, correlation_id: CORRELATION }));
    });
    await listGroups();
    await createGroup("Operators", null);
    await deleteGroup(ID, TICKET);
    expect(fetchMock).toHaveBeenCalledTimes(3);
    expect(fetchMock.mock.calls[2]![0]).toBe(GROUPS_DELETE_PATH);
    expect(body(fetchMock.mock.calls[1]![1])).toEqual({ name: "Operators", description: null });
    expect(body(fetchMock.mock.calls[2]![1])).toEqual({
      public_id: ID,
      grant_mutation_step_up_ticket: TICKET,
    });
    for (const [, init] of fetchMock.mock.calls)
      expect(init).toMatchObject({
        method: "PUT",
        credentials: "same-origin",
        cache: "no-store",
        redirect: "error",
      });
  });

  it("accepts only exact safe member grant and catalog projections", () => {
    expect(
      readGroupMembersPage({
        result: { items: [account()], next_cursor: null },
        correlation_id: CORRELATION,
      })?.items,
    ).toEqual([
      {
        publicId: ACCOUNT_ID,
        username: "target-user",
        displayName: null,
        active: true,
        mfaRequired: false,
      },
    ]);
    expect(
      readGroupMembersPage({
        result: { items: [{ ...account(), audit_reference: "ar-secret" }], next_cursor: null },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
    expect(
      readGroupGrantsPage({
        result: {
          items: [{ type: "client_module", value: "web-ui" }, { type: "server_administration" }],
          next_cursor: null,
        },
        correlation_id: CORRELATION,
      })?.items,
    ).toEqual([{ type: "client_module", value: "web-ui" }, { type: "server_administration" }]);
    expect(
      readGroupGrantsPage({
        result: {
          items: [{ type: "server_administration", value: "unsafe" }],
          next_cursor: null,
        },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
    expect(
      readAdministrationCatalog({
        result: { client_modules: ["web-ui"], service_modules: [], operations: [] },
        correlation_id: CORRELATION,
      }),
    ).toEqual({ clientModules: ["web-ui"], serviceModules: [], operations: [] });
    expect(
      readAdministrationCatalog({
        result: { client_modules: ["zeta", "alpha"], service_modules: [], operations: [] },
        correlation_id: CORRELATION,
      }),
    ).toBeNull();
  });

  it("uses structured public targets and one proof-bound request per association change", async () => {
    csrf();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUP_MEMBERS_LIST_PATH)
        return Promise.resolve(
          response({
            result: { items: [account()], next_cursor: null },
            correlation_id: CORRELATION,
          }),
        );
      if (target === GROUP_MEMBERS_CHANGE_PATH)
        return Promise.resolve(
          response({
            result: { account: account(), present: true },
            correlation_id: CORRELATION,
          }),
        );
      if (target === GROUP_GRANTS_LIST_PATH)
        return Promise.resolve(
          response({
            result: { items: [{ type: "client_module", value: "web-ui" }], next_cursor: null },
            correlation_id: CORRELATION,
          }),
        );
      if (target === GROUP_GRANTS_CHANGE_PATH)
        return Promise.resolve(
          response({
            result: { grant: { type: "client_module", value: "web-ui" }, present: true },
            correlation_id: CORRELATION,
          }),
        );
      return Promise.resolve(
        response({
          result: { client_modules: ["web-ui"], service_modules: [], operations: [] },
          correlation_id: CORRELATION,
        }),
      );
    });
    await listGroupMembers(ID);
    await changeGroupMember(ID, ACCOUNT_ID, true, TICKET);
    await listGroupGrants(ID);
    await changeGroupGrant(ID, { type: "client_module", value: "web-ui" }, true, TICKET);
    await loadAdministrationCatalog();
    expect(fetchMock).toHaveBeenCalledTimes(5);
    expect(body(fetchMock.mock.calls[0]![1])).toEqual({ group_public_id: ID });
    expect(body(fetchMock.mock.calls[1]![1])).toEqual({
      group_public_id: ID,
      account_public_id: ACCOUNT_ID,
      present: true,
      grant_mutation_step_up_ticket: TICKET,
    });
    expect(body(fetchMock.mock.calls[3]![1])).toEqual({
      group_public_id: ID,
      grant: { type: "client_module", value: "web-ui" },
      present: true,
      grant_mutation_step_up_ticket: TICKET,
    });
    expect(fetchMock.mock.calls[4]![0]).toBe(ADMINISTRATION_CATALOG_PATH);
  });

  it("distinguishes reported refusal from unknown mutation outcome and never retries", async () => {
    csrf();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(response({ error: "conflict", correlation_id: CORRELATION }, 409));
    await expect(createGroup("Operators", null)).rejects.toBeInstanceOf(GroupMutationRefusedError);
    fetchMock.mockRejectedValueOnce(new TypeError("network"));
    await expect(deleteGroup(ID, TICKET)).rejects.toBeInstanceOf(GroupMutationIndeterminateError);
    fetchMock.mockResolvedValueOnce(
      response({ error: "conflict", correlation_id: CORRELATION }, 409),
    );
    await expect(
      changeGroupGrant(ID, { type: "server_administration" }, false, TICKET),
    ).rejects.toBeInstanceOf(LastAdministratorRefusedError);
    expect(fetchMock).toHaveBeenCalledTimes(3);
  });
});
