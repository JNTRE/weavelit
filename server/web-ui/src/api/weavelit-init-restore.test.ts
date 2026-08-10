import { describe, expect, it, vi } from "vitest";

import {
  ARTIFACT_MEDIA_TYPE,
  RESTORE_ARTIFACT_PATH,
  RESTORE_KEY_PATH,
  RESTORE_TICKET_HEADER,
  RestoreFailedError,
  UNREPORTED_FAILURE_CODE,
  isRestoreCompleted,
  parseIssuedTicket,
  parseStableErrorCode,
  submitRestore,
} from "./weavelit-init-restore";

const TICKET = "0123456789abcdefghijklmnopqrstuvwxyzABC-_";
const CORRELATION = "0123456789abcdef0123456789abcdef";
const RECOVERY_KEY = "AGE-SECRET-KEY-1EXAMPLEEXAMPLEEXAMPLEEXAMPLE";

function jsonResponse(body: unknown, status: number): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

function ticketResponse(ticket = TICKET): Response {
  return jsonResponse({ result: { restore_ticket: ticket }, correlation_id: CORRELATION }, 202);
}

function completionResponse(): Response {
  return jsonResponse({ result: { lifecycle: "initialized" }, correlation_id: CORRELATION }, 200);
}

function artifact(): File {
  return new File([new Uint8Array([0x57, 0x4c, 0x42, 0x4b])], "backup.wlitbackup");
}

/** Routes the key submission and the artifact upload independently. */
function mockRoutedFetch(routes: {
  key: () => Promise<Response>;
  upload: () => Promise<Response>;
}) {
  return vi
    .spyOn(globalThis, "fetch")
    .mockImplementation((input: unknown) =>
      input === RESTORE_ARTIFACT_PATH ? routes.upload() : routes.key(),
    );
}

describe("parseStableErrorCode", () => {
  it("returns a documented stable code", () => {
    expect(parseStableErrorCode({ error: "recovery_key_invalid" })).toBe("recovery_key_invalid");
  });

  it.each([
    ["null", null],
    ["an array", [{ error: "backup_invalid" }]],
    ["a string", "backup_invalid"],
    ["a missing error field", { correlation_id: CORRELATION }],
    ["a non-string error", { error: 400 }],
    ["an uppercase code", { error: "Backup_Invalid" }],
    ["a code carrying a message", { error: "backup invalid: /var/lib/weavelit" }],
    ["an over-long code", { error: "a".repeat(49) }],
    ["an empty code", { error: "" }],
  ])("discards %s", (_label, payload) => {
    expect(parseStableErrorCode(payload)).toBe(UNREPORTED_FAILURE_CODE);
  });
});

describe("parseIssuedTicket", () => {
  it("accepts the documented ticket envelope", () => {
    expect(
      parseIssuedTicket({ result: { restore_ticket: TICKET }, correlation_id: CORRELATION }),
    ).toBe(TICKET);
  });

  it("ignores additive fields permitted by the versioned contract", () => {
    expect(
      parseIssuedTicket({
        result: { restore_ticket: TICKET, future_field: 1 },
        correlation_id: CORRELATION,
        future_field: "ignored",
      }),
    ).toBe(TICKET);
  });

  it.each([
    ["null", null],
    ["an array", [{ result: { restore_ticket: TICKET } }]],
    ["a bare ticket", { restore_ticket: TICKET }],
    ["a missing result", { correlation_id: CORRELATION }],
    ["an array result", { result: [TICKET] }],
    ["a non-string ticket", { result: { restore_ticket: 1 } }],
    ["an empty ticket", { result: { restore_ticket: "" } }],
    ["an over-long ticket", { result: { restore_ticket: "a".repeat(49) } }],
    ["a ticket outside the token character set", { result: { restore_ticket: "abc def" } }],
  ])("rejects %s", (_label, payload) => {
    expect(parseIssuedTicket(payload)).toBeNull();
  });
});

describe("isRestoreCompleted", () => {
  it("accepts the documented completion envelope", () => {
    expect(
      isRestoreCompleted({ result: { lifecycle: "initialized" }, correlation_id: CORRELATION }),
    ).toBe(true);
  });

  it.each([
    ["null", null],
    ["a missing result", { correlation_id: CORRELATION }],
    ["another lifecycle", { result: { lifecycle: "uninitialized" } }],
    ["a bare lifecycle", { lifecycle: "initialized" }],
  ])("rejects %s", (_label, payload) => {
    expect(isRestoreCompleted(payload)).toBe(false);
  });
});

