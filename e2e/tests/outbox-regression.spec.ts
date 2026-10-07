import { test, expect, login, openRoadTask } from '../helpers';

test('field saves made during an in-flight sync are sent without another reconnect', async ({ page, context }) => {
  await login(page, 'Jake');
  await page.goto('/staff/field');
  await openRoadTask(page);
  const id = new URL(page.url()).pathname.split('/').pop();
  const before = await (await context.request.get(`/api/field/tasks/${id}`)).json();
  // Delay the real first command while further UI actions are saved; never mock its response.
  let release!: () => void;
  let started!: () => void;
  const held = new Promise<void>(done => { release = done; });
  const sending = new Promise<void>(done => { started = done; });
  let first = true;
  await page.route(`**/api/field/tasks/${id}/updates`, async route => {
    if (first) { first = false; started(); await held; }
    await route.continue();
  });
  try {
    const checklist = page.locator('section').filter({ has: page.getByRole('heading', { name: 'Checklist', exact: true }) }).getByRole('checkbox');
    await checklist.first().click();
    await sending;
    await expect(checklist.first()).toBeChecked();
    await expect(checklist.first()).toBeEnabled();
    await checklist.nth(1).click();
    await expect(checklist.nth(1)).toBeChecked();
    await expect(checklist.nth(1)).toBeEnabled();
    await page.getByLabel('Result', { exact: false }).fill('Queued while a previous update was sending.');
    await page.getByRole('button', { name: 'Save result', exact: true }).click();
    await expect(page.getByRole('button', { name: 'Mark done' })).toBeEnabled();
    await page.getByRole('button', { name: 'Mark done' }).click();
    await expect(page.getByText('Saved on this device', { exact: true })).toHaveCount(3);
    release();
    await expect(page.getByText('Confirmed by server ✓', { exact: true })).toHaveCount(4);
    const after = await (await context.request.get(`/api/field/tasks/${id}`)).json();
    expect(after.status).toBe('done');
    expect(after.updates.length - before.updates.length).toBe(4);
  } finally { release(); }
});
