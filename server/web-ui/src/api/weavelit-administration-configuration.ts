import { CSRF_HEADER_NAME, readCsrfToken } from "./weavelit-authentication";

export const TOTP_ENABLEMENT_PREVIEW_PATH =
  "/api/v1/administration/mfa-modules/totp/enablement/preview";
export const TOTP_ENABLEMENT_APPLY_PATH =
  "/api/v1/administration/mfa-modules/totp/enablement/apply";
export const LOG_CONFIGURATIONS_LIST_PATH = "/api/v1/administration/log-configurations/list";
export const LOG_CONFIGURATIONS_VIEW_PATH = "/api/v1/administration/log-configurations/view";
export const LOG_CONFIGURATIONS_CHANGE_PATH = "/api/v1/administration/log-configurations/change";

const CORRELATION_PATTERN = /^[a-z0-9-]{1,64}$/;
const CURSOR_PATTERN = /^[A-Za-z0-9_-]{1,512}$/;
const PREVIEW_PATTERN = /^[A-Za-z0-9_-]{42}[AEIMQUYcgkosw048]$/;
const MAX_NAME_BYTES = 256;
const MAX_SETTING_KEY_BYTES = 256;
const MAX_SETTING_VALUE_BYTES = 4 * 1024;
const MAX_SETTINGS = 64;
const MAX_PAGE_ITEMS = 100;

const ENVELOPE_FIELDS = new Set(["result", "correlation_id"]);
const PREVIEW_FIELDS = new Set([
  "module",
  "current_enabled",
  "desired_enabled",
  "affected_users",
  "totp_enablement_preview",
]);
const APPLIED_FIELDS = new Set(["module", "current_enabled", "affected_users"]);
const PROJECTION_FIELDS = new Set([
  "configuration_name",
  "module",
  "enabled",
  "settings",
  "assigned_log_types",
]);
const SETTING_FIELDS = new Set(["key", "value"]);
const PAGE_FIELDS = new Set(["items", "next_cursor"]);

export type LogType = "system" | "audit";

export interface TotpEnablementPreview {
  readonly currentEnabled: boolean;
  readonly desiredEnabled: boolean;
  readonly affectedUsers: number;
  readonly preview: string;
}

export interface TotpEnablementApplied {
  readonly currentEnabled: boolean;
  readonly affectedUsers: number;
}

export interface LogSetting {
  readonly key: string;
  readonly value: string;
}

export interface LogConfiguration {
  readonly configurationName: string;
  readonly module: string;
  readonly enabled: boolean;
  readonly settings: readonly LogSetting[];
  readonly assignedLogTypes: readonly LogType[];
}

export interface LogConfigurationsPage {
  readonly items: readonly LogConfiguration[];
  readonly nextCursor: string | null;
}

export interface LogConfigurationChange {
  readonly configurationName: string;
  readonly enabled?: boolean;
  readonly settings?: readonly LogSetting[];
  readonly assignments?: readonly {
    readonly logType: LogType;
    readonly configurationName: string;
  }[];
}

export class ConfigurationUnavailableError extends Error {
  constructor() {
    super("configuration_unavailable");
    this.name = "ConfigurationUnavailableError";
  }
}

export class ConfigurationSessionInvalidError extends ConfigurationUnavailableError {
  constructor() {
    super();
    this.name = "ConfigurationSessionInvalidError";
  }
}

export class ConfigurationRefusedError extends Error {
  constructor() {
    super("configuration_refused");
    this.name = "ConfigurationRefusedError";
  }
}

export class ConfigurationConflictError extends ConfigurationRefusedError {
  constructor() {
    super();
    this.name = "ConfigurationConflictError";
  }
}

