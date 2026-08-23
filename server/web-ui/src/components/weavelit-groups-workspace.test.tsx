import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME } from "../api/weavelit-authentication";
import { ACCOUNTS_LIST_PATH } from "../api/weavelit-administration-accounts";
import {
  ADMINISTRATION_CATALOG_PATH,
  GROUP_GRANTS_LIST_PATH,
  GROUP_MEMBERS_LIST_PATH,
  GROUPS_CREATE_PATH,
  GROUPS_DELETE_PATH,
  GROUPS_LIST_PATH,
  GROUPS_UPDATE_PATH,
  GROUPS_VIEW_PATH,
} from "../api/weavelit-administration-groups";
import { MFA_POLICY_STEP_UP_PATH } from "../api/weavelit-mfa-policy";
import { GroupsWorkspace } from "./weavelit-groups-workspace";

const ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const OTHER_ID = "QkJCQkJCQkJCQkJCQkJCQg";
const STALE_ID = "Q0NDQ0NDQ0NDQ0NDQ0NDQw";
const TICKET = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const CORRELATION = "groups-correlation";
const CODE = "123456";
function group(): Record<string, unknown> {
  return { public_id: ID, name: "Operators", description: null };
}
function response(result: unknown): Response {
  return new Response(JSON.stringify({ result, correlation_id: CORRELATION }), { status: 200 });
}
function rejection(error: string, status: number): Response {
  return new Response(JSON.stringify({ error, correlation_id: CORRELATION }), { status });
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

  it("ends administration when the Group list reports session-invalid", async () => {
    csrf();
    const onAdministrationEnded = vi.fn();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(rejection("session_invalid", 401));

    render(<GroupsWorkspace onAdministrationEnded={onAdministrationEnded} />);

    await waitFor(() => {
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText("Groups are unavailable.")).toBeNull();
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("ends administration when a Group detail read reports session-invalid", async () => {
    csrf();
    const onAdministrationEnded = vi.fn();
    vi.spyOn(globalThis, "fetch").mockImplementation((target) =>
      Promise.resolve(
        target === GROUPS_VIEW_PATH
          ? rejection("session_invalid", 401)
          : response({ items: [group()], next_cursor: null }),
      ),
    );

    render(<GroupsWorkspace onAdministrationEnded={onAdministrationEnded} />);
    fireEvent.click(await screen.findByRole("button", { name: "View" }));

    await waitFor(() => {
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText("Groups are unavailable.")).toBeNull();
  });

  it.each([
    ["session-invalid", "session_invalid", 401],
    ["access-denied", "authorization_denied", 403],
  ])("falls back to a retryable failure when a Group read is %s", async (_, error, status) => {
    csrf();
    let requests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(() => {
      requests += 1;
      return Promise.resolve(
        requests === 1
          ? rejection(error, status)
          : response({ items: [group()], next_cursor: null }),
      );
    });

    render(<GroupsWorkspace />);

    expect(await screen.findByText("Groups are unavailable.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByRole("button", { name: "View" })).toBeTruthy();
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it("notifies once when concurrent Group reads end administration", async () => {
    csrf();
    const onAdministrationEnded = vi.fn();
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUPS_LIST_PATH) {
        listRequests += 1;
        return Promise.resolve(
          listRequests === 1
            ? response({ items: [group()], next_cursor: "next" })
            : rejection("session_invalid", 401),
        );
      }
      if (target === GROUPS_VIEW_PATH) return Promise.resolve(rejection("session_invalid", 401));
      throw new Error("unexpected request");
    });

    render(<GroupsWorkspace onAdministrationEnded={onAdministrationEnded} />);
    fireEvent.click(await screen.findByRole("button", { name: "View" }));
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(3);
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
  });

  it.each([
    ["create", GROUPS_CREATE_PATH, "session_invalid", 401],
    ["update", GROUPS_UPDATE_PATH, "authorization_denied", 403],
    ["delete", GROUPS_DELETE_PATH, "session_invalid", 401],
  ] as const)(
    "ends administration once when Group %s reports a terminal mutation outcome",
    async (family, mutationPath, error, status) => {
      csrf();
      const onAdministrationEnded = vi.fn();
      const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
        if (target === mutationPath) return Promise.resolve(rejection(error, status));
        if (target === GROUPS_LIST_PATH)
          return Promise.resolve(response({ items: [group()], next_cursor: null }));
        if (target === GROUPS_VIEW_PATH) return Promise.resolve(response(group()));
        if (target === MFA_POLICY_STEP_UP_PATH)
          return Promise.resolve(response({ totp_step_up_ticket: TICKET }));
        if (target === GROUP_MEMBERS_LIST_PATH || target === GROUP_GRANTS_LIST_PATH)
          return Promise.resolve(response({ items: [], next_cursor: null }));
        if (target === ACCOUNTS_LIST_PATH)
          return Promise.resolve(response({ items: [], next_cursor: null }));
        if (target === ADMINISTRATION_CATALOG_PATH)
          return Promise.resolve(
            response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
          );
        throw new Error("unexpected request");
      });

      render(<GroupsWorkspace onAdministrationEnded={onAdministrationEnded} />);
      if (family === "create") {
        await screen.findByRole("button", { name: "View" });
        fireEvent.change(screen.getByLabelText("Name"), { target: { value: "New Group" } });
        fireEvent.click(screen.getByRole("button", { name: "Create" }));
      } else {
        fireEvent.click(await screen.findByRole("button", { name: "View" }));
        await screen.findByText(ID);
        if (family === "update") {
          fireEvent.change(screen.getByDisplayValue("Operators"), {
            target: { value: "Updated Operators" },
          });
          fireEvent.click(screen.getByRole("button", { name: "Update" }));
        } else {
          fireEvent.click(screen.getByRole("button", { name: "Delete" }));
          fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
          fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
          fireEvent.click(screen.getByRole("button", { name: "Delete Group" }));
        }
      }

      await waitFor(() => {
        expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
      });
      expect(screen.queryByText("The Group was not changed.")).toBeNull();
      expect(screen.queryByText(/The Group outcome is unknown/)).toBeNull();
      expect(screen.queryByText("The Group was not deleted.")).toBeNull();
      expect(screen.queryByText(/Group deletion outcome is unknown/)).toBeNull();
      expect(fetchMock.mock.calls.filter(([target]) => target === mutationPath)).toHaveLength(1);
      expect(fetchMock.mock.calls.filter(([target]) => target === GROUPS_LIST_PATH)).toHaveLength(
        1,
      );
      expect(
        fetchMock.mock.calls.filter(([target]) => target === MFA_POLICY_STEP_UP_PATH),
      ).toHaveLength(family === "delete" ? 1 : 0);
    },
  );

  it("ignores a stale page after Refresh replaces the Group collection", async () => {
    csrf();
    let resolveStalePage: (value: Response) => void = () => {};
    const stalePage = new Promise<Response>((resolve) => {
      resolveStalePage = resolve;
    });
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target !== GROUPS_LIST_PATH) throw new Error("unexpected request");
      listRequests += 1;
      if (listRequests === 1)
        return Promise.resolve(response({ items: [group()], next_cursor: "old-next" }));
      if (listRequests === 2) return stalePage;
      if (listRequests === 3)
        return Promise.resolve(
          response({
            items: [{ public_id: OTHER_ID, name: "Auditors", description: null }],
            next_cursor: "fresh-next",
          }),
        );
      return Promise.resolve(response({ items: [], next_cursor: null }));
    });

    render(<GroupsWorkspace />);
    fireEvent.click(await screen.findByRole("button", { name: "Load more" }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));

    expect(await screen.findByText("Auditors")).toBeTruthy();
    resolveStalePage(
      response({
        items: [{ public_id: STALE_ID, name: "Stale page", description: null }],
        next_cursor: "stale-next",
      }),
    );
    await waitFor(() => {
      expect(screen.queryByText("Stale page")).toBeNull();
      expect(screen.getByText("Auditors")).toBeTruthy();
    });

    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledTimes(4);
      expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();
    });
    const listCalls = fetchMock.mock.calls.filter(([target]) => target === GROUPS_LIST_PATH);
    expect(requestBody(listCalls[3]![1]).cursor).toBe("fresh-next");
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
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUPS_LIST_PATH) {
        listRequests += 1;
        return Promise.resolve(
          response({ items: listRequests === 1 ? [group()] : [], next_cursor: null }),
        );
      }
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
      expect(screen.queryByRole("button", { name: "View" })).toBeNull();
      expect(screen.queryByRole("heading", { name: "Verify deletion" })).toBeNull();
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

  it("ignores a stale detail response after a newer Group is selected", async () => {
    csrf();
    let resolveFirst: (value: Response) => void = () => {};
    let resolveSecond: (value: Response) => void = () => {};
    const first = new Promise<Response>((resolve) => {
      resolveFirst = resolve;
    });
    const second = new Promise<Response>((resolve) => {
      resolveSecond = resolve;
    });
    vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
      if (target === GROUPS_LIST_PATH)
        return Promise.resolve(
          response({
            items: [group(), { public_id: OTHER_ID, name: "Auditors", description: null }],
            next_cursor: null,
          }),
        );
      if (target === GROUPS_VIEW_PATH) {
        const publicId = requestBody(init).public_id;
        return publicId === ID ? first : second;
      }
      if (target === GROUP_MEMBERS_LIST_PATH || target === GROUP_GRANTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ADMINISTRATION_CATALOG_PATH)
        return Promise.resolve(
          response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
        );
      throw new Error("unexpected request");
    });

    render(<GroupsWorkspace />);
    const viewButtons = await screen.findAllByRole("button", { name: "View" });
    fireEvent.click(viewButtons[0]!);
    fireEvent.click(viewButtons[1]!);
    resolveSecond(response({ public_id: OTHER_ID, name: "Auditors", description: null }));
    expect(await screen.findByRole("heading", { name: "Auditors" })).toBeTruthy();
    resolveFirst(response(group()));
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Auditors" })).toBeTruthy();
      expect(screen.queryByText(ID)).toBeNull();
    });
  });

  it("locks every Group mutation after an unknown delete until refresh succeeds", async () => {
    csrf();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUPS_LIST_PATH)
        return Promise.resolve(response({ items: [group()], next_cursor: null }));
      if (target === GROUPS_VIEW_PATH) return Promise.resolve(response(group()));
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(response({ totp_step_up_ticket: TICKET }));
      if (target === GROUPS_DELETE_PATH) return Promise.reject(new TypeError("network"));
      if (target === GROUP_MEMBERS_LIST_PATH || target === GROUP_GRANTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ADMINISTRATION_CATALOG_PATH)
        return Promise.resolve(
          response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
        );
      throw new Error("unexpected request");
    });

    render(<GroupsWorkspace />);
    fireEvent.click(await screen.findByRole("button", { name: "View" }));
    await screen.findByText(ID);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Delete Group" }));

    expect(await screen.findByText(/Group deletion outcome is unknown/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Create" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByRole("button", { name: "Update" }).hasAttribute("disabled")).toBe(true);
    expect(screen.getByRole("button", { name: "Delete" }).hasAttribute("disabled")).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Create" }).hasAttribute("disabled")).toBe(false);
    });
    expect(fetchMock.mock.calls.filter(([target]) => target === GROUPS_DELETE_PATH)).toHaveLength(
      1,
    );
  });

  it("renders a reported Group deletion step-up rejection as a refusal", async () => {
    csrf();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUPS_LIST_PATH)
        return Promise.resolve(response({ items: [group()], next_cursor: null }));
      if (target === GROUPS_VIEW_PATH) return Promise.resolve(response(group()));
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(rejection("mfa_policy_denied", 403));
      return Promise.resolve(response({ items: [], next_cursor: null }));
    });

    render(<GroupsWorkspace />);
    fireEvent.click(await screen.findByRole("button", { name: "View" }));
    await screen.findByText(ID);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Delete Group" }));

    expect(await screen.findByText("The Group was not deleted.")).toBeTruthy();
    expect(screen.queryByText(/outcome is unknown/)).toBeNull();
    expect(fetchMock.mock.calls.filter(([target]) => target === GROUPS_DELETE_PATH)).toHaveLength(
      0,
    );
  });

  it("ends administration when Group deletion step-up reports grant-mutation access denial", async () => {
    csrf();
    const onAdministrationEnded = vi.fn();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUPS_LIST_PATH)
        return Promise.resolve(response({ items: [group()], next_cursor: null }));
      if (target === GROUPS_VIEW_PATH) return Promise.resolve(response(group()));
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(rejection("authorization_denied", 403));
      if (target === GROUP_MEMBERS_LIST_PATH || target === GROUP_GRANTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ADMINISTRATION_CATALOG_PATH)
        return Promise.resolve(
          response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
        );
      throw new Error("unexpected request");
    });

    render(<GroupsWorkspace onAdministrationEnded={onAdministrationEnded} />);
    fireEvent.click(await screen.findByRole("button", { name: "View" }));
    await screen.findByText(ID);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Delete Group" }));

    await waitFor(() => {
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText("The Group was not deleted.")).toBeNull();
    expect(screen.queryByText(/Group deletion outcome is unknown/)).toBeNull();
    expect(screen.queryByRole("heading", { name: "Verify deletion" })).toBeNull();
    expect(fetchMock.mock.calls.filter(([target]) => target === GROUPS_DELETE_PATH)).toHaveLength(
      0,
    );
  });

  it("ends administration when Group deletion step-up reports session-invalid", async () => {
    csrf();
    const onAdministrationEnded = vi.fn();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === GROUPS_LIST_PATH)
        return Promise.resolve(response({ items: [group()], next_cursor: null }));
      if (target === GROUPS_VIEW_PATH) return Promise.resolve(response(group()));
      if (target === MFA_POLICY_STEP_UP_PATH)
        return Promise.resolve(rejection("session_invalid", 401));
      if (target === GROUP_MEMBERS_LIST_PATH || target === GROUP_GRANTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ACCOUNTS_LIST_PATH)
        return Promise.resolve(response({ items: [], next_cursor: null }));
      if (target === ADMINISTRATION_CATALOG_PATH)
        return Promise.resolve(
          response({ client_modules: ["web-ui"], service_modules: [], operations: [] }),
        );
      throw new Error("unexpected request");
    });

    render(<GroupsWorkspace onAdministrationEnded={onAdministrationEnded} />);
    fireEvent.click(await screen.findByRole("button", { name: "View" }));
    await screen.findByText(ID);
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm" }));
    fireEvent.change(screen.getByLabelText("TOTP code"), { target: { value: CODE } });
    fireEvent.click(screen.getByRole("button", { name: "Delete Group" }));

    await waitFor(() => {
      expect(onAdministrationEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText("The Group was not deleted.")).toBeNull();
    expect(screen.queryByText(/Group deletion outcome is unknown/)).toBeNull();
    expect(screen.queryByRole("heading", { name: "Verify deletion" })).toBeNull();
    expect(fetchMock.mock.calls.filter(([target]) => target === GROUPS_DELETE_PATH)).toHaveLength(
      0,
    );
  });
});
