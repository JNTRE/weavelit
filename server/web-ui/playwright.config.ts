import { defineConfig, devices } from '@playwright/test';

// The browser smoke test runs against the release Server binary over its real
// direct-TLS listener, so it must run after `cargo build --release`. Global
// setup starts that process and writes its base URL for the specs.
export default defineConfig({
  testDir: 'browser-tests',
  testMatch: '**/*.spec.ts',
  globalSetup: './browser-tests/global-setup.ts',
  globalTeardown: './browser-tests/global-teardown.ts',
  outputDir: 'browser-tests-output',
  fullyParallel: false,
  workers: 1,
  // A retry would hide a genuine regression and would also re-request the rate-
  // limited pre-operational routes, so a failure stays a failure.
  retries: 0,
  forbidOnly: true,
  timeout: 30_000,
  expect: { timeout: 10_000 },
  reporter: [['list']],
  use: {
    // The fixture generates a short-lived self-signed certificate, so the
    // browser cannot chain it to a trust anchor. This is the only place
    // certificate validation is relaxed.
    ignoreHTTPSErrors: true,
    trace: 'off',
    video: 'off',
    screenshot: 'off',
    serviceWorkers: 'block',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
