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

interface AccountFixture {
  public_id: string;
  username: string;
  display_name: string;
  active: boolean;
  mfa_required: boolean;
}

interface GrantFixture {
  type: string;
  value?: string;
}

type PaginationCase =
  | {
      readonly collection: "members";
      readonly buttonLabel: "Load more members";
      readonly listPath: string;
      readonly cursor: string;
      readonly pageItem: AccountFixture;
      readonly renderedText: string;
      readonly expectedRequestBody: Record<string, unknown>;
      readonly settledNextCursor: string;
    }
  | {
      readonly collection: "grants";
      readonly buttonLabel: "Load more grants";
      readonly listPath: string;
      readonly cursor: string;
      readonly pageItem: GrantFixture;
      readonly renderedText: string;
      readonly expectedRequestBody: Record<string, unknown>;
      readonly settledNextCursor: string;
    }
  | {
      readonly collection: "accounts";
      readonly buttonLabel: "Load more Accounts";
      readonly listPath: string;
      readonly cursor: string;
      readonly pageItem: AccountFixture;
      readonly renderedText: string;
      readonly expectedRequestBody: Record<string, unknown>;
      readonly settledNextCursor: string;
    };

const paginationCases = [
  {
    collection: "members",
    buttonLabel: "Load more members",
    listPath: GROUP_MEMBERS_LIST_PATH,
    cursor: "members-cursor",
    pageItem: {
      public_id: "CCCCCCCCCCCCCCCCCCCCCg",
      username: "member-user",
      display_name: "Member User",
      active: true,
      mfa_required: false,
    },
    renderedText: "member-user",
    expectedRequestBody: { group_public_id: GROUP_ID, cursor: "members-cursor" },
    settledNextCursor: "members-follow-on",
  },
  {
    collection: "grants",
    buttonLabel: "Load more grants",
    listPath: GROUP_GRANTS_LIST_PATH,
    cursor: "grants-cursor",
    pageItem: {
      type: "operation",
      value: "REdHREdHREdHREdHREdHRg",
    },
    renderedText: "Operation: REdHREdHREdHREdHREdHRg",
    expectedRequestBody: { group_public_id: GROUP_ID, cursor: "grants-cursor" },
    settledNextCursor: "grants-follow-on",
  },
  {
    collection: "accounts",
    buttonLabel: "Load more Accounts",
    listPath: ACCOUNTS_LIST_PATH,
    cursor: "accounts-cursor",
    pageItem: {
      public_id: "RElSRElSRElSRElSRElSRg",
      username: "account-user",
      display_name: "Account User",
      active: true,
      mfa_required: false,
    },
    renderedText: "account-user",
    expectedRequestBody: { cursor: "accounts-cursor" },
    settledNextCursor: "accounts-follow-on",
  },
] satisfies readonly PaginationCase[];

