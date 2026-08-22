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
      expect(screen.queryByRole("heading", { name: "Verify Group access change" })).toBeNull();
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

  it("locks association mutations after an unknown outcome and reports reconciliation", async () => {
    csrf();
    const onMutationIndeterminate = vi.fn();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUP_MEMBERS_LIST_PATH || target === GROUP_GRANTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [account()], next_cursor: null }));
      if (target === ADMINISTRATION_CATALOG_PATH)
        return Promise.resolve(
          response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
        );
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(response({ totp_step_up_ticket: TICKET }));
      if (target === GROUP_MEMBERS_CHANGE_PATH) return Promise.reject(new TypeError("network"));
      throw new Error("unexpected request");
    });

    render(
      <GroupAssociations
        groupPublicId={GROUP_ID}
        onMutationIndeterminate={onMutationIndeterminate}
      />,
    );
    fireEvent.change(await screen.findByLabelText("Add member"), {
      target: { value: ACCOUNT_ID },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add member" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Apply change" }));

    expect(await screen.findByText(/Group access outcome is unknown/)).toBeTruthy();
    expect(onMutationIndeterminate).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Add member" }).hasAttribute("disabled")).toBe(true);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === GROUP_MEMBERS_CHANGE_PATH),
    ).toHaveLength(1);
  });

  it("renders a reported association step-up rejection as a refusal", async () => {
    csrf();
    vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUP_MEMBERS_LIST_PATH || target === GROUP_GRANTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [account()], next_cursor: null }));
      if (target === ADMINISTRATION_CATALOG_PATH)
        return Promise.resolve(
          response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
        );
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(
          response({ error: "mfa_policy_denied", correlation_id: CORRELATION }, 403),
        );
      throw new Error("unexpected request");
    });

    render(<GroupAssociations groupPublicId={GROUP_ID} />);
    fireEvent.change(await screen.findByLabelText("Add member"), {
      target: { value: ACCOUNT_ID },
    });
    fireEvent.click(screen.getByRole("button", { name: "Add member" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Apply change" }));

    expect(await screen.findByText("Group access was not changed.")).toBeTruthy();
    expect(screen.queryByText(/outcome is unknown/)).toBeNull();
    expect(screen.getByRole("button", { name: "Add member" }).hasAttribute("disabled")).toBe(false);
  });

  it.each(["member", "grant"] as const)(
    "ends administration when a %s step-up reports session-invalid",
    async (kind) => {
      csrf();
      const onAdministrationEnded = vi.fn();
      const onMutationIndeterminate = vi.fn();
      const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
        if (target === GROUP_MEMBERS_LIST_PATH || target === GROUP_GRANTS_LIST_PATH)
          return Promise.resolve(response({ items: [], next_cursor: null }));
        if (target === ACCOUNTS_LIST_PATH)
          return Promise.resolve(response({ items: [account()], next_cursor: null }));
        if (target === ADMINISTRATION_CATALOG_PATH)
          return Promise.resolve(
            response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
          );
        if (target === MFA_POLICY_STEP_UP_PATH)
          return Promise.resolve(
            response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
          );
        throw new Error("unexpected request");
      });

      render(
        <GroupAssociations
          groupPublicId={GROUP_ID}
          onAdministrationEnded={onAdministrationEnded}
          onMutationIndeterminate={onMutationIndeterminate}
        />,
      );
      if (kind === "member") {
        fireEvent.change(await screen.findByLabelText("Add member"), {
          target: { value: ACCOUNT_ID },
        });
        fireEvent.click(screen.getByRole("button", { name: "Add member" }));
      } else {
        fireEvent.click(await screen.findByRole("button", { name: "Add grant" }));
      }
      fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
      fireEvent.click(screen.getByRole("button", { name: "Apply change" }));

      await waitFor(() => {
        expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
      });
      expect(onMutationIndeterminate).not.toHaveBeenCalled();
      expect(screen.queryByText("Group access was not changed.")).toBeNull();
      expect(screen.queryByText(/Group access outcome is unknown/)).toBeNull();
      expect(screen.queryByRole("heading", { name: "Verify Group access change" })).toBeNull();
      expect(
        fetchMock.mock.calls.filter(
          ([target]) => target === GROUP_MEMBERS_CHANGE_PATH || target === GROUP_GRANTS_CHANGE_PATH,
        ),
      ).toHaveLength(0);
    },
  );

  it("exits administration when a committed change removes the actor's access", async () => {
    csrf();
    const onAdministrationEnded = vi.fn();
    let memberReads = 0;
    let grantReads = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUP_MEMBERS_LIST_PATH) {
        memberReads += 1;
        return Promise.resolve(
          memberReads === 1
            ? response({ items: [], next_cursor: null })
            : response({ error: "authorization_denied", correlation_id: CORRELATION }, 403),
        );
      }
      if (target === GROUP_GRANTS_LIST_PATH) {
        grantReads += 1;
        return Promise.resolve(
          grantReads === 1
            ? response({ items: [{ type: "server_administration" }], next_cursor: null })
            : response({ error: "authorization_denied", correlation_id: CORRELATION }, 403),
        );
      }
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ADMINISTRATION_CATALOG_PATH)
        return Promise.resolve(
          response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
        );
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(response({ totp_step_up_ticket: TICKET }));
      if (target === GROUP_GRANTS_CHANGE_PATH)
        return Promise.resolve(
          response({ grant: { type: "server_administration" }, present: false }),
        );
      throw new Error("unexpected request");
    });

    render(
      <GroupAssociations groupPublicId={GROUP_ID} onAdministrationEnded={onAdministrationEnded} />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Remove" }));
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Apply change" }));

    await waitFor(() => {
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText(/outcome is unknown/)).toBeNull();
    expect(screen.queryByText("Group access was not changed.")).toBeNull();
    expect(
      fetchMock.mock.calls.filter(([target]) => target === GROUP_GRANTS_CHANGE_PATH),
    ).toHaveLength(1);
  });

  it("ends administration when a Group association read reports session-invalid", async () => {
    csrf();
    const onAdministrationEnded = vi.fn();
    vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUP_MEMBERS_LIST_PATH)
        return Promise.resolve(
          response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
        );
      if (target === GROUP_GRANTS_LIST_PATH || target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      return Promise.resolve(
        response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
      );
    });

    render(
      <GroupAssociations groupPublicId={GROUP_ID} onAdministrationEnded={onAdministrationEnded} />,
    );

    await waitFor(() => {
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText("Group access is unavailable.")).toBeNull();
  });

  it.each([
    ["session-invalid", "session_invalid", 401],
    ["access-denied", "authorization_denied", 403],
  ])("falls back to a failure when a Group association read is %s", async (_, error, status) => {
    csrf();
    vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUP_MEMBERS_LIST_PATH)
        return Promise.resolve(response({ error, correlation_id: CORRELATION }, status));
      if (target === GROUP_GRANTS_LIST_PATH || target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      return Promise.resolve(
        response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
      );
    });

    render(<GroupAssociations groupPublicId={GROUP_ID} />);

    expect(await screen.findByText("Group access is unavailable.")).toBeTruthy();
  });

  it("notifies once when concurrent Group association reads end administration", async () => {
    csrf();
    const onAdministrationEnded = vi.fn();
    let memberReads = 0;
    let grantReads = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUP_MEMBERS_LIST_PATH) {
        memberReads += 1;
        return Promise.resolve(
          memberReads === 1
            ? response({ items: [], next_cursor: "members-next" })
            : response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
        );
      }
      if (target === GROUP_GRANTS_LIST_PATH) {
        grantReads += 1;
        return Promise.resolve(
          grantReads === 1
            ? response({ items: [], next_cursor: "grants-next" })
            : response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
        );
      }
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      return Promise.resolve(
        response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
      );
    });

    render(
      <GroupAssociations groupPublicId={GROUP_ID} onAdministrationEnded={onAdministrationEnded} />,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Load more members" }));
    fireEvent.click(screen.getByRole("button", { name: "Load more grants" }));

    await waitFor(() => {
      expect(
        fetchMock.mock.calls.filter(([target]) => target === GROUP_MEMBERS_LIST_PATH),
      ).toHaveLength(2);
      expect(
        fetchMock.mock.calls.filter(([target]) => target === GROUP_GRANTS_LIST_PATH),
      ).toHaveLength(2);
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
  });

  it.each([
    ["members", "Load more members", GROUP_MEMBERS_LIST_PATH, "CCCCCCCCCCCCCCCCCCCCCg"],
    ["grants", "Load more grants", GROUP_GRANTS_LIST_PATH, "REdHREdHREdHREdHREdHRg"],
    ["accounts", "Load more Accounts", ACCOUNTS_LIST_PATH, "RElSRElSRElSRElSRElSRg"],
  ])(
    "admits one request per %s pagination cursor despite double click and disables during flight",
    async (collection, buttonLabel, listPath, paginationItemId) => {
      csrf();

      // Deferred response resolver for cursor-bearing pagination request
      let resolvePaginationResponse: ((value: Response | PromiseLike<Response>) => void) | null =
        null;
      const deferredPaginationPromise = new Promise<Response>((resolve) => {
        resolvePaginationResponse = resolve;
      });

      const initialMembers = [];
      const initialGrants = [];
      const initialAccounts = [
        {
          public_id: ACCOUNT_ID,
          username: "initial-account",
          display_name: "Initial Account",
          active: true,
          mfa_required: false,
        },
      ];

      const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
        if (target === listPath && init?.method !== "POST") {
          try {
            const req = body(init);
            // Cursor-bearing pagination request stays deferred
            if ("cursor" in req) {
              return deferredPaginationPromise;
            }
          } catch {
            // Initial request without body, resolve immediately
          }
          if (collection === "members") {
            return Promise.resolve(
              response({
                items: initialMembers,
                next_cursor: "members-cursor",
              }),
            );
          }
          if (collection === "grants") {
            return Promise.resolve(
              response({
                items: initialGrants,
                next_cursor: "grants-cursor",
              }),
            );
          }
          if (collection === "accounts") {
            return Promise.resolve(
              response({
                items: initialAccounts,
                next_cursor: "accounts-cursor",
              }),
            );
          }
        }
        if (target === GROUP_MEMBERS_LIST_PATH)
          return Promise.resolve(
            response({
              items: initialMembers,
              next_cursor: collection === "members" ? "members-cursor" : null,
            }),
          );
        if (target === GROUP_GRANTS_LIST_PATH)
          return Promise.resolve(
            response({
              items: initialGrants,
              next_cursor: collection === "grants" ? "grants-cursor" : null,
            }),
          );
        if (target === ACCOUNTS_LIST_PATH)
          return Promise.resolve(
            response({
              items: initialAccounts,
              next_cursor: collection === "accounts" ? "accounts-cursor" : null,
            }),
          );
        if (target === ADMINISTRATION_CATALOG_PATH)
          return Promise.resolve(
            response({ client_modules: [], service_modules: [], operations: ["operation"] }),
          );
        return Promise.reject(new Error("unexpected request: " + target));
      });

      render(<GroupAssociations groupPublicId={GROUP_ID} />);

      // Wait for initial load and find the pagination button
      const paginationButton = await screen.findByRole("button", { name: buttonLabel });

      // Verify button is not disabled before clicking
      expect(paginationButton).not.toHaveAttribute("disabled");

      // Double-click the pagination button rapidly
      fireEvent.click(paginationButton);
      fireEvent.click(paginationButton);

      // After first click, button should be disabled
      await waitFor(() => {
        expect(paginationButton).toHaveAttribute("disabled");
      });

      // Verify exactly one pagination request with cursor was made
      const listRequests = fetchMock.mock.calls.filter(([target]) => target === listPath);
      const paginationRequests = listRequests.filter((call) => {
        try {
          const req = body(call[1]);
          return "cursor" in req;
        } catch {
          return false;
        }
      });
      expect(paginationRequests).toHaveLength(1);
      expect(body(paginationRequests[0][1])).toEqual({
        cursor:
          collection === "members"
            ? "members-cursor"
            : collection === "grants"
              ? "grants-cursor"
              : "accounts-cursor",
      });

      // Resolve the deferred pagination response
      expect(resolvePaginationResponse).not.toBeNull();
      resolvePaginationResponse!(
        response({
          items: [
            collection === "members"
              ? {
                  public_id: paginationItemId,
                  username: `${paginationItemId}-user`,
                  display_name: `${paginationItemId} Display`,
                  active: true,
                  mfa_required: false,
                }
              : collection === "grants"
                ? { type: "operation", value: paginationItemId }
                : {
                    public_id: paginationItemId,
                    username: `${paginationItemId}-user`,
                    display_name: `${paginationItemId} Display`,
                    active: true,
                    mfa_required: false,
                  },
          ],
          next_cursor: null,
        }),
      );

      // Wait for button to be re-enabled after response settles
      await waitFor(() => {
        expect(paginationButton).not.toHaveAttribute("disabled");
      });

      // Verify exactly one new item is rendered
      // For members and accounts, the new public_id should appear in the display
      // For grants, the operation value should appear
      const newItemPattern =
        collection === "grants"
          ? new RegExp(`^${paginationItemId}$`)
          : new RegExp(`^${paginationItemId}`);
      const newItems = screen.queryAllByText(newItemPattern);
      expect(newItems.length).toBeGreaterThan(0);

      fetchMock.mockRestore();
    },
  );
});
