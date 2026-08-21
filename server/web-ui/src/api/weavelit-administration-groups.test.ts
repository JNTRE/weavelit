import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME } from "./weavelit-authentication";
import {
  GROUPS_CREATE_PATH,
  GROUPS_DELETE_PATH,
  GROUPS_LIST_PATH,
  GroupMutationIndeterminateError,
  GroupMutationRefusedError,
  createGroup,
  deleteGroup,
  listGroups,
  readGroupProjection,
  readGroupsPage,
} from "./weavelit-administration-groups";

const ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const TICKET = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const CORRELATION = "groups-correlation";
const CSRF = "csrf-token";
function projection(): Record<string, unknown> {
  return { public_id: ID, name: "Operators", description: null };
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

  it("distinguishes reported refusal from unknown mutation outcome and never retries", async () => {
    csrf();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValueOnce(response({ error: "conflict", correlation_id: CORRELATION }, 409));
    await expect(createGroup("Operators", null)).rejects.toBeInstanceOf(GroupMutationRefusedError);
    fetchMock.mockRejectedValueOnce(new TypeError("network"));
    await expect(deleteGroup(ID, TICKET)).rejects.toBeInstanceOf(GroupMutationIndeterminateError);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});
