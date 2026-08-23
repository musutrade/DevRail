import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
  testDir: './e2e',
  testMatch: 'fullstack-smoke.spec.ts',
  fullyParallel: false,
  workers: 1,
  forbidOnly: Boolean(process.env['CI']),
  retries: process.env['CI'] ? 1 : 0,
  reporter: [['list'], ['html', { open: 'never', outputFolder: 'playwright-report-fullstack' }]],
  use: {
    baseURL: 'http://localhost:4300',
    trace: 'on-first-retry',
  },
  expect: { timeout: 15_000 },
  timeout: 60_000,
  projects: [{ name: 'fullstack-chromium', use: { ...devices['Desktop Chrome'] } }],
  webServer: [
    {
      command: 'bash ../scripts/start-fullstack-smoke.sh',
      url: 'http://127.0.0.1:18081/api/v1/readyz',
      reuseExistingServer: false,
      timeout: 300_000,
      stdout: 'pipe',
    },
    {
      command: 'npm start -- --host 127.0.0.1 --port 4300 --proxy-config proxy.fullstack.conf.json',
      url: 'http://localhost:4300',
      reuseExistingServer: false,
      timeout: 120_000,
      stdout: 'pipe',
    },
  ],
});
