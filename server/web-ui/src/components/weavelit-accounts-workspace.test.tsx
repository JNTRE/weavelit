import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME } from "../api/weavelit-authentication";
import { AUTH_SESSION_PATH } from "../api/weavelit-authentication";
import {
  ACCOUNTS_LIST_PATH,
  ACCOUNTS_STATUS_PATH,
  ACCOUNTS_VIEW_PATH,
} from "../api/weavelit-administration-accounts";
import {
  ACCOUNTS_CREATE_PATH,
  ACCOUNTS_RESET_PASSWORD_PATH,
  CREDENTIAL_ISSUANCE_STEP_UP_PATH,
} from "../api/weavelit-credential-issuance";
import {
  ACCOUNTS_MFA_REQUIREMENT_PATH,
  ACCOUNTS_MFA_RESET_PATH,
  MFA_POLICY_STEP_UP_PATH,
} from "../api/weavelit-mfa-policy";
import {
  AccountsWorkspace as AccountsWorkspaceComponent,
  type AccountsWorkspaceProps,
} from "./weavelit-accounts-workspace";

const CSRF = "csrf-token";
const ALICE_ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const BOB_ID = "QkJCQkJCQkJCQkJCQkJCQg";
const PASSWORD = "current-administrator-password";
const TOTP = "123456";
const TICKET = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const TEMPORARY_PASSWORD = "YWJjZGVmZ2hpamtsbW5vcHFy";
const CORRELATION = "credential-issuance-correlation";

function account(
  publicId: string,
  username: string,
  displayName: string | null,
  active: boolean,
  mfaRequired: boolean,
): Record<string, unknown> {
  return {
    public_id: publicId,
    username,
    display_name: displayName,
    active,
    mfa_required: mfaRequired,
  };
}