describe("submitRestore", () => {
  it("issues the two documented same-origin requests in order", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(completionResponse()),
    });
    const file = artifact();

    await expect(submitRestore(RECOVERY_KEY, file)).resolves.toBeUndefined();

    expect(fetchMock).toHaveBeenCalledTimes(2);
    const [keyTarget, keyInit] = fetchMock.mock.calls[0]!;
    expect(keyTarget).toBe("/api/v1/restore");
    expect(RESTORE_KEY_PATH).toBe("/api/v1/restore");
    expect(keyInit?.method).toBe("PUT");
    expect(keyInit?.headers).toEqual({
      Accept: "application/json",
      "Content-Type": "application/json",
      "X-Weavelit-CSRF": "1",
    });
    expect(keyInit?.body).toBe(`{"recovery_key":"${RECOVERY_KEY}"}`);
    expect(keyInit?.credentials).toBe("omit");
    expect(keyInit?.cache).toBe("no-store");
    expect(keyInit?.redirect).toBe("error");

    const [uploadTarget, uploadInit] = fetchMock.mock.calls[1]!;
    expect(uploadTarget).toBe("/api/v1/restore/artifact");
    expect(RESTORE_ARTIFACT_PATH).toBe("/api/v1/restore/artifact");
    expect(uploadInit?.method).toBe("PUT");
    expect(uploadInit?.headers).toEqual({
      Accept: "application/json",
      "Content-Type": ARTIFACT_MEDIA_TYPE,
      "X-Weavelit-CSRF": "1",
      "X-Weavelit-Restore-Ticket": TICKET,
    });
    expect(RESTORE_TICKET_HEADER).toBe("X-Weavelit-Restore-Ticket");
    expect(uploadInit?.credentials).toBe("omit");
    expect(uploadInit?.cache).toBe("no-store");
    expect(uploadInit?.redirect).toBe("error");
  });

  it("passes the selected File itself as the upload body without copying it", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(completionResponse()),
    });
    const file = artifact();
    const arrayBuffer = vi.spyOn(file, "arrayBuffer");
    const text = vi.spyOn(file, "text");

    await submitRestore(RECOVERY_KEY, file);

    // The identical File instance reaches fetch, so its bytes are streamed by
    // the browser rather than read into the JavaScript heap first.
    expect(fetchMock.mock.calls[1]![1]?.body).toBe(file);
    expect(arrayBuffer).not.toHaveBeenCalled();
    expect(text).not.toHaveBeenCalled();
  });

  it("never places the recovery key or the ticket in a URL", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(completionResponse()),
    });

    await submitRestore(RECOVERY_KEY, artifact());

    for (const [target] of fetchMock.mock.calls) {
      const url = target as string;
      expect(url).not.toContain(RECOVERY_KEY);
      expect(url).not.toContain(TICKET);
    }
  });

  it("never sets the browser-controlled Host or Origin headers", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(completionResponse()),
    });

    await submitRestore(RECOVERY_KEY, artifact());

    for (const [, init] of fetchMock.mock.calls) {
      const headers = init?.headers as Record<string, string>;
      const names = Object.keys(headers).map((name) => name.toLowerCase());
      expect(names).not.toContain("host");
      expect(names).not.toContain("origin");
      expect(headers["Content-Type"]).not.toContain("charset");
    }
  });

  it("does not upload the artifact when no ticket was issued", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(jsonResponse({ error: "restore_not_allowed" }, 409)),
      upload: () => Promise.resolve(completionResponse()),
    });

    await expect(submitRestore(RECOVERY_KEY, artifact())).rejects.toBeInstanceOf(
      RestoreFailedError,
    );

    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it.each([
    [400, "recovery_key_invalid"],
    [400, "backup_invalid"],
    [400, "backup_incompatible"],
    [403, "request_origin_denied"],
    [403, "restore_ticket_invalid"],
    [409, "restore_not_allowed"],
    [409, "restore_pending"],
    [500, "restore_failed"],
    [503, "service_unavailable"],
  ])("reports only the stable code of a %i rejection", async (status, code) => {
    mockRoutedFetch({
      key: () => Promise.resolve(ticketResponse()),
      upload: () => Promise.resolve(jsonResponse({ error: code }, status)),
    });

    const reason = await submitRestore(RECOVERY_KEY, artifact()).catch(
      (failure: unknown) => failure,
    );

    expect(reason).toBeInstanceOf(RestoreFailedError);
    expect((reason as RestoreFailedError).code).toBe(code);
    expect((reason as RestoreFailedError).message).toBe("restore_failed");
  });

  it.each([
    ["a transport failure", () => Promise.reject(new Error("ECONNREFUSED 127.0.0.1:8443"))],
    ["an unreadable body", () => Promise.resolve(new Response("<html>", { status: 502 }))],
    [
      "a detail-bearing body",
      () =>
        Promise.resolve(jsonResponse({ error: "backup_invalid: /var/lib/weavelit/secret" }, 400)),
    ],
    ["an undocumented success payload", () => Promise.resolve(jsonResponse({ result: {} }, 200))],
  ])("presents %s as the fixed unreported code", async (_label, upload) => {
    mockRoutedFetch({ key: () => Promise.resolve(ticketResponse()), upload });

    const reason = await submitRestore(RECOVERY_KEY, artifact()).catch(
      (failure: unknown) => failure,
    );

    expect(reason).toBeInstanceOf(RestoreFailedError);
    expect((reason as RestoreFailedError).code).toBe(UNREPORTED_FAILURE_CODE);
  });

  it("rejects a ticket response that is not the documented envelope", async () => {
    const fetchMock = mockRoutedFetch({
      key: () => Promise.resolve(jsonResponse({ result: { restore_ticket: "not a ticket" } }, 202)),
      upload: () => Promise.resolve(completionResponse()),
    });

    const reason = await submitRestore(RECOVERY_KEY, artifact()).catch(
      (failure: unknown) => failure,
    );

    expect((reason as RestoreFailedError).code).toBe(UNREPORTED_FAILURE_CODE);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
