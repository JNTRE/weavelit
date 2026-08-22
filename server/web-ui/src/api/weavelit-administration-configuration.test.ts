import { afterEach, describe, expect, it, vi } from "vitest";

import { CSRF_COOKIE_NAME } from "./weavelit-authentication";
import {
  ConfigurationConflictError,
  LOG_CONFIGURATIONS_CHANGE_PATH,
  LOG_CONFIGURATIONS_LIST_PATH,
  TOTP_ENABLEMENT_APPLY_PATH,
  TOTP_ENABLEMENT_PREVIEW_PATH,
  applyTotpEnablement,
  changeLogConfiguration,
  listLogConfigurations,
  previewTotpEnablement,
  readLogConfiguration,
  readLogConfigurationsPage,
  readTotpEnablementApplied,
  readTotpEnablementPreview,
} from "./weavelit-administration-configuration";

const CORRELATION = "configuration-correlation";
const CSRF = "csrf-token";
const PREVIEW = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

function configuration(name = "primary"): Record<string, unknown> {
  return {
    configuration_name: name,
    module: "sqlite",
    enabled: true,
    settings: [],
    assigned_log_types: ["system", "audit"],
  };
}

function envelope(result: unknown): Record<string, unknown> {
  return { result, correlation_id: CORRELATION };
}

function response(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), {
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
  if (typeof init?.body !== "string") throw new TypeError("expected JSON body");
  return JSON.parse(init.body) as Record<string, unknown>;
}

afterEach(() => {
  vi.restoreAllMocks();
  Reflect.deleteProperty(document, "cookie");
});

describe("Configuration API", () => {
  it("accepts exact safe projections and rejects internal or misordered fields", () => {
    expect(readLogConfiguration(envelope(configuration()))).toEqual({
      configurationName: "primary",
      module: "sqlite",
      enabled: true,
      settings: [],
      assignedLogTypes: ["system", "audit"],
    });
    expect(readLogConfiguration(envelope({ ...configuration(), generation: 2 }))).toBeNull();
    expect(
      readLogConfiguration(
        envelope({ ...configuration(), settings: [{ key: "path", value: "/private/log" }] }),
      ),
    ).toBeNull();
    expect(
      readLogConfiguration(
        envelope({ ...configuration(), assigned_log_types: ["audit", "system"] }),
      ),
    ).toBeNull();
    expect(
      readLogConfigurationsPage(
        envelope({ items: [configuration("second"), configuration("first")], next_cursor: null }),
      ),
    ).toBeNull();
  });

  it("accepts only an exact canonical TOTP preview result", () => {
    const exact = envelope({
      module: "totp",
      current_enabled: true,
      desired_enabled: false,
      affected_users: 2,
      totp_enablement_preview: PREVIEW,
    });
    expect(readTotpEnablementPreview(exact)).toEqual({
      currentEnabled: true,
      desiredEnabled: false,
      affectedUsers: 2,
      preview: PREVIEW,
    });
    expect(
      readTotpEnablementPreview(
        envelope({
          ...(exact.result as object),
          totp_enablement_preview: `${PREVIEW.slice(0, 42)}B`,
        }),
      ),
    ).toBeNull();
    expect(
      readTotpEnablementPreview(envelope({ ...(exact.result as object), ticket: PREVIEW })),
    ).toBeNull();
  });

  it("accepts committed results only without an Audit delivery field", () => {
    const applied = { module: "totp", current_enabled: false, affected_users: 1 };
    expect(readTotpEnablementApplied(envelope(applied))).toEqual({
      currentEnabled: false,
      affectedUsers: 1,
    });
    expect(
      readTotpEnablementApplied(envelope({ ...applied, audit_delivery: "pending" })),
    ).toBeNull();
    expect(readLogConfiguration(envelope(configuration()))).not.toBeNull();
    expect(
      readLogConfiguration(envelope({ ...configuration(), audit_delivery: "acknowledged" })),
    ).toBeNull();
  });

  it("sends one exact same-origin request per action and never retries", async () => {
    globalThis.localStorage.clear();
    globalThis.sessionStorage.clear();
    const setItemSpy = vi.spyOn(Storage.prototype, "setItem");
    globalThis.sessionStorage.setItem("sanity-check", "safe-value");
    expect(setItemSpy).toHaveBeenCalledWith("sanity-check", "safe-value");
    expect(globalThis.sessionStorage.length).toBe(1);
    globalThis.sessionStorage.clear();
    setItemSpy.mockClear();

    csrf();
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation((target) => {
      if (target === TOTP_ENABLEMENT_PREVIEW_PATH)
        return Promise.resolve(
          response(
            envelope({
              module: "totp",
              current_enabled: true,
              desired_enabled: false,
              affected_users: 1,
              totp_enablement_preview: PREVIEW,
            }),
          ),
        );
      if (target === TOTP_ENABLEMENT_APPLY_PATH)
        return Promise.resolve(
          response(envelope({ module: "totp", current_enabled: false, affected_users: 1 })),
        );
      if (target === LOG_CONFIGURATIONS_LIST_PATH)
        return Promise.resolve(response(envelope({ items: [configuration()], next_cursor: null })));
      return Promise.resolve(response(envelope(configuration())));
    });

    const preview = await previewTotpEnablement(false);
    expect(preview.preview).toBe(PREVIEW);
    expect(setItemSpy).not.toHaveBeenCalled();
    const applied = await applyTotpEnablement(false, PREVIEW);
    expect(setItemSpy).not.toHaveBeenCalled();
    await listLogConfigurations();
    const changed = await changeLogConfiguration({
      configurationName: "primary",
      enabled: true,
      settings: [],
      assignments: [
        { logType: "system", configurationName: "primary" },
        { logType: "audit", configurationName: "primary" },
      ],
    });

    expect(applied).toEqual({ currentEnabled: false, affectedUsers: 1 });
    expect(changed).toEqual({
      configurationName: "primary",
      module: "sqlite",
      enabled: true,
      settings: [],
      assignedLogTypes: ["system", "audit"],
    });
    expect(fetchMock).toHaveBeenCalledTimes(4);
    expect(body(fetchMock.mock.calls[0]![1])).toEqual({ enabled: false });
    expect(body(fetchMock.mock.calls[1]![1])).toEqual({
      enabled: false,
      totp_enablement_preview: PREVIEW,
    });
    expect(fetchMock.mock.calls[3]![0]).toBe(LOG_CONFIGURATIONS_CHANGE_PATH);
    expect(body(fetchMock.mock.calls[3]![1])).toEqual({
      configuration_name: "primary",
      enabled: true,
      settings: [],
      assignments: [
        { log_type: "system", configuration_name: "primary" },
        { log_type: "audit", configuration_name: "primary" },
      ],
    });
    for (const [, init] of fetchMock.mock.calls)
      expect(init).toMatchObject({
        method: "PUT",
        credentials: "same-origin",
        cache: "no-store",
        redirect: "error",
      });
    setItemSpy.mockRestore();
  });

  it("maps a reported stale preview to conflict without retrying", async () => {
    csrf();
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(response({ error: "conflict", correlation_id: CORRELATION }, 409));

    await expect(applyTotpEnablement(false, PREVIEW)).rejects.toBeInstanceOf(
      ConfigurationConflictError,
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });
});
