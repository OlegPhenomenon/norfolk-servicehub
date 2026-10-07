import { test as base, expect, type Page } from '@playwright/test';
export const test = base.extend({ baseURL: async ({}, use) => { await use(process.env.E2E_BASE_URL); } });
export { expect };
export async function ready(page: Page) {
  await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
  await expect(page.getByRole('status').filter({ hasText: /Loading/ })).toHaveCount(0);
  await page.evaluate(() => document.fonts.ready);
}
export async function login(page: Page, name: string) {
  await page.goto('/demo');
  await page.getByRole('button', { name: `Sign in as ${name}`, exact: true }).click();
  if (!['Alexey', 'Ben'].includes(name)) {
    await expect(page).toHaveURL(/\/login\/totp/);
    const code = page.getByRole('region', { name: 'Demo authenticator' }).locator('[aria-label^="Code "]');
    await expect(code).toBeVisible();
    const current = (await code.innerText()).replace(/\s/g, '');
    await page.getByLabel('6-digit code').fill(current);
    const verified = page.waitForResponse(r => r.url().endsWith('/api/auth/totp') && r.request().method() === 'POST');
    await page.getByRole('button', { name: 'Verify and continue' }).click();
    if (!(await verified).ok()) {
      // TOTP replay protection is global to a persona, including across independent contexts.
      await expect(page.getByText('This code has already been used. Wait for the next code.')).toBeVisible();
      await expect.poll(async () => (await code.innerText()).replace(/\s/g, ''), { timeout: 35000 }).not.toBe(current);
      await page.getByLabel('6-digit code').fill((await code.innerText()).replace(/\s/g, ''));
      await page.getByRole('button', { name: 'Verify and continue' }).click();
    }
    await expect(page).toHaveURL(name === 'Mark' ? /\/(staff|admin)/ : /\/staff/);
  } else await expect(page).toHaveURL(/\/my/);
  await ready(page);
}
export async function advance(page: Page) {
  await page.getByRole('button', { name: 'Complete this step', exact: true }).click();
  await page.getByRole('dialog').getByLabel('Reason or completion note').fill('Reviewed in the browser demonstration.');
  await page.getByRole('button', { name: 'Confirm action', exact: true }).click();
  await expect(page.getByRole('dialog')).toHaveCount(0);
}
export async function openSeededCase(page: Page, area: 'my' | 'staff', service = 'rawson-hall-hire') {
  await page.goto(area === 'my' ? '/my' : `/staff/cases?service=${service}`);
  const link = area === 'my' ? page.getByRole('link', { name: /NSH-/ }).first() : page.locator('main table a').first();
  await link.click();
  await expect(page).toHaveURL(new RegExp(`/${area}/cases/\\d+`));
  await ready(page);
}
export function futureDate(days: number) {
  const local = new Intl.DateTimeFormat('en-CA', { timeZone: 'Pacific/Norfolk', year: 'numeric', month: '2-digit', day: '2-digit' }).format(new Date());
  const date = new Date(`${local}T12:00:00Z`);
  date.setUTCDate(date.getUTCDate() + days);
  return date.toISOString().slice(0, 10);
}
export async function startHall(page: Page) {
  await page.goto('/services/rawson-hall-hire');
  await page.getByRole('button', { name: /Start request/ }).click();
  await expect(page).toHaveURL(/\/my\/drafts\/\d+/);
  await page.getByLabel('Name of applicant', { exact: false }).fill('Alexey Turner');
  await page.getByLabel('Postal address').fill('10 Fictional Lane, Norfolk Island');
  await page.getByLabel('Type of organisation').selectOption('Private / Individual');
  await page.getByLabel('Event title and type').fill('Community supper — browser demonstration');
  await page.getByLabel('Date', { exact: true }).fill(futureDate(28));
  await page.getByLabel('Number of guests').fill('30');
  await page.getByLabel('Name of person collecting key').fill('Alexey Turner');
  await page.getByLabel('Will alcohol be served?').selectOption('no');
  await page.getByRole('checkbox', { name: /Public liability policy or Council/ }).check();
  await page.getByRole('checkbox', { name: /I accept the Conditions/ }).check();
  await expect(page.getByText('No confirmed booking overlaps your selected times')).toBeVisible();
}

export async function openRoadTask(page: Page) {
  const roadTasks = page.getByRole('link', { name: 'Inspect reported road issue', exact: true });
  await expect(roadTasks.first()).toBeVisible();
  let selected = false;
  for (const link of await roadTasks.all()) {
    if (await link.locator('xpath=ancestor::section[1]').getByText('Open', { exact: true }).count()) {
      await link.click(); selected = true; break;
    }
  }
  expect(selected, 'An open road inspection is available in the seeded history').toBe(true);
  await expect(page.getByRole('heading', { name: 'Checklist', exact: true })).toBeVisible();
}
