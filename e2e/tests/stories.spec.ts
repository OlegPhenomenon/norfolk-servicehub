import { resolve } from 'node:path';
import { test, expect, login, advance, startHall, openRoadTask } from '../helpers';

test('a: Alexey books Rawson Hall via hosted DemoPay and Olga confirms', async ({ page, browser, baseURL }) => {
  const staffContext = await browser.newContext({ baseURL });
  try {
    const olga = await staffContext.newPage();
    await login(page, 'Alexey');
    await startHall(page);
    await page.getByRole('checkbox', { name: 'I declare that the information is correct' }).check();
    await page.getByLabel('Public liability policy ($20 million) or Council casual hirer agreement', { exact: false })
      .setInputFiles(resolve(import.meta.dirname, '../../server/seed-data/docs/site-plan.pdf'));
    await expect(page.getByText('Document attached.', { exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Review before submitting' }).click();
    await page.getByRole('button', { name: 'Submit request', exact: true }).click();
    await expect(page).toHaveURL(/\/my\/cases\/\d+/);
    await expect(page.getByText('Your booking is not confirmed yet.', { exact: false })).toBeVisible();
    const caseId = new URL(page.url()).pathname.split('/').pop();
    const reference = (await page.locator('main').innerText()).match(/NSH-\d+-\d+/)![0];
    await login(olga, 'Olga');
    await olga.goto('/staff/cases?queue=new');
    await olga.getByRole('link', { name: reference, exact: true }).click();
    await advance(olga);
    await page.reload();
    await page.getByRole('tab', { name: 'Money', exact: true }).click();
    await page.getByRole('button', { name: /^Pay / }).click();
    await expect(page).toHaveURL(/\/mock\/pay\/checkout\//);
    await expect(page.getByText('TEST MODE', { exact: false }).first()).toBeVisible();
    await page.getByRole('button', { name: 'Pay with test card', exact: true }).click();
    await expect(page).toHaveURL(new RegExp(`/my/cases/${caseId}.*paid=1`));
    // Provider money appears only after the signed webhook; the checkout return cannot settle it.
    await expect(page.getByText('Payment confirmed', { exact: true })).toBeVisible({ timeout: 45000 });
    await expect(page.getByRole('table', { name: 'Confirmed payments', exact: true })).toContainText('DemoPay');
    await expect(page.getByRole('heading', { level: 1 }).locator('..')).toContainText('Confirming your booking');
    await olga.reload();
    await olga.getByRole('tab', { name: 'Booking', exact: true }).click();
    await olga.getByRole('button', { name: 'Confirm booking', exact: true }).click();
    await expect(olga.getByRole('link', { name: 'Download booking confirmation PDF' })).toBeVisible();
    await page.reload();
    await page.getByRole('tab', { name: 'Booking', exact: true }).click();
    await expect(page.getByText('Booking confirmed', { exact: false }).first()).toBeVisible();
    const download = page.waitForEvent('download');
    await page.getByRole('link', { name: 'Download booking confirmation PDF' }).click();
    expect((await download).suggestedFilename()).toMatch(/\.pdf$/);
  } finally { await staffContext.close(); }
});

test('b: Olga records phone intake without an applicant account', async ({ page }) => {
  await login(page, 'Olga');
  await page.getByRole('link', { name: 'Assisted intake', exact: true }).first().click();
  await expect(page).toHaveURL(/\/staff\/intake$/);
  await expect(page.getByRole('heading', { name: 'Assisted intake', exact: true })).toBeVisible();
  await page.getByLabel('Service', { exact: false }).selectOption('road-issue');
  await page.getByLabel('Received by').selectOption('phone');
  await page.getByLabel('Applicant name').fill('Fictional phone caller');
  await page.getByLabel('Applicant email').fill('phone-caller@example.test');
  await page.getByLabel('Applicant phone').fill('+672 3 55555');
  await page.getByLabel('Name of applicant').fill('Fictional phone caller');
  await page.getByLabel('Postal address').fill('20 Fictional Road');
  await page.getByLabel('Latitude').fill('-29.04');
  await page.getByLabel('Longitude').fill('167.95');
  await page.getByLabel('Location description').fill('At the fictional depot entrance');
  await page.getByLabel('Describe the pothole, culvert or other issue').fill('A pothole reported over the phone.');
  await page.getByRole('checkbox', { name: 'I declare that the information is correct' }).check();
  await page.getByRole('button', { name: 'Review request', exact: true }).click();
  await page.getByRole('button', { name: 'Submit assisted request' }).click();
  await expect(page.getByText(/Request NSH-.* received/)).toBeVisible();
  await page.getByRole('link', { name: 'Open request workspace' }).click();
  await expect(page.locator('main')).toContainText('Fictional phone caller · phone');
  await expect(page.getByRole('tabpanel')).toContainText('A pothole reported over the phone.');
  await advance(page);
  await expect(page.getByRole('tab', { name: 'Tasks', exact: true })).toBeVisible();
  await page.getByRole('tab', { name: 'Tasks', exact: true }).click();
  await expect(page.getByRole('tabpanel')).toContainText('Inspect');
});

test('c: Jake completes a field task offline and synchronizes once online', async ({ page, context }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await login(page, 'Jake');
  await page.goto('/staff/field');
  // Road inspection needs no equipment usage card and exists in a fresh seed.
  await openRoadTask(page);
  const taskId = new URL(page.url()).pathname.split('/').pop();
  const before = await (await context.request.get(`/api/field/tasks/${taskId}`)).json();
  expect(before.status).toBe('open');
  await context.setOffline(true);
  const checklist = page.locator('section').filter({ has: page.getByRole('heading', { name: 'Checklist', exact: true }) }).getByRole('checkbox');
  for (const item of await checklist.all()) {
    if (!(await item.isChecked())) await item.click();
    await expect(item).toBeChecked();
    await expect(item).toBeEnabled();
  }
  await page.getByLabel('Result', { exact: false }).fill('Inspected offline: pothole measured and repair area marked.');
  await page.getByRole('button', { name: 'Save result', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Mark done', exact: true })).toBeEnabled();
  await page.getByRole('button', { name: 'Mark done', exact: true }).click();
  await expect(page.getByText('Saved on this device', { exact: true })).toHaveCount(before.checklist.filter((c: { done: boolean }) => !c.done).length + 2);
  // The API request client remains online, so it can verify nothing was written by the offline browser.
  const during = await (await context.request.get(`/api/field/tasks/${taskId}`)).json();
  expect(during).toEqual(before);
  await context.setOffline(false);
  await expect(page.getByText('Confirmed by server ✓', { exact: true })).toHaveCount(before.checklist.filter((c: { done: boolean }) => !c.done).length + 2, { timeout: 45000 });
  const after = await (await context.request.get(`/api/field/tasks/${taskId}`)).json();
  expect(after.status).toBe('done');
  expect(after.result_text).toBe('Inspected offline: pothole measured and repair area marked.');
  expect(after.updates.length - before.updates.length).toBe(before.checklist.filter((c: { done: boolean }) => !c.done).length + 2);
  await page.reload();
  await expect(page.getByText('Inspected offline: pothole measured and repair area marked.', { exact: true }).first()).toBeVisible();
});

test('d: Mark builds and publishes a service, a new resident submits it', async ({ page, browser, baseURL }) => {
  const residentContext = await browser.newContext({ baseURL });
  try {
    await login(page, 'Mark');
    await page.goto('/admin/services');
    await page.getByRole('link', { name: 'Create service', exact: true }).click();
    await page.getByLabel('Name', { exact: false }).first().fill('Fictional community garden request');
    await page.getByLabel('Slug', { exact: false }).fill('browser-community-garden');
    await page.getByRole('button', { name: 'Create blank draft' }).click();
    await expect(page).toHaveURL(/\/admin\/services\/\d+/);
    await page.getByLabel('Summary', { exact: false }).fill('Request access to the fictional community garden.');
    await page.getByLabel('What the applicant receives').fill('A written garden access response.');
    await page.getByLabel('Who can apply').fill('Norfolk Island residents.');
    await page.getByLabel('Price calculation', { exact: true }).fill('No fee for this fictional demonstration.');
    await page.getByRole('tab', { name: 'Fields', exact: true }).click();
    await page.getByRole('button', { name: 'Add field', exact: true }).click();
    await page.getByLabel('Key', { exact: false }).fill('purpose');
    await page.getByLabel('Label', { exact: false }).first().fill('Garden purpose');
    await page.getByRole('checkbox', { name: 'Required while shown' }).check();
    await page.getByRole('tab', { name: 'Workflow steps', exact: true }).click();
    await expect(page.getByLabel('Staff label').first()).toBeVisible();
    const firstKind = page.getByLabel('Kind', { exact: false }).first();
    await firstKind.selectOption('module');
    const handlers = page.getByLabel('Module handler');
    await expect(handlers.locator('option')).toHaveCount(2); // placeholder and generic service-response handler
    await expect(handlers.locator('option').last()).toHaveAttribute('value', 'documents.letter_issued:service_response');
    await firstKind.selectOption('decision');
    await expect(page.getByLabel('service response', { exact: true })).toBeVisible();
    await expect(page.getByLabel('building approval', { exact: true })).toHaveCount(0);
    await firstKind.selectOption('review');

    await page.getByRole('tab', { name: 'Preview', exact: true }).click();
    await page.getByLabel('Garden purpose').fill('Grow vegetables with neighbours.');
    await page.getByRole('button', { name: 'Check sample answers' }).click();
    await expect(page.getByText('The sample answers passed validation. Nothing was submitted.')).toBeVisible();
    await page.getByRole('button', { name: 'Validate and publish' }).click();
    await expect(page.getByText('Immutable version', { exact: true })).toBeVisible();
    const resident = await residentContext.newPage();
    await resident.goto('/register');
    await resident.getByLabel('Full name').fill('Fictional Garden Resident');
    await resident.getByLabel('Email address').fill('garden-browser@example.test');
    await resident.getByLabel('Password').fill('fictional-garden-password');
    await resident.getByRole('button', { name: 'Create account', exact: true }).click();
    await expect(resident).toHaveURL(/\/my/);
    await resident.goto('/services');
    await resident.getByRole('link', { name: /Fictional community garden request/ }).click();
    await resident.getByRole('button', { name: /Start request/ }).click();
    await resident.getByLabel('Garden purpose').fill('Grow vegetables with neighbours.');
    await resident.getByRole('button', { name: 'Review before submitting' }).click();
    await resident.getByRole('button', { name: 'Submit request', exact: true }).click();
    await expect(resident).toHaveURL(/\/my\/cases\/\d+/);
    await expect(resident.getByText('Your request has been received', { exact: true })).toBeVisible();
    await expect(resident.getByRole('tabpanel')).toContainText('Grow vegetables with neighbours.');
  } finally { await residentContext.close(); }
});
