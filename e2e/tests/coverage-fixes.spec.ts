import { resolve } from 'node:path';
import { test, expect, login, ready } from '../helpers';

test('organisation creation works with keyboard at 200% text size', async ({ page }) => {
  await login(page, 'Alexey');
  await page.goto('/my/organisation');
  await ready(page);
  await page.evaluate(() => { document.documentElement.style.fontSize = '200%'; });
  const name = page.getByLabel('Organisation name');
  await name.focus();
  await expect(name).toBeFocused();
  await page.keyboard.type('Browser Island Business');
  await page.keyboard.press('Tab');
  await expect(page.getByLabel('ABN (if applicable)')).toBeFocused();
  await page.keyboard.type('123');
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'Create organisation', exact: true })).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: 'Browser Island Business', exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth + 1)).toBe(true);
});

test('resident can upload Proof of payment without confirming money', async ({ page, context }) => {
  await login(page, 'Alexey');
  const list = await (await context.request.get('/api/my/cases')).json();
  const item = list.items.find((c: { module: string; status: string }) => c.module === 'venue_booking' && c.status !== 'completed');
  expect(item).toBeTruthy();
  const before = await (await context.request.get(`/api/cases/${item.id}/money`)).json();
  await page.goto(`/my/cases/${item.id}`);
  await page.getByRole('tab', { name: 'Documents', exact: true }).click();
  const form = page.locator('form').filter({ has: page.getByLabel('Category', { exact: false }) });
  await form.getByLabel('Category', { exact: false }).selectOption('receipt');
  await form.getByLabel('Document title').fill('Browser receipt evidence');
  await form.getByLabel('File', { exact: true }).setInputFiles(resolve(import.meta.dirname, '../../server/seed-data/docs/site-plan.pdf'));
  await form.getByRole('button', { name: 'Upload document', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'Browser receipt evidence', exact: true })).toBeVisible();
  await expect(page.getByText('Files are type-checked, not virus-scanned.').first()).toBeVisible();
  const after = await (await context.request.get(`/api/cases/${item.id}/money`)).json();
  expect(after.payments).toEqual(before.payments);
  expect(after.summary).toEqual(before.summary);
  const documents = await (await context.request.get(`/api/cases/${item.id}/documents`)).json();
  expect(documents.some((r: { title: string; category: string }) => r.title === 'Browser receipt evidence' && r.category === 'receipt')).toBe(true);
});

test('Tom records Alexey past inspected bond and sees live partial refund complete', async ({ page, context }) => {
  await login(page, 'Tom');
  const queue = await (await context.request.get('/api/finance/deposits')).json();
  const item = queue.find((c: { applicant_name: string }) => c.applicant_name === 'Alexey Turner');
  expect(item).toBeTruthy();
  await page.goto(`/staff/cases/${item.case_id}?tab=finance.money`);
  await page.getByRole('button', { name: 'Record bond decision', exact: true }).click();
  const dialog = page.getByRole('dialog');
  await dialog.getByLabel('Amount to refund (AUD)').fill('200.00');
  await dialog.getByRole('button', { name: 'Add retained item' }).click();
  await dialog.getByLabel('Reason for this charge').fill('Extra cleaning');
  await dialog.getByLabel('Amount retained (AUD)').fill('50.00');
  await dialog.getByLabel('Explain the decision to the applicant').fill('Past event inspected; extra cleaning charged.');
  await dialog.getByRole('button', { name: 'Record decision and reserve refund' }).click();
  await expect(page.getByText('Bond decision recorded; any refund awaits confirmation')).toBeVisible();
  await dialog.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(page.getByRole('table', { name: 'Refund status' })).toContainText('Refund completed', { timeout: 45000 });
  await expect(page.getByRole('heading', { level: 1 }).locator('..')).toContainText('Completed', { timeout: 45000 });
});

 test('equipment estimates label minutes as estimated', async ({ page, context }) => {
  await login(page, 'Ben');
  const list = await (await context.request.get('/api/my/cases')).json();
  let selected: number | undefined;
  for (const item of list.items.filter((c: { module: string }) => c.module === 'equipment_hire')) {
    const money = await (await context.request.get(`/api/cases/${item.id}/money`)).json();
    if (money.invoices.some((i: { kind: string }) => i.kind === 'estimate')) { selected = item.id; break; }
  }
  expect(selected).toBeDefined();
  await page.goto(`/my/cases/${selected}?tab=finance.money`);
  await expect(page.getByText(/estimated minutes/).first()).toBeVisible();
 });
