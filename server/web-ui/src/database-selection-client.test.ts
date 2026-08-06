import { describe, expect, it, vi } from 'vitest';

import {
  DATABASE_SELECTION_PATH,
  SQLITE_SELECTION_BODY,
  parseDatabaseSelectionResult,
  selectSqliteDatabase,
} from './database-selection-client';

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json; charset=utf-8' },
  });
}

describe('parseDatabaseSelectionResult', () => {
  it('accepts the documented success projection', () => {
    expect(
      parseDatabaseSelectionResult({ lifecycle: 'uninitialized', database_selected: true }),
    ).toEqual({ lifecycle: 'uninitialized', databaseSelected: true });
  });

  it('ignores additive fields permitted by the versioned contract', () => {
    expect(
      parseDatabaseSelectionResult({
        lifecycle: 'uninitialized',
        database_selected: true,
        future_field: 'ignored',
      }),
    ).toEqual({ lifecycle: 'uninitialized', databaseSelected: true });
  });

  it.each([
    ['null', null],
    ['undefined', undefined],
    ['an array', [{ lifecycle: 'uninitialized', database_selected: true }]],
    ['a string', 'uninitialized'],
    ['a number', 200],
    ['a missing lifecycle', { database_selected: true }],
    ['an unexpected lifecycle', { lifecycle: 'operational', database_selected: true }],
    ['a missing selection flag', { lifecycle: 'uninitialized' }],
    ['a non-boolean selection flag', { lifecycle: 'uninitialized', database_selected: 'true' }],
    ['a success payload denying selection', { lifecycle: 'uninitialized', database_selected: false }],
  ])('rejects %s', (_label, payload) => {
    expect(parseDatabaseSelectionResult(payload)).toBeNull();
  });
});

describe('selectSqliteDatabase', () => {
  it('issues the documented same-origin request without credentials', async () => {
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(jsonResponse({ lifecycle: 'uninitialized', database_selected: true }));

    await expect(selectSqliteDatabase()).resolves.toEqual({
      lifecycle: 'uninitialized',
      databaseSelected: true,
    });

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [target, init] = fetchMock.mock.calls[0]!;
    expect(target).toBe('/api/v1/application-database');
    expect(DATABASE_SELECTION_PATH).toBe('/api/v1/application-database');
    expect(init?.method).toBe('PUT');
    expect(init?.headers).toEqual({
      Accept: 'application/json',
      'Content-Type': 'application/json',
      'X-Weavelit-CSRF': '1',
    });
    expect(init?.body).toBe('{"backend":"sqlite","settings":{}}');
    expect(SQLITE_SELECTION_BODY).toBe('{"backend":"sqlite","settings":{}}');
    expect(new TextEncoder().encode(SQLITE_SELECTION_BODY).byteLength).toBe(34);
    expect(init?.credentials).toBe('omit');
    expect(init?.cache).toBe('no-store');
    expect(init?.redirect).toBe('error');
  });

  it('never sets the browser-controlled Host or Origin headers', async () => {
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(jsonResponse({ lifecycle: 'uninitialized', database_selected: true }));

    await selectSqliteDatabase();

    const headers = fetchMock.mock.calls[0]![1]?.headers as Record<string, string>;
    const names = Object.keys(headers).map((name) => name.toLowerCase());
    expect(names).not.toContain('host');
    expect(names).not.toContain('origin');
    expect(headers['Content-Type']).not.toContain('charset');
  });

  it('forwards an abort signal when one is supplied', async () => {
    const fetchMock = vi
      .spyOn(globalThis, 'fetch')
      .mockResolvedValue(jsonResponse({ lifecycle: 'uninitialized', database_selected: true }));
    const controller = new AbortController();

    await selectSqliteDatabase(controller.signal);

    expect(fetchMock.mock.calls[0]![1]?.signal).toBe(controller.signal);
  });

  it.each([
    [400, 'bad_request'],
    [403, 'request_origin_denied'],
    [405, 'method_not_allowed'],
    [409, 'database_selection_not_allowed'],
    [503, 'service_unavailable'],
  ])('fails without detail on %i', async (status, code) => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(jsonResponse({ error: code }, status));

    const error = await selectSqliteDatabase().catch((reason: unknown) => reason);

    expect(error).toBeInstanceOf(Error);
    expect((error as Error).name).toBe('DatabaseSelectionFailedError');
    expect((error as Error).message).toBe('database_selection_failed');
    expect(JSON.stringify(error)).not.toContain(code);
    expect((error as Error).message).not.toContain(String(status));
  });

  it('fails without detail when a non-200 response carries a diagnostic body', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response('sensitive diagnostic body', { status: 500, statusText: 'Boom' }),
    );

    await expect(selectSqliteDatabase()).rejects.toMatchObject({
      name: 'DatabaseSelectionFailedError',
      message: 'database_selection_failed',
    });
  });

  it('fails without detail when the success body is not valid JSON', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response('not json', {
        status: 200,
        headers: { 'content-type': 'application/json; charset=utf-8' },
      }),
    );

    await expect(selectSqliteDatabase()).rejects.toMatchObject({
      name: 'DatabaseSelectionFailedError',
      message: 'database_selection_failed',
    });
  });

  it.each([
    ['a non-object payload', '"uninitialized"'],
    ['an array payload', '[{"lifecycle":"uninitialized","database_selected":true}]'],
    ['a wrongly typed selection flag', '{"lifecycle":"uninitialized","database_selected":"yes"}'],
    ['a missing documented field', '{"lifecycle":"uninitialized"}'],
    ['a success payload denying selection', '{"lifecycle":"uninitialized","database_selected":false}'],
  ])('fails without detail on %s', async (_label, body) => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(body, {
        status: 200,
        headers: { 'content-type': 'application/json; charset=utf-8' },
      }),
    );

    await expect(selectSqliteDatabase()).rejects.toMatchObject({
      name: 'DatabaseSelectionFailedError',
      message: 'database_selection_failed',
    });
  });

  it('fails without detail when the transport rejects', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('ECONNREFUSED 127.0.0.1:8443'));

    await expect(selectSqliteDatabase()).rejects.toMatchObject({
      name: 'DatabaseSelectionFailedError',
      message: 'database_selection_failed',
    });
  });
});
