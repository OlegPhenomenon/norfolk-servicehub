import { mkdirSync } from 'node:fs';
import { resolve } from 'node:path';
import { test, expect, login, ready, startHall, futureDate } from '../helpers';

test('regenerate the ten README screenshots from a fresh seeded demo', async ({ page, browser, baseURL }) => {
  const directory = resolve(import.meta.dirname, '../../docs/screenshots');
  mkdirSync(directory, { recursive: true });
  const capture = async (name: string) => {
    await ready(page);
    await expect(page.locator('main')).not.toContainText('Loading…');
    // Let success announcements finish before capturing the product.
    await expect(page.getByRole('button', { name: 'Dismiss', exact: true })).toHaveCount(0);
    await page.screenshot({ path: resolve(directory, name), animations: 'disabled' });
  };
  await page.goto('/');
  await capture('home.png');
  await page.goto('/services');
  await expect(page.getByRole('link', { name: /Rawson Hall/ }).first()).toBeVisible();
  await capture('catalogue.png');
  await page.goto('/notices');
  await page.getByRole('link', { name: 'Read notice and published documents' }).first().click();
  await page.locator('summary').first().click();
  const plan = page.getByRole('img', { name: /published copy with approved redactions/ }).first();
  await expect(plan).toBeVisible();
  await expect.poll(() => plan.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
  await page.getByRole('heading', { level: 1 }).evaluate(el => window.scrollTo(0, window.scrollY + el.getBoundingClientRect().top - 32));
  await capture('exhibition.png');
  await login(page, 'Alexey');
  await startHall(page);
  await page.getByLabel('Date', { exact: true }).fill(futureDate(7));
  await page.getByLabel('Start (Norfolk time)').fill('18:00');
  await page.getByLabel('End (Norfolk time)').fill('23:00');
  await expect(page.getByText('(includes buffers)', { exact: false }).first()).toBeVisible();
  await page.getByRole('group', { name: 'Date, time, space and number of guests', exact: true })
    .evaluate(el => window.scrollTo(0, window.scrollY + el.getBoundingClientRect().top - 32));
  await capture('hall-booking.png');
  const olgaContext = await browser.newContext({ baseURL, viewport: { width: 1440, height: 900 } });
  try {
    const olga = await olgaContext.newPage();
    await login(olga, 'Olga');
    await olga.goto('/staff/cases?service=rawson-hall-hire&status=in_progress');
    // The paid confirmation and upcoming field-task cases have active action bars.
    await olga.locator('main table a').first().click();
    await expect(olga.getByRole('button', { name: 'Cancel request', exact: true })).toBeVisible();
    await olga.evaluate(() => document.fonts.ready);
    await olga.screenshot({ path: resolve(directory, 'staff-case.png'), animations: 'disabled' });
    await olga.goto('/staff/calendar');
    await olga.getByLabel('Week starting').fill(futureDate(6));
    await expect(olga.locator('table')).toContainText('Rawson Hall');
    await olga.screenshot({ path: resolve(directory, 'calendar.png'), animations: 'disabled' });
  } finally { await olgaContext.close(); }
  await login(page, 'Tom');
  await page.goto('/staff/finance');
  await expect(page.getByRole('table', { name: 'Outstanding invoices', exact: true })).toContainText('INV-');
  await capture('finance.png');
  await login(page, 'Helen');
  await page.goto('/staff/dashboard');
  await expect(page.getByText('Open requests', { exact: true }).first()).toBeVisible();
  await capture('dashboard.png');
  await login(page, 'Mark');
  await page.goto('/admin/services');
  await page.getByRole('link', { name: /Dog registration/i }).click();
  await page.getByRole('tab', { name: 'Fields', exact: true }).click();
  await expect(page.getByLabel('Label', { exact: false }).first()).toBeVisible();
  await capture('service-builder.png');
  await page.setViewportSize({ width: 390, height: 844 });
  await login(page, 'Jake');
  await page.goto('/staff/field');
  await expect(page.locator('main a[href^="/staff/field/"]').first()).toBeVisible();
  await capture('field-mobile.png');
});