export class ConfigurationIndeterminateError extends Error {
  constructor() {
    super("configuration_indeterminate");
    this.name = "ConfigurationIndeterminateError";
  }
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function exact(value: Record<string, unknown>, fields: ReadonlySet<string>): boolean {
  const keys = Object.keys(value);
  return keys.length === fields.size && keys.every((key) => fields.has(key));
}

function safeText(value: unknown, maximum: number): string | null {
  return typeof value === "string" &&
    value !== "" &&
    !/[\u0000-\u001f\u007f]/.test(value) &&
    new TextEncoder().encode(value).length <= maximum
    ? value
    : null;
}

function resultObject(payload: unknown): Record<string, unknown> | null {
  const envelope = objectValue(payload);
  return envelope !== null &&
    exact(envelope, ENVELOPE_FIELDS) &&
    typeof envelope.correlation_id === "string" &&
    CORRELATION_PATTERN.test(envelope.correlation_id)
    ? objectValue(envelope.result)
    : null;
}

export function readTotpEnablementPreview(payload: unknown): TotpEnablementPreview | null {
  const result = resultObject(payload);
  if (
    result === null ||
    !exact(result, PREVIEW_FIELDS) ||
    result.module !== "totp" ||
    typeof result.current_enabled !== "boolean" ||
    typeof result.desired_enabled !== "boolean" ||
    !Number.isSafeInteger(result.affected_users) ||
    (result.affected_users as number) < 0 ||
    typeof result.totp_enablement_preview !== "string" ||
    !PREVIEW_PATTERN.test(result.totp_enablement_preview)
  )
    return null;
  return {
    currentEnabled: result.current_enabled,
    desiredEnabled: result.desired_enabled,
    affectedUsers: result.affected_users as number,
    preview: result.totp_enablement_preview,
  };
}

export function readTotpEnablementApplied(payload: unknown): TotpEnablementApplied | null {
  const result = resultObject(payload);
  if (
    result === null ||
    !exact(result, APPLIED_FIELDS) ||
    result.module !== "totp" ||
    typeof result.current_enabled !== "boolean" ||
    !Number.isSafeInteger(result.affected_users) ||
    (result.affected_users as number) < 0
  )
    return null;
  return {
    currentEnabled: result.current_enabled,
    affectedUsers: result.affected_users as number,
  };
}

function readSetting(value: unknown): LogSetting | null {
  const setting = objectValue(value);
  if (setting === null || !exact(setting, SETTING_FIELDS)) return null;
  const key = safeText(setting.key, MAX_SETTING_KEY_BYTES);
  const settingValue = safeText(setting.value, MAX_SETTING_VALUE_BYTES);
  if (key === null || settingValue === null) return null;
  const normalized = key.toLowerCase();
  if (
    normalized === "path" ||
    normalized === "file" ||
    normalized === "directory" ||
    normalized.endsWith("_path") ||
    normalized.endsWith("_file") ||
    normalized.endsWith("_directory")
  )
    return null;
  return { key, value: settingValue };
}

export function readLogConfigurationValue(value: unknown): LogConfiguration | null {
  const result = objectValue(value);
  if (
    result === null ||
    !exact(result, PROJECTION_FIELDS) ||
    typeof result.enabled !== "boolean" ||
    !Array.isArray(result.settings) ||
    result.settings.length > MAX_SETTINGS ||
    !Array.isArray(result.assigned_log_types) ||
    result.assigned_log_types.length > 2
  )
    return null;
  const configurationName = safeText(result.configuration_name, MAX_NAME_BYTES);
  const module = safeText(result.module, MAX_NAME_BYTES);
  if (configurationName === null || module === null) return null;
  const settings: LogSetting[] = [];
  for (const value of result.settings) {
    const setting = readSetting(value);
    const previous = settings.at(-1);
    if (setting === null || (previous !== undefined && previous.key >= setting.key)) return null;
    settings.push(setting);
  }
  const assignedLogTypes: LogType[] = [];
  const rawAssignedLogTypes: unknown[] = result.assigned_log_types;
  for (const value of rawAssignedLogTypes) {
    if (value !== "system" && value !== "audit") return null;
    const previous = assignedLogTypes.at(-1);
    if (
      previous !== undefined &&
      (previous === value || (previous === "audit" && value === "system"))
    )
      return null;
    assignedLogTypes.push(value);
  }
  return { configurationName, module, enabled: result.enabled, settings, assignedLogTypes };
}

export function readLogConfiguration(payload: unknown): LogConfiguration | null {
  return readLogConfigurationValue(resultObject(payload));
}

export function readLogConfigurationsPage(payload: unknown): LogConfigurationsPage | null {
  const result = resultObject(payload);
  if (
    result === null ||
    !exact(result, PAGE_FIELDS) ||
    !Array.isArray(result.items) ||
    result.items.length > MAX_PAGE_ITEMS
  )
    return null;
  const items: LogConfiguration[] = [];
  for (const value of result.items) {
    const item = readLogConfigurationValue(value);
    const previous = items.at(-1);
    if (
      item === null ||
      (previous !== undefined && previous.configurationName >= item.configurationName)
    )
      return null;
    items.push(item);
  }
  const cursor = result.next_cursor;
  if (cursor !== null && (typeof cursor !== "string" || !CURSOR_PATTERN.test(cursor))) return null;
  return { items, nextCursor: cursor };
}

async function reportedError(response: Response): Promise<string | null> {
  const allowed = new Map<number, ReadonlySet<string>>([
    [400, new Set(["bad_request"])],
    [401, new Set(["session_invalid"])],
    [403, new Set(["request_origin_denied", "authorization_denied"])],
    [404, new Set(["not_found"])],
    [405, new Set(["method_not_allowed"])],
    [409, new Set(["conflict"])],
    [503, new Set(["service_unavailable"])],
  ]).get(response.status);
  if (allowed === undefined) return null;
  try {
    const value = objectValue(await response.json());
    return value !== null &&
      Object.keys(value).length === 2 &&
      typeof value.error === "string" &&
      allowed.has(value.error) &&
      typeof value.correlation_id === "string" &&
      CORRELATION_PATTERN.test(value.correlation_id)
      ? value.error
      : null;
  } catch {
    return null;
  }
}

async function request(path: string, body: object, mutation: boolean): Promise<unknown> {
  const csrf = readCsrfToken();
  if (csrf === null)
    throw mutation ? new ConfigurationRefusedError() : new ConfigurationUnavailableError();
  let response: Response;
  try {
    response = await fetch(path, {
      method: "PUT",
      headers: {
        Accept: "application/json",
        "Content-Type": "application/json",
        [CSRF_HEADER_NAME]: csrf,
      },
      body: JSON.stringify(body),
      credentials: "same-origin",
      cache: "no-store",
      redirect: "error",
    });
  } catch {
    throw mutation ? new ConfigurationIndeterminateError() : new ConfigurationUnavailableError();
  }
  if (response.status !== 200) {
    const error = await reportedError(response);
    if (!mutation)
      throw error === "session_invalid"
        ? new ConfigurationSessionInvalidError()
        : new ConfigurationUnavailableError();
    if (error === "conflict") throw new ConfigurationConflictError();
    if (error !== null && error !== "service_unavailable") throw new ConfigurationRefusedError();
    throw new ConfigurationIndeterminateError();
  }
  try {
    return await response.json();
  } catch {
    throw mutation ? new ConfigurationIndeterminateError() : new ConfigurationUnavailableError();
  }
}

export async function previewTotpEnablement(enabled: boolean): Promise<TotpEnablementPreview> {
  const result = readTotpEnablementPreview(
    await request(TOTP_ENABLEMENT_PREVIEW_PATH, { enabled }, false),
  );
  if (result?.desiredEnabled !== enabled) throw new ConfigurationUnavailableError();
  return result;
}

export async function applyTotpEnablement(
  enabled: boolean,
  preview: string,
): Promise<TotpEnablementApplied> {
  const result = readTotpEnablementApplied(
    await request(TOTP_ENABLEMENT_APPLY_PATH, { enabled, totp_enablement_preview: preview }, true),
  );
  if (result?.currentEnabled !== enabled) throw new ConfigurationIndeterminateError();
  return result;
}

export async function listLogConfigurations(cursor?: string): Promise<LogConfigurationsPage> {
  const result = readLogConfigurationsPage(
    await request(LOG_CONFIGURATIONS_LIST_PATH, cursor === undefined ? {} : { cursor }, false),
  );
  if (result === null) throw new ConfigurationUnavailableError();
  return result;
}

export async function viewLogConfiguration(configurationName: string): Promise<LogConfiguration> {
  const result = readLogConfiguration(
    await request(LOG_CONFIGURATIONS_VIEW_PATH, { configuration_name: configurationName }, false),
  );
  if (result?.configurationName !== configurationName) throw new ConfigurationUnavailableError();
  return result;
}

export async function changeLogConfiguration(
  change: LogConfigurationChange,
): Promise<LogConfiguration> {
  const body: Record<string, unknown> = { configuration_name: change.configurationName };
  if (change.enabled !== undefined) body.enabled = change.enabled;
  if (change.settings !== undefined) body.settings = change.settings;
  if (change.assignments !== undefined)
    body.assignments = change.assignments.map((assignment) => ({
      log_type: assignment.logType,
      configuration_name: assignment.configurationName,
    }));
  const result = readLogConfiguration(await request(LOG_CONFIGURATIONS_CHANGE_PATH, body, true));
  if (result?.configurationName !== change.configurationName)
    throw new ConfigurationIndeterminateError();
  return result;
}
