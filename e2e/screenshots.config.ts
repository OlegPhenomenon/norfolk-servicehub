import { defineConfig } from '@playwright/test';
import config from './playwright.config';
export default defineConfig({ ...config, testIgnore: [], testMatch: ['**/screenshots.spec.ts', '**/audit2-evidence.spec.ts'] });
