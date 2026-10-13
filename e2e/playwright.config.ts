import { defineConfig, devices } from '@playwright/test';

/**
 * Procera E2E configuration (docs/23-E2E-TEST-PLAN.md).
 *
 * - `api` project: request-level scenarios, no browser needed (runs everywhere, incl. the P0 smoke pack).
 * - Browser projects (chromium/firefox/webkit × desktop/mobile) are enabled per release gate (docs/07 §4).
 *
 * The target stack is either a locally started `procera` built with `--features e2e`, or
 * `deploy/compose.e2e.yml`. Set PROCERA_BASE_URL to point at it.
 */
const baseURL = process.env.PROCERA_BASE_URL ?? 'http://127.0.0.1:18080';

export default defineConfig({
  testDir: './packs',
  fullyParallel: true,
  // The suite runs a debug server build under parallel workers: allow 10 s for a UI state to appear
  // (what is asserted is unchanged; only how long a check waits).
  expect: { timeout: 10_000 },
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0, // docs/23 §7: at most one retry; a pass-on-retry is reported as flaky
  reporter: process.env.CI ? [['github'], ['html', { open: 'never' }], ['junit', { outputFile: 'test-results/junit.xml' }]] : 'list',
  use: {
    baseURL,
    trace: 'retain-on-failure',
    video: 'retain-on-failure',
    extraHTTPHeaders: { accept: 'application/json' },
  },
  projects: [
    { name: 'api', testMatch: /.*\.api\.spec\.ts/ },
    { name: 'chromium-desktop', testMatch: /.*\.ui\.spec\.ts/, use: { ...devices['Desktop Chrome'] } },
    { name: 'chromium-mobile', testMatch: /.*\.ui\.spec\.ts/, use: { ...devices['Pixel 7'] } },
    { name: 'firefox-desktop', testMatch: /.*\.ui\.spec\.ts/, use: { ...devices['Desktop Firefox'] } },
    { name: 'webkit-desktop', testMatch: /.*\.ui\.spec\.ts/, use: { ...devices['Desktop Safari'] } },
  ],
});
