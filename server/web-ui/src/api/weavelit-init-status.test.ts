import { describe, expect, it, vi } from "vitest";

import {
  STATUS_PATH,
  fetchPreOperationalStatus,
  parsePreOperationalStatus,
} from "./weavelit-init-status";

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

describe("parsePreOperationalStatus", () => {
  it("accepts both documented projections", () => {
    expect(
      parsePreOperationalStatus({ lifecycle: "uninitialized", database_selected: true }),
    ).toEqual({ lifecycle: "uninitialized", databaseSelected: true });
    expect(
      parsePreOperationalStatus({ lifecycle: "uninitialized", database_selected: false }),
    ).toEqual({ lifecycle: "uninitialized", databaseSelected: false });
  });

  it("ignores additive fields permitted by the versioned contract", () => {
    expect(
      parsePreOperationalStatus({
        lifecycle: "uninitialized",
        database_selected: false,
        future_field: "ignored",
      }),
    ).toEqual({ lifecycle: "uninitialized", databaseSelected: false });
  });

  it.each([
    ["null", null],
    ["an array", [{ lifecycle: "uninitialized", database_selected: true }]],
    ["a string", "uninitialized"],
    ["a missing lifecycle", { database_selected: true }],
    ["an unexpected lifecycle", { lifecycle: "operational", database_selected: true }],
    ["a missing selection flag", { lifecycle: "uninitialized" }],
    ["a non-boolean selection flag", { lifecycle: "uninitialized", database_selected: "true" }],
  ])("rejects %s", (_label, payload) => {
    expect(parsePreOperationalStatus(payload)).toBeNull();
  });
});

describe("fetchPreOperationalStatus", () => {
  it("requests only the same-origin status path with a JSON Accept header", async () => {
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(jsonResponse({ lifecycle: "uninitialized", database_selected: true }));

    await expect(fetchPreOperationalStatus()).resolves.toEqual({
      lifecycle: "uninitialized",
      databaseSelected: true,
    });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0]!;
    expect(target).toBe("/api/v1/status");
    expect(STATUS_PATH).toBe("/api/v1/status");
    expect(init?.method).toBe("GET");
    expect(init?.headers).toEqual({ Accept: "application/json" });
    expect(init?.credentials).toBe("omit");
  });

  it("fails without detail when the response is not 200", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("sensitive diagnostic body", { status: 500, statusText: "Boom" }),
    );

    await expect(fetchPreOperationalStatus()).rejects.toMatchObject({
      name: "StatusUnavailableError",
      message: "status_unavailable",
    });
  });

  it("fails without detail when the body is not valid JSON", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("not json", {
        status: 200,
        headers: { "content-type": "application/json; charset=utf-8" },
      }),
    );

    await expect(fetchPreOperationalStatus()).rejects.toMatchObject({
      name: "StatusUnavailableError",
    });
  });

  it("fails without detail when the payload does not match the contract", async () => {
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      jsonResponse({ lifecycle: "uninitialized", database_selected: "yes" }),
    );

    await expect(fetchPreOperationalStatus()).rejects.toMatchObject({
      name: "StatusUnavailableError",
    });
  });

  it("fails without detail when the transport rejects", async () => {
    vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("ECONNREFUSED 127.0.0.1:8443"));

    await expect(fetchPreOperationalStatus()).rejects.toMatchObject({
      name: "StatusUnavailableError",
      message: "status_unavailable",
    });
  });
});