function response(result: unknown, status = 200): Response {
  return new Response(JSON.stringify(result), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function withCsrfCookie(onWrite: (value: string) => void = () => undefined): void {
  Object.defineProperty(globalThis.document, "cookie", {
    configurable: true,
    get: () => `${CSRF_COOKIE_NAME}=${CSRF}`,
    set: onWrite,
  });
}

function accountPage(items: readonly Record<string, unknown>[]): Response {
  return response({ result: { items, next_cursor: null }, correlation_id: CORRELATION });
}

function ticketResponse(): Response {
  return response({
    result: { credential_issuance_ticket: TICKET },
    correlation_id: CORRELATION,
  });
}

function mfaPolicyTicketResponse(): Response {
  return response({
    result: { totp_step_up_ticket: TICKET },
    correlation_id: CORRELATION,
  });
}

function credentialResponse(publicId: string): Response {
  return response({
    result: { public_id: publicId, temporary_password: TEMPORARY_PASSWORD },
    correlation_id: CORRELATION,
  });
}

function requestBody(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") {
    throw new Error("the request body must be a JSON string");
  }
  return JSON.parse(init.body) as Record<string, unknown>;
}

function AccountsWorkspace(props: Omit<AccountsWorkspaceProps, "currentAccountPublicId"> = {}) {
  return <AccountsWorkspaceComponent currentAccountPublicId={ALICE_ID} {...props} />;
}

afterEach(() => {
  Reflect.deleteProperty(globalThis.document, "cookie");
});

describe("AccountsWorkspace", () => {
  it("loads, pages, and views accounts through the public API", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
      if (target === ACCOUNTS_VIEW_PATH) {
        expect(requestBody(init)).toEqual({ public_id: BOB_ID });
        return Promise.resolve(
          response({
            result: account(BOB_ID, "bob", null, false, true),
            correlation_id: "view-correlation",
          }),
        );
      }
      expect(target).toBe(ACCOUNTS_LIST_PATH);
      const body = requestBody(init);
      if (body.cursor === undefined) {
        return Promise.resolve(
          response({
            result: {
              items: [account(ALICE_ID, "alice", "Alice", true, false)],
              next_cursor: "bmV4dC1wYWdl",
            },
            correlation_id: CORRELATION,
          }),
        );
      }
      expect(body.cursor).toBe("bmV4dC1wYWdl");
      return Promise.resolve(
        response({
          result: {
            items: [account(BOB_ID, "bob", null, false, true)],
            next_cursor: null,
          },
          correlation_id: CORRELATION,
        }),
      );
    });

    render(<AccountsWorkspace />);

    expect(await screen.findByRole("rowheader", { name: "alice" })).toBeTruthy();
    expect(screen.getByText("Alice")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    expect(await screen.findByRole("rowheader", { name: "bob" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Load more" })).toBeNull();

    const bobRow = screen.getByRole("rowheader", { name: "bob" }).closest("tr")!;
    fireEvent.click(bobRow.querySelector("button")!);
    expect(await screen.findByRole("heading", { name: "bob", level: 3 })).toBeTruthy();
    expect(screen.getByText(BOB_ID)).toBeTruthy();
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_LIST_PATH)).toHaveLength(
      2,
    );
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_VIEW_PATH)).toHaveLength(
      1,
    );
  });

  it("does not let a stale detail response overwrite a later selection", async () => {
    withCsrfCookie();
    let resolveAliceDetail!: (value: Response) => void;
    const aliceDetail = new Promise<Response>((resolve) => {
      resolveAliceDetail = resolve;
    });
    vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(
          accountPage([
            account(ALICE_ID, "alice", "Alice", true, false),
            account(BOB_ID, "bob", "Bob", false, true),
          ]),
        );
      }
      if (target === ACCOUNTS_VIEW_PATH) {
        const publicId = requestBody(init).public_id;
        if (publicId === ALICE_ID) {
          return aliceDetail;
        }
        if (publicId === BOB_ID) {
          return Promise.resolve(
            response({
              result: account(BOB_ID, "bob", "Bob", false, true),
              correlation_id: CORRELATION,
            }),
          );
        }
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    const aliceRow = (await screen.findByRole("rowheader", { name: "alice" })).closest("tr")!;
    const bobRow = screen.getByRole("rowheader", { name: "bob" }).closest("tr")!;
    fireEvent.click(aliceRow.querySelector<HTMLButtonElement>("button")!);
    fireEvent.click(bobRow.querySelector<HTMLButtonElement>("button")!);
    expect(await screen.findByRole("heading", { name: "bob", level: 3 })).toBeTruthy();

    await act(async () => {
      resolveAliceDetail(
        response({
          result: account(ALICE_ID, "alice", "Alice", true, false),
          correlation_id: CORRELATION,
        }),
      );
      await aliceDetail;
    });

    expect(screen.getByRole("heading", { name: "bob", level: 3 })).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "alice", level: 3 })).toBeNull();
  });

  it("ignores stale pagination after a status mutation reloads the collection", async () => {
    withCsrfCookie();
    let resolveStalePage!: (value: Response) => void;
    const stalePage = new Promise<Response>((resolve) => {
      resolveStalePage = resolve;
    });
    const seenCursors: unknown[] = [];
    let firstPageRequests = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
      if (target === ACCOUNTS_LIST_PATH) {
        const cursor = requestBody(init).cursor;
        if (cursor === "c3RhbGU") {
          seenCursors.push(cursor);
          return stalePage;
        }
        if (cursor === "ZnJlc2g") {
          seenCursors.push(cursor);
          return Promise.resolve(accountPage([]));
        }
        firstPageRequests += 1;
        return Promise.resolve(
          response({
            result: {
              items: [account(ALICE_ID, "alice", "Alice", firstPageRequests === 1, false)],
              next_cursor: firstPageRequests === 1 ? "c3RhbGU" : "ZnJlc2g",
            },
            correlation_id: CORRELATION,
          }),
        );
      }
      if (target === ACCOUNTS_STATUS_PATH) {
        return Promise.resolve(
          response({
            result: account(ALICE_ID, "alice", "Alice", false, false),
            correlation_id: CORRELATION,
          }),
        );
      }
      if (target === AUTH_SESSION_PATH) {
        return Promise.resolve(
          response({
            result: {
              account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
              public_id: BOB_ID,
              client_module: "web-ui",
              password_change_required: false,
            },
            correlation_id: CORRELATION,
          }),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    fireEvent.click(screen.getByRole("button", { name: "Disable alice" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm disable" }));

    await waitFor(() => {
      expect(firstPageRequests).toBe(2);
    });
    expect(screen.getByText("Disabled")).toBeTruthy();

    await act(async () => {
      resolveStalePage(
        response({
          result: {
            items: [account(BOB_ID, "stale-bob", "Stale Bob", true, false)],
            next_cursor: "c3RhbGUtdGFpbA",
          },
          correlation_id: CORRELATION,
        }),
      );
      await stalePage;
    });

    expect(screen.queryByRole("rowheader", { name: "stale-bob" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => {
      expect(seenCursors).toEqual(["c3RhbGU", "ZnJlc2g"]);
    });
  });

  it("renders one fixed failure without response or transport detail", async () => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({ error: "authorization_denied", detail: "internal secret" }, 403),
    );

    render(<AccountsWorkspace />);

    expect((await screen.findByRole("alert")).textContent).toContain("Accounts are unavailable.");
    expect(document.body.textContent).not.toContain("authorization_denied");
    expect(document.body.textContent).not.toContain("internal secret");
  });

  it("routes an exact expired-session account read to the shell", async () => {
    withCsrfCookie();
    const onSessionEnded = vi.fn();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
    );

    render(<AccountsWorkspace onSessionEnded={onSessionEnded} />);

    await waitFor(() => {
      expect(onSessionEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText("Accounts are unavailable.")).toBeNull();
  });

  it("rejects an additive sensitive projection without rendering its fields", async () => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      response({
        result: {
          items: [
            {
              ...account(ALICE_ID, "alice", "Alice", true, false),
              password_verifier: "secret-verifier",
              account_id: "internal-account",
            },
          ],
          next_cursor: null,
        },
      }),
    );

    render(<AccountsWorkspace />);
    expect((await screen.findByRole("alert")).textContent).toContain("Accounts are unavailable.");
    await waitFor(() => {
      expect(document.body.textContent).not.toContain("secret-verifier");
      expect(document.body.textContent).not.toContain("internal-account");
    });
  });

  it("confirms one exact disable, probes the session, refreshes, and offers detail re-enable", async () => {
    withCsrfCookie();
    let listRequests = 0;
    let statusRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
      if (target === ACCOUNTS_LIST_PATH) {
        listRequests += 1;
        return Promise.resolve(
          accountPage([
            account(ALICE_ID, "alice", "Alice", true, false),
            account(BOB_ID, "bob", "Bob", listRequests === 1, false),
          ]),
        );
      }
      if (target === ACCOUNTS_STATUS_PATH) {
        statusRequests += 1;
        expect(requestBody(init)).toEqual({ public_id: BOB_ID, active: false });
        return Promise.resolve(
          response({
            result: account(BOB_ID, "bob", "Bob", false, false),
            correlation_id: CORRELATION,
          }),
        );
      }
      if (target === AUTH_SESSION_PATH) {
        return Promise.resolve(
          response({
            result: {
              account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
              public_id: ALICE_ID,
              client_module: "web-ui",
              password_change_required: false,
            },
            correlation_id: CORRELATION,
          }),
        );
      }
      if (target === ACCOUNTS_VIEW_PATH) {
        return Promise.resolve(
          response({
            result: account(BOB_ID, "bob", "Bob", false, false),
            correlation_id: CORRELATION,
          }),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "bob" });
    expect(screen.getByRole("button", { name: "Disable bob" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Disable bob" }));
    expect(screen.getByRole("heading", { name: "Disable account" })).toBeTruthy();
    expect(screen.getByText(/This ends every session for the account/)).toBeTruthy();
    expect(statusRequests).toBe(0);

    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("heading", { name: "Disable account" })).toBeNull();
    expect(statusRequests).toBe(0);

    fireEvent.click(screen.getByRole("button", { name: "Disable bob" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm disable" }));
    await waitFor(() => {
      expect(listRequests).toBe(2);
    });
    expect(statusRequests).toBe(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === AUTH_SESSION_PATH)).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Re-enable bob" })).toBeTruthy();

    const bobRow = screen.getByRole("rowheader", { name: "bob" }).closest("tr")!;
    fireEvent.click(bobRow.querySelector<HTMLButtonElement>("button")!);
    expect(
      await screen.findByRole("button", { name: "Re-enable bob from account detail" }),
    ).toBeTruthy();
  });

  it("renders a reported status refusal without detail or automatic retry", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([account(ALICE_ID, "alice", "Alice", true, false)]));
      }
      if (target === ACCOUNTS_STATUS_PATH) {
        return Promise.resolve(
          response(
            {
              error: "authorization_denied",
              correlation_id: CORRELATION,
            },
            403,
          ),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("button", { name: "Disable alice" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm disable" }));

    expect(await screen.findByText("The account status was not changed.")).toBeTruthy();
    expect(document.body.textContent).not.toContain("authorization_denied");
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_STATUS_PATH)).toHaveLength(
      1,
    );
    expect(fetchMock.mock.calls.filter(([target]) => target === AUTH_SESSION_PATH)).toHaveLength(0);
  });

  it("renders one generic indeterminate outcome and never retries the mutation", async () => {
    withCsrfCookie();
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        listRequests += 1;
        if (listRequests === 2) {
          return Promise.reject(new Error("refresh failed before reconciliation"));
        }
        return Promise.resolve(accountPage([account(ALICE_ID, "alice", "Alice", true, false)]));
      }
      if (target === ACCOUNTS_STATUS_PATH) {
        return Promise.reject(new Error("response lost after request"));
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("button", { name: "Disable alice" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm disable" }));

    expect(await screen.findByText(/The account status outcome is unknown\./)).toBeTruthy();
    expect(document.body.textContent).not.toContain("response lost");
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_STATUS_PATH)).toHaveLength(
      1,
    );
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_LIST_PATH)).toHaveLength(
      1,
    );
    expect(screen.getByRole("button", { name: "Disable alice" })).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: "Refresh" })).toHaveProperty("disabled", false);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    const retry = await screen.findByRole("button", { name: "Retry" });
    expect(listRequests).toBe(2);
    expect(screen.getByLabelText("Username")).toHaveProperty("disabled", true);
    expect(retry).toHaveProperty("disabled", false);

    fireEvent.click(retry);
    await screen.findByRole("rowheader", { name: "alice" });
    expect(listRequests).toBe(3);
    expect(screen.getByRole("button", { name: "Disable alice" })).toHaveProperty("disabled", false);
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_STATUS_PATH)).toHaveLength(
      1,
    );
  });

  it("creates through one assurance ticket without persisting secrets and withdraws disclosure", async () => {
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    const storageWrite = vi.spyOn(Storage.prototype, "setItem");
    globalThis.sessionStorage.setItem("sanity-check", "safe-value");
    expect(storageWrite).toHaveBeenCalledWith("sanity-check", "safe-value");
    expect(globalThis.sessionStorage.length).toBe(1);
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    storageWrite.mockClear();

    const cookieWrite = vi.fn();
    withCsrfCookie(cookieWrite);
    const originalUrl = globalThis.location.href;
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
      if (target === ACCOUNTS_LIST_PATH) {
        listRequests += 1;
        return Promise.resolve(accountPage([account(ALICE_ID, "alice", "Alice", true, false)]));
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        expect(requestBody(init)).toEqual({ password: PASSWORD, totp_code: TOTP });
        return Promise.resolve(ticketResponse());
      }
      if (target === ACCOUNTS_CREATE_PATH) {
        expect(requestBody(init)).toEqual({
          username: "charlie",
          display_name: "Charlie",
          credential_issuance_ticket: TICKET,
        });
        return Promise.resolve(credentialResponse(BOB_ID));
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "charlie" } });
    fireEvent.change(screen.getByLabelText("Display name (optional)"), {
      target: { value: "Charlie" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.change(screen.getByLabelText("Authentication code (optional)"), {
      target: { value: TOTP },
    });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    const disclosure = await screen.findByRole("textbox", {
      name: "Temporary password",
    });
    expect(disclosure).toHaveProperty("value", TEMPORARY_PASSWORD);
    expect(screen.getAllByDisplayValue(TEMPORARY_PASSWORD)).toHaveLength(1);
    expect(document.body.innerHTML).not.toContain(TICKET);
    expect(document.body.innerHTML).not.toContain(PASSWORD);
    expect(globalThis.location.href).toBe(originalUrl);
    expect(cookieWrite).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(listRequests).toBe(2);
    });
    expect(storageWrite).not.toHaveBeenCalled();
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_CREATE_PATH)).toHaveLength(
      1,
    );

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(screen.queryByRole("textbox", { name: "Temporary password" })).toBeNull();
  });

  it("transfers a created credential disclosure when its refresh reports an expired session", async () => {
    localStorage.clear();
    sessionStorage.clear();
    const storageWrite = vi.spyOn(Storage.prototype, "setItem");
    const cookieWrite = vi.fn();
    withCsrfCookie(cookieWrite);
    const onSessionEnded = vi.fn();
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        listRequests += 1;
        return Promise.resolve(
          listRequests === 1
            ? accountPage([account(ALICE_ID, "alice", "Alice", true, false)])
            : response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
        );
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        return Promise.resolve(ticketResponse());
      }
      if (target === ACCOUNTS_CREATE_PATH) {
        return Promise.resolve(credentialResponse(BOB_ID));
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace onSessionEnded={onSessionEnded} />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "charlie" } });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    await waitFor(() => {
      expect(onSessionEnded).toHaveBeenCalledWith({
        publicId: BOB_ID,
        temporaryPassword: TEMPORARY_PASSWORD,
      });
    });
    expect(onSessionEnded).toHaveBeenCalledTimes(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_LIST_PATH)).toHaveLength(
      2,
    );
    expect(screen.getAllByDisplayValue(TEMPORARY_PASSWORD)).toHaveLength(1);
    expect(document.body.innerHTML).not.toContain(TICKET);
    expect(document.body.innerHTML).not.toContain(PASSWORD);
    expect(storageWrite).not.toHaveBeenCalled();
    expect(cookieWrite).not.toHaveBeenCalled();
    storageWrite.mockRestore();
  });

  it.each([
    {
      phase: "assurance ticket",
      action: "create",
      invalidPath: CREDENTIAL_ISSUANCE_STEP_UP_PATH,
      expectedCreateRequests: 0,
      expectedResetRequests: 0,
    },
    {
      phase: "create",
      action: "create",
      invalidPath: ACCOUNTS_CREATE_PATH,
      expectedCreateRequests: 1,
      expectedResetRequests: 0,
    },
    {
      phase: "reset",
      action: "reset",
      invalidPath: ACCOUNTS_RESET_PASSWORD_PATH,
      expectedCreateRequests: 0,
      expectedResetRequests: 1,
    },
  ])(
    "ends the session neutrally when the $phase phase reports canonical session invalidation",
    async ({ action, invalidPath, expectedCreateRequests, expectedResetRequests }) => {
      withCsrfCookie();
      const onSessionEnded = vi.fn();
      let listRequests = 0;
      const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
        if (target === ACCOUNTS_LIST_PATH) {
          listRequests += 1;
          return Promise.resolve(
            accountPage([
              account(ALICE_ID, "alice", "Alice", true, false),
              account(BOB_ID, "bob", "Bob", true, false),
            ]),
          );
        }
        if (target === invalidPath) {
          return Promise.resolve(
            response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
          );
        }
        if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
          return Promise.resolve(ticketResponse());
        }
        throw new Error("unexpected request");
      });

      render(<AccountsWorkspace onSessionEnded={onSessionEnded} />);
      await screen.findByRole("rowheader", { name: action === "create" ? "alice" : "bob" });
      if (action === "create") {
        fireEvent.change(screen.getByLabelText("Username"), { target: { value: "charlie" } });
        fireEvent.click(screen.getByRole("button", { name: "Create account" }));
      } else {
        fireEvent.click(screen.getByRole("button", { name: "Reset password for bob" }));
      }
      fireEvent.change(screen.getByLabelText("Current password"), {
        target: { value: PASSWORD },
      });
      fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

      await waitFor(() => {
        expect(onSessionEnded.mock.calls).toEqual([[]]);
      });
      expect(listRequests).toBe(1);
      expect(
        fetchMock.mock.calls.filter(([target]) => target === CREDENTIAL_ISSUANCE_STEP_UP_PATH),
      ).toHaveLength(1);
      expect(
        fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_CREATE_PATH),
      ).toHaveLength(expectedCreateRequests);
      expect(
        fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_RESET_PASSWORD_PATH),
      ).toHaveLength(expectedResetRequests);
      expect(fetchMock.mock.calls.filter(([target]) => target === AUTH_SESSION_PATH)).toHaveLength(
        0,
      );
      expect(screen.queryByRole("textbox", { name: "Temporary password" })).toBeNull();
      expect(screen.queryByText("Credential issuance was not completed.")).toBeNull();
      expect(screen.queryByText(/The credential issuance outcome is unknown\./)).toBeNull();
    },
  );

  it("does not retry an assurance request whose outcome is unknown", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([]));
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        return Promise.reject(new Error("connection closed after request"));
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByText("0 loaded");
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "charlie" } });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    expect(await screen.findByText(/The credential issuance outcome is unknown\./)).toBeTruthy();
    expect(screen.getByLabelText("Current password")).toHaveProperty("value", "");
    expect(
      fetchMock.mock.calls.filter(([target]) => target === CREDENTIAL_ISSUANCE_STEP_UP_PATH),
    ).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_CREATE_PATH)).toHaveLength(
      0,
    );
  });

  it("does not retry an indeterminate consuming request or disclose its ticket", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([]));
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        return Promise.resolve(ticketResponse());
      }
      if (target === ACCOUNTS_CREATE_PATH) {
        return Promise.reject(new Error("response was lost"));
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByText("0 loaded");
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "charlie" } });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    expect(await screen.findByText(/The credential issuance outcome is unknown\./)).toBeTruthy();
    expect(screen.queryByRole("heading", { name: "Credential assurance" })).toBeNull();
    expect(document.body.innerHTML).not.toContain(TICKET);
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_CREATE_PATH)).toHaveLength(
      1,
    );
    expect(
      fetchMock.mock.calls.filter(([target]) => target === CREDENTIAL_ISSUANCE_STEP_UP_PATH),
    ).toHaveLength(1);
  });

  it("renders a reported refusal without its reason or response detail", async () => {
    withCsrfCookie();
    vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([]));
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        return Promise.resolve(ticketResponse());
      }
      if (target === ACCOUNTS_CREATE_PATH) {
        return Promise.resolve(
          response(
            { error: "conflict", correlation_id: CORRELATION, detail: "private conflict cause" },
            409,
          ),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByText("0 loaded");
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "charlie" } });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      "Credential issuance was not completed.",
    );
    expect(document.body.textContent).not.toContain("conflict");
    expect(document.body.textContent).not.toContain("private conflict cause");
  });

  it("probes the authenticated session after another-account reset and refreshes once", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(
          accountPage([
            account(ALICE_ID, "alice", "Alice", true, false),
            account(BOB_ID, "bob", "Bob", true, false),
          ]),
        );
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        return Promise.resolve(ticketResponse());
      }
      if (target === ACCOUNTS_RESET_PASSWORD_PATH) {
        expect(requestBody(init)).toEqual({
          public_id: BOB_ID,
          credential_issuance_ticket: TICKET,
        });
        return Promise.resolve(credentialResponse(BOB_ID));
      }
      if (target === AUTH_SESSION_PATH) {
        return Promise.resolve(
          response({
            result: {
              account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
              public_id: ALICE_ID,
              client_module: "web-ui",
              password_change_required: false,
            },
            correlation_id: CORRELATION,
          }),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "bob" });
    fireEvent.click(screen.getByRole("button", { name: "Reset password for bob" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    expect(
      await screen.findByRole("textbox", {
        name: "Temporary password",
      }),
    ).toHaveProperty("value", TEMPORARY_PASSWORD);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_RESET_PASSWORD_PATH),
    ).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_CREATE_PATH)).toHaveLength(
      0,
    );
    await waitFor(() => {
      expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_LIST_PATH)).toHaveLength(
        2,
      );
    });
    expect(screen.getAllByDisplayValue(TEMPORARY_PASSWORD)).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === AUTH_SESSION_PATH)).toHaveLength(1);
  });

  it("probes despite a stale cached identity and transfers disclosure without retrying", async () => {
    localStorage.clear();
    sessionStorage.clear();
    const storageWrite = vi.spyOn(Storage.prototype, "setItem");
    const cookieWrite = vi.fn();
    withCsrfCookie(cookieWrite);
    const originalUrl = location.href;
    const onSessionEnded = vi.fn();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(
          accountPage([
            account(ALICE_ID, "alice", "Alice", true, false),
            account(BOB_ID, "bob", "Bob", true, false),
          ]),
        );
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        return Promise.resolve(ticketResponse());
      }
      if (target === ACCOUNTS_RESET_PASSWORD_PATH) {
        return Promise.resolve(credentialResponse(BOB_ID));
      }
      if (target === AUTH_SESSION_PATH) {
        return Promise.resolve(
          response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace onSessionEnded={onSessionEnded} />);
    await screen.findByRole("rowheader", { name: "bob" });
    fireEvent.click(screen.getByRole("button", { name: "Reset password for bob" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    await waitFor(() => {
      expect(onSessionEnded).toHaveBeenCalledWith({
        publicId: BOB_ID,
        temporaryPassword: TEMPORARY_PASSWORD,
      });
    });
    expect(screen.getAllByDisplayValue(TEMPORARY_PASSWORD)).toHaveLength(1);
    expect(document.body.innerHTML).not.toContain(TICKET);
    expect(document.body.innerHTML).not.toContain(PASSWORD);
    expect(location.href).toBe(originalUrl);
    expect(storageWrite).not.toHaveBeenCalled();
    expect(cookieWrite).not.toHaveBeenCalled();
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_RESET_PASSWORD_PATH),
    ).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === AUTH_SESSION_PATH)).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_LIST_PATH)).toHaveLength(
      1,
    );
    storageWrite.mockRestore();
  });

  it("preserves self-reset disclosure and only rechecks an indeterminate session manually", async () => {
    withCsrfCookie();
    const onSessionEnded = vi.fn();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([account(ALICE_ID, "alice", "Alice", true, false)]));
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        return Promise.resolve(ticketResponse());
      }
      if (target === ACCOUNTS_RESET_PASSWORD_PATH) {
        return Promise.resolve(credentialResponse(ALICE_ID));
      }
      if (target === AUTH_SESSION_PATH) {
        return Promise.resolve(new Response("unreadable", { status: 200 }));
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace onSessionEnded={onSessionEnded} />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("button", { name: "Reset password for alice" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    expect(
      await screen.findByText(/The current session state could not be determined\./),
    ).toBeTruthy();
    expect(screen.getAllByDisplayValue(TEMPORARY_PASSWORD)).toHaveLength(1);
    expect(screen.getByRole("button", { name: "Reset password for alice" })).toHaveProperty(
      "disabled",
      true,
    );
    expect(onSessionEnded).not.toHaveBeenCalled();
    expect(fetchMock.mock.calls.filter(([target]) => target === AUTH_SESSION_PATH)).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Check session again" }));
    await waitFor(() => {
      expect(fetchMock.mock.calls.filter(([target]) => target === AUTH_SESSION_PATH)).toHaveLength(
        2,
      );
    });
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_RESET_PASSWORD_PATH),
    ).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_LIST_PATH)).toHaveLength(
      1,
    );
    expect(screen.getAllByDisplayValue(TEMPORARY_PASSWORD)).toHaveLength(1);
  });

  it("locks a failed paging Retry while reset session reconciliation is indeterminate", async () => {
    localStorage.clear();
    sessionStorage.clear();
    const storageWrite = vi.spyOn(Storage.prototype, "setItem");
    const cookieWrite = vi.fn();
    withCsrfCookie(cookieWrite);
    let rejectLoadMore!: (reason?: unknown) => void;
    const pendingLoadMore = new Promise<Response>((_resolve, reject) => {
      rejectLoadMore = reject;
    });
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        listRequests += 1;
        if (listRequests === 1) {
          return Promise.resolve(
            response({
              result: {
                items: [account(ALICE_ID, "alice", "Alice", true, false)],
                next_cursor: "bmV4dC1wYWdl",
              },
              correlation_id: CORRELATION,
            }),
          );
        }
        return pendingLoadMore;
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        return Promise.resolve(ticketResponse());
      }
      if (target === ACCOUNTS_RESET_PASSWORD_PATH) {
        return Promise.resolve(credentialResponse(ALICE_ID));
      }
      if (target === AUTH_SESSION_PATH) {
        return Promise.resolve(new Response("unreadable", { status: 200 }));
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => {
      expect(listRequests).toBe(2);
    });
    fireEvent.click(screen.getByRole("button", { name: "Reset password for alice" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));

    expect(
      await screen.findByText(/The current session state could not be determined\./),
    ).toBeTruthy();
    await act(async () => {
      rejectLoadMore(new Error("paging response lost"));
      await pendingLoadMore.catch(() => undefined);
    });

    const retry = await screen.findByRole("button", { name: "Retry" });
    expect(retry).toHaveProperty("disabled", true);
    expect(screen.getByRole("button", { name: "Check session again" })).toHaveProperty(
      "disabled",
      false,
    );
    retry.removeAttribute("disabled");
    fireEvent.click(retry);

    expect(screen.getAllByDisplayValue(TEMPORARY_PASSWORD)).toHaveLength(1);
    expect(document.body.innerHTML).not.toContain(TICKET);
    expect(document.body.innerHTML).not.toContain(PASSWORD);
    expect(storageWrite).not.toHaveBeenCalled();
    expect(cookieWrite).not.toHaveBeenCalled();
    expect(listRequests).toBe(2);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_RESET_PASSWORD_PATH),
    ).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === AUTH_SESSION_PATH)).toHaveLength(1);
    storageWrite.mockRestore();
  });

  it("does not consume a ticket returned after the workspace unmounts", async () => {
    withCsrfCookie();
    let resolveTicket!: (value: Response) => void;
    const pendingTicket = new Promise<Response>((resolve) => {
      resolveTicket = resolve;
    });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([]));
      }
      if (target === CREDENTIAL_ISSUANCE_STEP_UP_PATH) {
        return pendingTicket;
      }
      throw new Error("unexpected request");
    });

    const rendered = render(<AccountsWorkspace />);
    await screen.findByText("0 loaded");
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "charlie" } });
    fireEvent.click(screen.getByRole("button", { name: "Create account" }));
    fireEvent.change(screen.getByLabelText("Current password"), { target: { value: PASSWORD } });
    fireEvent.click(screen.getByRole("button", { name: "Confirm credential issuance" }));
    rendered.unmount();

    await act(async () => {
      resolveTicket(ticketResponse());
      await pendingTicket;
      await Promise.resolve();
    });

    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_CREATE_PATH)).toHaveLength(
      0,
    );
    expect(document.body.innerHTML).not.toContain(TICKET);
  });

  it("confirms and applies one MFA requirement with a code-only step-up and private ticket", async () => {
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    const storageWrite = vi.spyOn(Storage.prototype, "setItem");
    globalThis.sessionStorage.setItem("sanity-check", "safe-value");
    expect(storageWrite).toHaveBeenCalledWith("sanity-check", "safe-value");
    expect(globalThis.sessionStorage.length).toBe(1);
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    storageWrite.mockClear();

    const cookieWrite = vi.fn();
    withCsrfCookie(cookieWrite);
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
      if (target === ACCOUNTS_LIST_PATH) {
        listRequests += 1;
        return Promise.resolve(
          accountPage([account(ALICE_ID, "alice", "Alice", true, listRequests > 1)]),
        );
      }
      if (target === MFA_POLICY_STEP_UP_PATH) {
        expect(requestBody(init)).toEqual({ family: "mfa_policy", code: TOTP });
        return Promise.resolve(mfaPolicyTicketResponse());
      }
      if (target === ACCOUNTS_MFA_REQUIREMENT_PATH) {
        expect(requestBody(init)).toEqual({
          public_id: ALICE_ID,
          required: true,
          totp_step_up_ticket: TICKET,
        });
        expect(requestBody(init)).not.toHaveProperty("confirmed");
        return Promise.resolve(
          response({
            result: account(ALICE_ID, "alice", "Alice", true, true),
            correlation_id: CORRELATION,
          }),
        );
      }
      if (target === AUTH_SESSION_PATH) {
        return Promise.resolve(
          response({
            result: {
              account_id: "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
              public_id: BOB_ID,
              client_module: "web-ui",
              password_change_required: false,
            },
            correlation_id: CORRELATION,
          }),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("switch", { name: "Require MFA for alice" }));
    expect(screen.getByRole("heading", { name: "Require MFA" })).toBeTruthy();
    expect(
      fetchMock.mock.calls.filter(([target]) => target === MFA_POLICY_STEP_UP_PATH),
    ).toHaveLength(0);
    fireEvent.click(screen.getByRole("button", { name: "Confirm policy action" }));
    expect(screen.queryByLabelText("Current password")).toBeNull();
    const code = screen.getByLabelText("Authentication code");
    fireEvent.change(code, { target: { value: TOTP } });
    fireEvent.click(screen.getByRole("button", { name: "Verify and apply" }));

    await waitFor(() => {
      expect(listRequests).toBe(2);
    });
    expect(screen.getByRole("switch", { name: "Require MFA for alice" })).toHaveProperty(
      "checked",
      true,
    );
    expect(
      fetchMock.mock.calls.filter(([target]) => target === MFA_POLICY_STEP_UP_PATH),
    ).toHaveLength(1);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_MFA_REQUIREMENT_PATH),
    ).toHaveLength(1);
    expect(document.body.innerHTML).not.toContain(TICKET);
    expect(document.body.innerHTML).not.toContain(TOTP);
    expect(storageWrite).not.toHaveBeenCalled();
    expect(cookieWrite).not.toHaveBeenCalled();
  });

  it("resets MFA for the exact account and returns to sign-in after self-session revocation", async () => {
    withCsrfCookie();
    const onSessionEnded = vi.fn();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target, init) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([account(ALICE_ID, "alice", "Alice", true, true)]));
      }
      if (target === MFA_POLICY_STEP_UP_PATH) {
        return Promise.resolve(mfaPolicyTicketResponse());
      }
      if (target === ACCOUNTS_MFA_RESET_PATH) {
        expect(requestBody(init)).toEqual({
          public_id: ALICE_ID,
          totp_step_up_ticket: TICKET,
        });
        return Promise.resolve(
          response({
            result: account(ALICE_ID, "alice", "Alice", true, true),
            correlation_id: CORRELATION,
          }),
        );
      }
      if (target === AUTH_SESSION_PATH) {
        return Promise.resolve(
          response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace onSessionEnded={onSessionEnded} />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("button", { name: "Reset MFA for alice" }));
    expect(
      screen.getByText(/This removes the enrolled factor and ends every session/),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Confirm policy action" }));
    fireEvent.change(screen.getByLabelText("Authentication code"), { target: { value: TOTP } });
    fireEvent.click(screen.getByRole("button", { name: "Verify and apply" }));

    await waitFor(() => {
      expect(onSessionEnded).toHaveBeenCalledTimes(1);
    });
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_MFA_RESET_PATH),
    ).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === AUTH_SESSION_PATH)).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_LIST_PATH)).toHaveLength(
      1,
    );
    expect(document.body.innerHTML).not.toContain(TICKET);
  });

  it("does not retry or mutate after an indeterminate MFA step-up", async () => {
    withCsrfCookie();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([account(ALICE_ID, "alice", "Alice", true, false)]));
      }
      if (target === MFA_POLICY_STEP_UP_PATH) {
        return Promise.reject(new Error("response lost after code submission"));
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("switch", { name: "Require MFA for alice" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm policy action" }));
    fireEvent.change(screen.getByLabelText("Authentication code"), { target: { value: TOTP } });
    fireEvent.click(screen.getByRole("button", { name: "Verify and apply" }));

    expect(await screen.findByText(/The MFA policy outcome is unknown\./)).toBeTruthy();
    expect(document.body.textContent).not.toContain("response lost");
    expect(
      fetchMock.mock.calls.filter(([target]) => target === MFA_POLICY_STEP_UP_PATH),
    ).toHaveLength(1);
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_MFA_REQUIREMENT_PATH),
    ).toHaveLength(0);
  });

  it("returns an expired MFA step-up session to the shell once without an outcome claim", async () => {
    withCsrfCookie();
    const onSessionEnded = vi.fn();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([account(ALICE_ID, "alice", "Alice", true, false)]));
      }
      if (target === MFA_POLICY_STEP_UP_PATH) {
        return Promise.resolve(
          response({ error: "session_invalid", correlation_id: CORRELATION }, 401),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace onSessionEnded={onSessionEnded} />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("switch", { name: "Require MFA for alice" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm policy action" }));
    fireEvent.change(screen.getByLabelText("Authentication code"), { target: { value: TOTP } });
    fireEvent.click(screen.getByRole("button", { name: "Verify and apply" }));

    await waitFor(() => {
      expect(onSessionEnded).toHaveBeenCalledTimes(1);
    });
    expect(screen.queryByText("MFA policy was not changed.")).toBeNull();
    expect(screen.queryByText(/The MFA policy outcome is unknown\./)).toBeNull();
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_MFA_REQUIREMENT_PATH),
    ).toHaveLength(0);
  });

  it("keeps a normal MFA step-up refusal in Accounts without attempting the mutation", async () => {
    withCsrfCookie();
    const onSessionEnded = vi.fn();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        return Promise.resolve(accountPage([account(ALICE_ID, "alice", "Alice", true, false)]));
      }
      if (target === MFA_POLICY_STEP_UP_PATH) {
        return Promise.resolve(
          response({ error: "mfa_policy_denied", correlation_id: CORRELATION }, 403),
        );
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace onSessionEnded={onSessionEnded} />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("switch", { name: "Require MFA for alice" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm policy action" }));
    fireEvent.change(screen.getByLabelText("Authentication code"), { target: { value: TOTP } });
    fireEvent.click(screen.getByRole("button", { name: "Verify and apply" }));

    expect(await screen.findByText("MFA policy was not changed.")).toBeTruthy();
    expect(onSessionEnded).not.toHaveBeenCalled();
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_MFA_REQUIREMENT_PATH),
    ).toHaveLength(0);
  });

  it("does not retry or refresh after an indeterminate MFA mutation", async () => {
    withCsrfCookie();
    let listRequests = 0;
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === ACCOUNTS_LIST_PATH) {
        listRequests += 1;
        return Promise.resolve(accountPage([account(ALICE_ID, "alice", "Alice", true, false)]));
      }
      if (target === MFA_POLICY_STEP_UP_PATH) {
        return Promise.resolve(mfaPolicyTicketResponse());
      }
      if (target === ACCOUNTS_MFA_REQUIREMENT_PATH) {
        return Promise.reject(new Error("mutation response lost"));
      }
      throw new Error("unexpected request");
    });

    render(<AccountsWorkspace />);
    await screen.findByRole("rowheader", { name: "alice" });
    fireEvent.click(screen.getByRole("switch", { name: "Require MFA for alice" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm policy action" }));
    fireEvent.change(screen.getByLabelText("Authentication code"), { target: { value: TOTP } });
    fireEvent.click(screen.getByRole("button", { name: "Verify and apply" }));

    expect(await screen.findByText(/The MFA policy outcome is unknown\./)).toBeTruthy();
    expect(document.body.textContent).not.toContain("mutation response lost");
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_MFA_REQUIREMENT_PATH),
    ).toHaveLength(1);
    expect(fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_LIST_PATH)).toHaveLength(
      1,
    );
    expect(document.body.innerHTML).not.toContain(TICKET);
    expect(screen.getByRole("button", { name: "Reset MFA for alice" })).toHaveProperty(
      "disabled",
      true,
    );
    expect(screen.getByRole("button", { name: "Refresh" })).toHaveProperty("disabled", false);

    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => {
      expect(listRequests).toBe(2);
    });
    expect(screen.getByRole("button", { name: "Reset MFA for alice" })).toHaveProperty(
      "disabled",
      false,
    );
    expect(
      fetchMock.mock.calls.filter(([target]) => target === ACCOUNTS_MFA_REQUIREMENT_PATH),
    ).toHaveLength(1);
  });
});
