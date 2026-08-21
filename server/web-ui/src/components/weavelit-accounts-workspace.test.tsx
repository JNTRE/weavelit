import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME } from "../api/weavelit-authentication";
import { ACCOUNTS_LIST_PATH, ACCOUNTS_VIEW_PATH } from "../api/weavelit-administration-accounts";
import { AccountsWorkspace } from "./weavelit-accounts-workspace";

const CSRF = "csrf-token";
const ALICE_ID = "QUFBQUFBQUFBQUFBQUFBQQ";
const BOB_ID = "QkJCQkJCQkJCQkJCQkJCQg";

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

function withCsrfCookie(): void {
  Object.defineProperty(globalThis.document, "cookie", {
    configurable: true,
    get: () => `${CSRF_COOKIE_NAME}=${CSRF}`,
  });
}

function requestBody(init: RequestInit | undefined): Record<string, unknown> {
  if (typeof init?.body !== "string") {
    throw new Error("the request body must be a JSON string");
  }
  return JSON.parse(init.body) as Record<string, unknown>;
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

  it("never renders additive sensitive fields from an otherwise valid projection", async () => {
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
    expect(await screen.findByRole("rowheader", { name: "alice" })).toBeTruthy();
    await waitFor(() => {
      expect(document.body.textContent).not.toContain("secret-verifier");
      expect(document.body.textContent).not.toContain("internal-account");
    });
  });
});
