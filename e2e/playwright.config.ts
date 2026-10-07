import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: './tests', globalSetup: './global-setup.ts',
  testIgnore: '**/screenshots.spec.ts',
  fullyParallel: false, workers: 1, retries: 0,
  timeout: 90000, expect: { timeout: 15000 },
  reporter: [['list'], ['html', { open: 'never' }]],
  use: { browserName: 'chromium', actionTimeout: 15000, viewport: { width: 1440, height: 900 },
    trace: 'retain-on-failure', screenshot: 'only-on-failure' },
});
