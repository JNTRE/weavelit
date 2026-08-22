import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { ACCOUNTS_LIST_PATH } from "../api/weavelit-administration-accounts";
import {
  ADMINISTRATION_CATALOG_PATH,
  GROUP_GRANTS_CHANGE_PATH,
  GROUP_GRANTS_LIST_PATH,
  GROUP_MEMBERS_CHANGE_PATH,
  GROUP_MEMBERS_LIST_PATH,
} from "../api/weavelit-administration-groups";
import { CSRF_COOKIE_NAME } from "../api/weavelit-authentication";
import { MFA_POLICY_STEP_UP_PATH } from "../api/weavelit-mfa-policy";
import { GroupAssociations } from "./weavelit-group-associations";

const GROUP_ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const ACCOUNT_ID = "QkJCQkJCQkJCQkJCQkJCQg";
const TICKET = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const CORRELATION = "group-associations";
const CODE = "123456";

function account(): Record<string, unknown> {
  return {
    public_id: ACCOUNT_ID,
    username: "target-user",
    display_name: "Target User",
    active: true,
    mfa_required: false,
  };
}

function response(result: unknown, status = 200): Response {
  return new Response(
    JSON.stringify(status === 200 ? { result, correlation_id: CORRELATION } : result),
    { status, headers: { "content-type": "application/json" } },
  );
}

function body(init?: RequestInit): Record<string, unknown> {
  if (typeof init?.body !== "string") throw new TypeError("expected a request body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

function csrf(): void {
  Object.defineProperty(document, "cookie", {
    configurable: true,
    get: () => `${CSRF_COOKIE_NAME}=csrf-token`,
  });
}

afterEach(() => Reflect.deleteProperty(document, "cookie"));

describe("GroupAssociations", () => {
  it("uses safe selectors and one memory-only ticket to add a member then refresh", async () => {
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
      if (target === GROUP_MEMBERS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === GROUP_GRANTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [account()], next_cursor: null }));
      if (target === ADMINISTRATION_CATALOG_PATH)
        return Promise.resolve(
          response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
        );
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(response({ totp_step_up_ticket: TICKET }));
      if (target === GROUP_MEMBERS_CHANGE_PATH)
        return Promise.resolve(response({ account: account(), present: true }));
      throw new Error("unexpected request");
    });

    render(<GroupAssociations groupPublicId={GROUP_ID} />);
    const memberSelect = await screen.findByLabelText("Add member");
    fireEvent.change(memberSelect, { target: { value: ACCOUNT_ID } });
    fireEvent.click(screen.getByRole("button", { name: "Add member" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Apply change" }));

    await waitFor(() => {
      expect(fetchMock.mock.calls.some(([target]) => target === GROUP_MEMBERS_CHANGE_PATH)).toBe(
        true,
      );
    });
    const stepUp = fetchMock.mock.calls.find(([target]) => target === MFA_POLICY_STEP_UP_PATH)!;
    const change = fetchMock.mock.calls.find(([target]) => target === GROUP_MEMBERS_CHANGE_PATH)!;
    expect(body(stepUp[1])).toEqual({ family: "grant_mutation", code: CODE });
    expect(body(change[1])).toEqual({
      group_public_id: GROUP_ID,
      account_public_id: ACCOUNT_ID,
      present: true,
      grant_mutation_step_up_ticket: TICKET,
    });
    expect(
      fetchMock.mock.calls.filter(([target]) => target === MFA_POLICY_STEP_UP_PATH),
    ).toHaveLength(1);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === GROUP_MEMBERS_CHANGE_PATH),
    ).toHaveLength(1);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === GROUP_MEMBERS_LIST_PATH),
    ).toHaveLength(2);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === GROUP_GRANTS_LIST_PATH),
    ).toHaveLength(2);
    expect(storageSpy).not.toHaveBeenCalled();
    expect(window.location.href).not.toContain(CODE);
    expect(window.location.href).not.toContain(TICKET);
    expect(screen.queryByRole("textbox", { name: /grant target/i })).toBeNull();
  });

  it("confirms removal client-side and renders the stable last-administrator wording", async () => {
    csrf();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUP_MEMBERS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === GROUP_GRANTS_LIST_PATH)
        return Promise.resolve(
          response({ items: [{ type: "server_administration" }], next_cursor: null }),
        );
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ADMINISTRATION_CATALOG_PATH)
        return Promise.resolve(
          response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
        );
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(response({ totp_step_up_ticket: TICKET }));
      if (target === GROUP_GRANTS_CHANGE_PATH)
        return Promise.resolve(response({ error: "conflict", correlation_id: CORRELATION }, 409));
      throw new Error("unexpected request");
    });

    render(<GroupAssociations groupPublicId={GROUP_ID} />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    expect(screen.getByRole("heading", { name: "Confirm removal" })).not.toBeNull();
    expect(fetchMock.mock.calls.some(([target]) => target === GROUP_GRANTS_CHANGE_PATH)).toBe(
      false,
    );
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Apply change" }));

    expect(
      await screen.findByText("Cannot remove the last Server Administration Permission grant."),
    ).not.toBeNull();
    const change = fetchMock.mock.calls.find(([target]) => target === GROUP_GRANTS_CHANGE_PATH)!;
    expect(body(change[1])).toEqual({
      group_public_id: GROUP_ID,
      grant: { type: "server_administration" },
      present: false,
      grant_mutation_step_up_ticket: TICKET,
    });
    expect(body(change[1])).not.toHaveProperty("confirmed");
    expect(
      fetchMock.mock.calls.filter(([target]) => target === GROUP_GRANTS_CHANGE_PATH),
    ).toHaveLength(1);
  });
});
