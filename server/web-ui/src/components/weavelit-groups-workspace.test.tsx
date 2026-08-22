import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME } from "../api/weavelit-authentication";
import {
  GROUPS_DELETE_PATH,
  GROUPS_LIST_PATH,
  GROUPS_VIEW_PATH,
} from "../api/weavelit-administration-groups";
import { MFA_POLICY_STEP_UP_PATH } from "../api/weavelit-mfa-policy";
import { GroupsWorkspace } from "./weavelit-groups-workspace";

const ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const TICKET = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const CORRELATION = "groups-correlation";
const CODE = "123456";
function group(): Record<string, unknown> {
  return { public_id: ID, name: "Operators", description: null };
}
function response(result: unknown): Response {
  return new Response(JSON.stringify({ result, correlation_id: CORRELATION }), { status: 200 });
}
function requestBody(init?: RequestInit): Record<string, unknown> {
  const value = init?.body;
  if (typeof value !== "string") throw new TypeError("expected a string request body");
  return JSON.parse(value) as Record<string, unknown>;
}
function csrf(): void {
  Object.defineProperty(document, "cookie", {
    configurable: true,
    get: () => `${CSRF_COOKIE_NAME}=csrf-token`,
  });
}
afterEach(() => Reflect.deleteProperty(document, "cookie"));

describe("GroupsWorkspace", () => {
  it("loads and views only public Group projections", async () => {
    csrf();
    vi.spyOn(globalThis, "fetch").mockImplementation((target) =>
      Promise.resolve(
        target === GROUPS_VIEW_PATH
          ? response(group())
          : response({ items: [group()], next_cursor: null }),
      ),
    );
    render(<GroupsWorkspace />);
    fireEvent.click(await screen.findByRole("button", { name: "View" }));
    expect(await screen.findByText(ID)).not.toBeNull();
    expect(screen.queryByText(/audit_reference|group_id/i)).toBeNull();
  });

  it("requires confirmation and one GrantMutation step-up without storing or retrying secrets", async () => {
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    const storageSpy = vi.spyOn(Storage.prototype, "setItem");
    globalThis.sessionStorage.setItem("sanity-check", "safe-value");
    expect(storageSpy).toHaveBeenCalledWith("sanity-check", "safe-value");
    expect(globalThis.sessionStorage.length).toBe(1);
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    storageSpy.mockClear();

    csrf();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUPS_LIST_PATH)
        return Promise.resolve(response({ items: [group()], next_cursor: null }));
      if (target === GROUPS_VIEW_PATH) return Promise.resolve(response(group()));
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(response({ totp_step_up_ticket: TICKET }));
      if (target === GROUPS_DELETE_PATH) return Promise.resolve(response({ public_id: ID }));
      throw new Error("unexpected request");
    });
    render(<GroupsWorkspace />);
    fireEvent.click(await screen.findByRole("button", { name: "View" }));
    await screen.findByText(ID);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(screen.getByRole("heading", { name: "Delete Operators?" })).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Delete Group" }));
    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([target]) => target === GROUPS_DELETE_PATH)).toBe(true);
    });
    const stepUp = fetchMock.mock.calls.find(([target]) => target === MFA_POLICY_STEP_UP_PATH)!;
    const deletion = fetchMock.mock.calls.find(([target]) => target === GROUPS_DELETE_PATH)!;
    expect(requestBody(stepUp[1])).toEqual({ family: "grant_mutation", code: CODE });
    expect(requestBody(deletion[1])).toEqual({
      public_id: ID,
      grant_mutation_step_up_ticket: TICKET,
    });
    expect(storageSpy).not.toHaveBeenCalled();
    expect(window.location.href).not.toContain(CODE);
    expect(window.location.href).not.toContain(TICKET);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === MFA_POLICY_STEP_UP_PATH),
    ).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === GROUPS_DELETE_PATH)).toHaveLength(
      1,
    );
  });
});