function account(): AccountFixture {
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

function safeRequestLabel(target: unknown): string {
  return typeof target === "string" ? target : String(target);
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
      return Promise.reject(new Error(`unexpected request: ${safeRequestLabel(target)}`));
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
    "ends administration when a %s step-up reports grant-mutation access denial",
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
            response({ error: "authorization_denied", correlation_id: CORRELATION }, 403),
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

  it.each([
    ["member", GROUP_MEMBERS_CHANGE_PATH, "session_invalid", 401],
    ["grant", GROUP_GRANTS_CHANGE_PATH, "authorization_denied", 403],
  ] as const)(
    "ends administration once when a %s change reports a terminal mutation outcome",
    async (kind, mutationPath, error, status) => {
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
          return Promise.resolve(response({ totp_step_up_ticket: TICKET }));
        if (target === mutationPath)
          return Promise.resolve(response({ error, correlation_id: CORRELATION }, status));
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
      expect(fetchMock.mock.calls.filter(([target]) => target === mutationPath)).toHaveLength(1);
      expect(
        fetchMock.mock.calls.filter(([target]) => target === MFA_POLICY_STEP_UP_PATH),
      ).toHaveLength(1);
      expect(
        fetchMock.mock.calls.filter(([target]) => target === GROUP_MEMBERS_LIST_PATH),
      ).toHaveLength(1);
      expect(
        fetchMock.mock.calls.filter(([target]) => target === GROUP_GRANTS_LIST_PATH),
      ).toHaveLength(1);
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

  it.each(paginationCases)(
    "admits one request per $collection pagination cursor despite double click and disables during flight",
    async (testCase) => {
      csrf();

      // Deferred response resolver for cursor-bearing pagination request
      let resolvePaginationResponse: (response: Response) => void = () => {
        throw new Error("resolver was not assigned");
      };
      const deferredPaginationPromise = new Promise<Response>((resolve) => {
        resolvePaginationResponse = resolve;
      });

      const initialMembers: AccountFixture[] = [];
      const initialGrants: GrantFixture[] = [];
      const initialAccounts: AccountFixture[] = [
        {
          public_id: ACCOUNT_ID,
          username: "initial-account",
          display_name: "Initial Account",
          active: true,
          mfa_required: false,
        },
      ];

      const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
        if (target === testCase.listPath && init?.method !== "POST") {
          try {
            const req = body(init);
            // Cursor-bearing pagination request stays deferred
            if ("cursor" in req) {
              return deferredPaginationPromise;
            }
          } catch {
            // Initial request without body, resolve immediately
          }
          if (testCase.collection === "members") {
            return Promise.resolve(
              response({
                items: initialMembers,
                next_cursor: "members-cursor",
              }),
            );
          }
          if (testCase.collection === "grants") {
            return Promise.resolve(
              response({
                items: initialGrants,
                next_cursor: "grants-cursor",
              }),
            );
          }
          return Promise.resolve(
            response({
              items: initialAccounts,
              next_cursor: "accounts-cursor",
            }),
          );
        }
        if (target === GROUP_MEMBERS_LIST_PATH)
          return Promise.resolve(
            response({
              items: initialMembers,
              next_cursor: testCase.collection === "members" ? "members-cursor" : null,
            }),
          );
        if (target === GROUP_GRANTS_LIST_PATH)
          return Promise.resolve(
            response({
              items: initialGrants,
              next_cursor: testCase.collection === "grants" ? "grants-cursor" : null,
            }),
          );
        if (target === ACCOUNTS_LIST_PATH)
          return Promise.resolve(
            response({
              items: initialAccounts,
              next_cursor: testCase.collection === "accounts" ? "accounts-cursor" : null,
            }),
          );
        if (target === ADMINISTRATION_CATALOG_PATH)
          return Promise.resolve(
            response({ client_modules: [], service_modules: [], operations: ["operation"] }),
          );
        return Promise.reject(new Error(`unexpected request: ${safeRequestLabel(target)}`));
      });

      render(<GroupAssociations groupPublicId={GROUP_ID} />);

      // Wait for initial load and find the pagination button
      const paginationButton = await screen.findByRole("button", {
        name: testCase.buttonLabel,
      });

      // Verify button is not disabled before clicking
      expect(paginationButton.hasAttribute("disabled")).toBe(false);

      // Double-click the pagination button rapidly
      fireEvent.click(paginationButton);
      fireEvent.click(paginationButton);

      // After first click, button should be disabled
      await waitFor(() => {
        expect(paginationButton.hasAttribute("disabled")).toBe(true);
      });

      // Verify exactly one pagination request with cursor was made
      const listRequests = fetchMock.mock.calls.filter(([target]) => target === testCase.listPath);
      const paginationRequests = listRequests.filter((call) => {
        try {
          const req = body(call[1]);
          return "cursor" in req;
        } catch {
          return false;
        }
      });
      expect(paginationRequests).toHaveLength(1);
      const paginationRequest = paginationRequests[0];
      if (paginationRequest === undefined) throw new Error("pagination request was not captured");
      expect(body(paginationRequest[1])).toEqual(testCase.expectedRequestBody);

      // Resolve the deferred pagination response
      resolvePaginationResponse(
        response({
          items: [testCase.pageItem],
          next_cursor: testCase.settledNextCursor,
        }),
      );

      // Wait for button to be re-enabled after response settles
      await waitFor(() => {
        const requeriedButton = screen.getByRole("button", {
          name: testCase.buttonLabel,
        });
        expect(requeriedButton.hasAttribute("disabled")).toBe(false);
      });

      // Verify exactly one new item with exact rendered text
      const newItems = screen.getAllByText(testCase.renderedText, { exact: true });
      expect(newItems.length).toBe(1);

      fetchMock.mockRestore();
    },
  );
});
