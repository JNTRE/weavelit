import { expect, test, type Page, type Response } from '@playwright/test';

import { readFixtureState } from './fixture-support';

/** The exact assets the page is expected to request, with their required media types. */
const EXPECTED_ASSETS = [
  { path: '/', mediaType: 'text/html; charset=utf-8' },
  { path: '/assets/weavelit-application.js', mediaType: 'text/javascript; charset=utf-8' },
  { path: '/assets/weavelit-application.css', mediaType: 'text/css; charset=utf-8' },
] as const;

const ASSET_CONTENT_SECURITY_POLICY =
  "default-src 'none'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; " +
  "form-action 'none'; script-src 'self'; style-src 'self'; connect-src 'self'";

const STATUS_PATH = '/api/v1/status';

/** A fresh, empty state root has no selected Application Database. */
const UNSELECTED_MESSAGE = 'No Application Database is selected for this deployment.';

interface ObservedResponse {
  readonly path: string;
  readonly status: number;
  readonly headers: Record<string, string>;
}

interface Observations {
  readonly responses: ObservedResponse[];
  readonly consoleMessages: string[];
  readonly failedRequests: string[];
}

function observe(page: Page): Observations {
  const responses: ObservedResponse[] = [];
  const consoleMessages: string[] = [];
  const failedRequests: string[] = [];

  page.on('response', (response: Response) => {
    responses.push({
      path: new URL(response.url()).pathname,
      status: response.status(),
      headers: response.headers(),
    });
  });
  page.on('requestfailed', (request) => {
    failedRequests.push(
      `${request.method()} ${request.url()} failed: ${request.failure()?.errorText ?? 'unknown error'}`,
    );
  });
  page.on('console', (message) => {
    if (message.type() === 'error' || message.type() === 'warning') {
      consoleMessages.push(`console.${message.type()}: ${message.text()}`);
    }
  });
  page.on('pageerror', (error) => {
    consoleMessages.push(`uncaught: ${error.message}`);
  });

  return { responses, consoleMessages, failedRequests };
}

function responseFor(observations: Observations, path: string): ObservedResponse {
  const matching = observations.responses.filter((response) => response.path === path);
  expect(
    matching,
    `expected exactly one response for ${path}, observed ${JSON.stringify(observations.responses)}`,
  ).toHaveLength(1);
  return matching[0] as ObservedResponse;
}

test('the release Server delivers the pre-operational Web UI over direct TLS', async ({ page }) => {
  const { baseUrl } = readFixtureState();
  const observations = observe(page);

  const statusResponse = page.waitForResponse(
    (response) => new URL(response.url()).pathname === STATUS_PATH,
  );
  const documentResponse = await page.goto(baseUrl, { waitUntil: 'load' });

  // The page loads over HTTPS from the real release binary.
  expect(page.url()).toBe(`${baseUrl}/`);
  expect(documentResponse?.status()).toBe(200);

  // The application performs the same-origin status request and the Server
  // reports the actual state of the fresh, empty state root.
  const status = await statusResponse;
  expect(status.status()).toBe(200);
  expect(status.headers()['content-type']).toBe('application/json; charset=utf-8');
  expect(await status.json()).toEqual({ lifecycle: 'uninitialized', database_selected: false });

  // The application renders that actual state rather than a placeholder.
  const rendered = page.locator('[data-status-state]');
  await expect(rendered).toHaveAttribute('data-status-state', 'available');
  await expect(rendered).toHaveText(UNSELECTED_MESSAGE);
  await expect(page.locator('h1')).toHaveText('Weavelit Server');
  // The stylesheet was parsed and applied, not merely fetched.
  await expect(page.locator('main.shell')).toHaveCSS('max-width', '640px');

  // Each asset loads with its exact media type and the required security headers.
  for (const asset of EXPECTED_ASSETS) {
    const response = responseFor(observations, asset.path);
    expect(response.status, `${asset.path} status`).toBe(200);
    expect(response.headers['content-type'], `${asset.path} content type`).toBe(asset.mediaType);
    expect(response.headers['content-security-policy'], `${asset.path} content security policy`).toBe(
      ASSET_CONTENT_SECURITY_POLICY,
    );
    expect(response.headers['x-content-type-options'], `${asset.path} nosniff`).toBe('nosniff');
    expect(response.headers['cache-control'], `${asset.path} cache control`).toBe('no-store');
  }

  // The page requests nothing beyond the allowlisted assets and the status route.
  expect(observations.responses.map((response) => response.path).sort()).toEqual(
    [...EXPECTED_ASSETS.map((asset) => asset.path), STATUS_PATH].sort(),
  );

  // Zero console errors and zero failed network requests.
  expect(observations.consoleMessages, 'browser console output').toEqual([]);
  expect(observations.failedRequests, 'failed network requests').toEqual([]);
  expect(
    observations.responses.filter((response) => response.status !== 200),
    'non-200 responses',
  ).toEqual([]);
});
