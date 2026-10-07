import { test, expect, login, ready } from '../helpers';

const footnote = 'Prices are a demo copy of the FY2026-27 schedule; time targets are illustrative';

test('catalogue is concise and service conditions stay available', async ({ page }) => {
  await page.goto('/services');
  await ready(page);
  const cards = page.locator('main section').filter({ has: page.getByRole('link', { name: 'View service', exact: true }) });
  await expect(cards).toHaveCount(11);
  for (const card of await cards.all()) {
    const paragraphs = card.locator('p');
    await expect(paragraphs).toHaveCount(2);
    expect((await paragraphs.first().innerText()).length).toBeLessThanOrEqual(140);
    await expect(card).not.toContainText('time targets');
  }
  await expect(page.getByText(footnote, { exact: true })).toHaveCount(1);
  await page.goto('/services/rawson-hall-hire');
  await ready(page);
  await expect(page.getByRole('heading', { level: 1 }).locator('..').locator('..')).toContainText('Hire the Main Hall, Supper Room or both for an event.');
  const conditions = page.locator('section').filter({ has: page.getByRole('heading', { name: 'Conditions', exact: true }) });
  await expect(conditions).toContainText('Music stops by 10 pm');
  await expect(conditions).toContainText('$20 million');
  await expect(page.getByText(footnote, { exact: true })).toHaveCount(1);
});

test('workspace navigation, staff wording and safe action hierarchy', async ({ page }) => {
  await login(page, 'Alexey');
  const account = page.getByRole('navigation', { name: 'Your account' });
  await expect(account.getByRole('link', { name: 'Overview', exact: true })).toHaveCount(0);
  await expect(account.getByRole('link', { name: 'My requests', exact: true })).toHaveCount(1);
  await expect(page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'My requests', exact: true })).toHaveCount(0);
  await expect(page.getByText('Nothing needs your action', { exact: true })).toBeVisible();
  await login(page, 'Olga');
  await expect(page.locator('aside a[href="/staff"]')).toHaveCount(1);
  await expect(page.locator('aside a[href="/staff"]')).toHaveText('Home');
  await page.goto('/staff/cases?service=rawson-hall-hire&status=in_progress');
  await page.locator('main table a').first().click();
  await ready(page);
  const progress = page.getByRole('list', { name: 'Progress', exact: true });
  await expect(progress).toContainText('Check request');
  await expect(progress).toContainText('Prepare hall');
  await expect(progress).not.toContainText('Preparing the hall');
  await expect(page.locator('main')).not.toContainText('Before you submit');
  await expect(page.locator('main')).not.toContainText('rawson-main');
  const cancel = page.getByRole('button', { name: 'Cancel request', exact: true });
  await expect(cancel).toHaveClass(/text-danger/);
  const actionLabels = await cancel.locator('..').getByRole('button').allTextContents();
  expect(actionLabels.at(-1)).toBe('Cancel request');
  await cancel.click();
  const dialog = page.getByRole('dialog');
  await expect(dialog).toHaveAccessibleName('Cancel request?');
  await expect(dialog.getByRole('button', { name: 'Cancel request', exact: true })).toBeDisabled();
  await dialog.getByLabel('Reason', { exact: true }).fill('A reason to review, without changing the seeded case.');
  await expect(dialog.getByRole('button', { name: 'Cancel request', exact: true })).toBeEnabled();
  await dialog.getByRole('button', { name: 'Back', exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await login(page, 'Mark');
  await page.goto('/admin');
  await ready(page);
  await expect(page.locator('aside a[href="/admin"]')).toHaveCount(1);
});
