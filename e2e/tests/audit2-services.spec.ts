import { resolve } from 'node:path';
import type { Browser, Page } from '@playwright/test';
import { test, expect, login, advance, futureDate, ready } from '../helpers';

const pdf = resolve(import.meta.dirname, '../../server/seed-data/docs/site-plan.pdf');

async function persona(browser: Browser, baseURL: string | undefined, name: string) {
  const context = await browser.newContext({ baseURL });
  const page = await context.newPage();
  await login(page, name);
  return page;
}
async function caseStatus(page: Page, id: string) {
  return (await (await page.request.get(`/api/cases/${id}`)).json()).case;
}
async function prepareAndIssue(priya: Page, helen: Page, id: string) {
  await priya.goto(`/staff/cases/${id}?tab=documents.decisions`);
  await ready(priya);
  await priya.getByText('Prepare a decision', { exact: true }).click();
  await priya.getByLabel('Decision type').selectOption('service_response');
  await priya.getByLabel('Versioned template').selectOption({ index: 1 });
  await priya.getByLabel('Reasons / certificate information').fill('Fictional written decision prepared in the browser.');
  await priya.getByRole('button', { name: 'Save draft', exact: true }).click();
  await priya.getByRole('button', { name: 'Submit for approval', exact: true }).click();
  await expect(priya.getByText('pending approval', { exact: false }).first()).toBeVisible();
  await helen.goto(`/staff/cases/${id}?tab=documents.decisions`);
  await ready(helen);
  await helen.getByRole('button', { name: 'Issue decision and PDF', exact: true }).click();
  await expect(helen.getByRole('link', { name: 'Download issued decision' })).toBeVisible();
}

test('Stage B notice: returned declaration, new version accepted and shown in the project history', async ({ page, browser, baseURL }) => {
  const staff: Page[] = [];
  try {
    await login(page, 'Ben');
    const projects = await (await page.request.get('/api/my/building-projects')).json();
    const project = projects.find((p: { approvals: unknown[] }) => p.approvals.length > 0);
    expect(project, 'The seeded Island Builders project is listed for Ben').toBeTruthy();
    await page.goto('/services/builder-stage-b-notice');
    await expect(page.getByRole('heading', { level: 1 })).toContainText("Builder's Stage B Compliance Declaration Notice");
    await page.getByRole('button', { name: /Start request/ }).click();
    await expect(page).toHaveURL(/\/my\/drafts\/\d+/);
    await page.getByLabel('Your building projects').selectOption(project.reference);
    await page.getByLabel('Person who carried out the building work — first name').fill('Ben');
    await page.getByLabel('Person who carried out the building work — last name').fill('Carter');
    await page.getByLabel('Phone number').fill('+672 3 55501');
    await page.getByLabel('Email', { exact: true }).fill('ben@example.invalid');
    await page.getByLabel('Portion number').fill('Portion DEMO-44');
    await page.getByLabel('Property address').fill('44 Fictional Taylors Road');
    await page.getByRole('checkbox', { name: /I have completed the building work described for Inspection Stage B/ }).check();
    await page.getByRole('checkbox', { name: /Stage B building work specified in Schedule 3/ }).check();
    await page.getByRole('checkbox', { name: /in accordance with the building approval/ }).check();
    await page.getByLabel('Date of the compliance declaration').fill(futureDate(0));
    await page.getByLabel(/Specified Stage B work/).selectOption('yes');
    await page.getByLabel("Signed Builder's Stage B compliance declaration notice", { exact: false }).setInputFiles(pdf);
    await expect(page.getByText(/Attached:/).first()).toBeVisible();
    await page.getByRole('button', { name: 'Review before submitting' }).click();
    await page.getByRole('button', { name: 'Submit request', exact: true }).click();
    await expect(page).toHaveURL(/\/my\/cases\/\d+/);
    const id = new URL(page.url()).pathname.split('/').pop()!;

    // Olga returns the declaration for a new version; the case waits for Ben.
    const olga = await persona(browser, baseURL, 'Olga'); staff.push(olga);
    await olga.goto(`/staff/cases/${id}?tab=documents.documents`);
    await ready(olga);
    await olga.getByText('Comment or ask for a new version', { exact: true }).first().click();
    await olga.getByLabel('Comment', { exact: true }).fill('Browser check: the builder signature is missing. Please upload the signed notice.');
    await olga.getByRole('checkbox', { name: /Ask for a new version/ }).check();
    await olga.getByRole('button', { name: 'Ask for a new version', exact: true }).click();
    await expect(olga.getByText('New version requested', { exact: true })).toBeVisible();
    expect((await caseStatus(olga, id)).status).toBe('waiting_on_applicant');
    await olga.reload();
    await expect(olga.getByRole('button', { name: 'Complete this step', exact: true })).toHaveCount(0);

    // Ben uploads version 2, resolving the request.
    await page.goto(`/my/cases/${id}?tab=documents.documents`);
    await ready(page);
    const replace = page.locator('form').filter({ hasText: 'This replaces' }).first();
    await replace.getByLabel('File', { exact: true }).setInputFiles(pdf);
    await replace.getByLabel('What changed?').fill('Signed declaration');
    await replace.getByRole('checkbox', { name: /builder signature is missing/ }).check();
    await replace.getByRole('button', { name: 'Upload replacement version', exact: true }).click();
    await expect(page.getByText('Replacement received', { exact: true })).toBeVisible();
    await expect(page.getByText('Version 2', { exact: true })).toBeVisible();

    // Intake accepts the corrected file; the $83 inspection fee is paid through DemoPay.
    await olga.reload();
    await advance(olga);
    await page.goto(`/my/cases/${id}`);
    await page.getByRole('tab', { name: 'Money', exact: true }).click();
    await expect(page.getByText('$83.00').first()).toBeVisible();
    await page.getByRole('button', { name: /^Pay / }).click();
    await page.getByRole('button', { name: 'Pay with test card', exact: true }).click();
    await expect(page.getByText('Payment confirmed', { exact: true })).toBeVisible({ timeout: 45000 });
    await expect.poll(async () => (await caseStatus(olga, id)).current_step, { timeout: 45000 }).toBe('site');

    // Jake completes the required site inspection.
    const tasks = await (await olga.request.get(`/api/cases/${id}/tasks`)).json();
    const task = tasks.find((t: { kind: string }) => t.kind === 'site_inspection');
    const jake = await persona(browser, baseURL, 'Jake'); staff.push(jake);
    await jake.goto(`/staff/field/${task.id}`);
    await expect(jake.getByRole('heading', { name: 'Checklist', exact: true })).toBeVisible();
    const checklist = jake.locator('section').filter({ has: jake.getByRole('heading', { name: 'Checklist', exact: true }) }).getByRole('checkbox');
    for (const item of await checklist.all()) {
      if (!(await item.isChecked())) await item.click();
      await expect(item).toBeChecked();
    }
    await jake.getByLabel('Result', { exact: false }).fill('Stage B framework inspected; work may continue.');
    await jake.getByRole('button', { name: 'Save result', exact: true }).click();
    await jake.getByRole('button', { name: 'Mark done', exact: true }).click();
    await expect.poll(async () => (await caseStatus(olga, id)).current_step, { timeout: 45000 }).toBe('decision');

    // Priya prepares the written permission on version 2; Helen issues it.
    const priya = await persona(browser, baseURL, 'Priya'); staff.push(priya);
    const helen = await persona(browser, baseURL, 'Helen'); staff.push(helen);
    await prepareAndIssue(priya, helen, id);
    await expect.poll(async () => (await caseStatus(olga, id)).status, { timeout: 45000 }).toBe('completed');

    // The project history shows the returned version, the accepted version 2 and the decision.
    await page.goto(`/my/cases/${id}?tab=documents.decisions`);
    await page.getByRole('link', { name: 'View building project and approval history' }).click();
    await expect(page).toHaveURL(/\/my\/projects\/\d+/);
    await ready(page);
    await expect(page.getByText(/Returned for a new version: Browser check: the builder signature is missing/)).toBeVisible();
    await expect(page.getByText(/Decision based on .* v2/).first()).toBeVisible();
    const history = page.getByRole('heading', { name: 'Project history', exact: true }).locator('xpath=ancestor::section[1]');
    await expect(history).toContainText('version 2');
    await expect(page.getByText("Builder's Stage B Compliance Declaration Notice").first()).toBeVisible();
  } finally { for (const p of staff) await p.context().close(); }
});

test('Form 212 pipeline crossing goes from application to an issued decision', async ({ page, browser, baseURL }) => {
  const staff: Page[] = [];
  try {
    await login(page, 'Ben');
    await page.goto('/services');
    await expect(page.getByText('Application to Install Pipeline or Conduit Crossing in Public Roadway').first()).toBeVisible();
    await page.goto('/services/pipeline-conduit-crossing');
    await page.getByRole('button', { name: /Start request/ }).click();
    await expect(page).toHaveURL(/\/my\/drafts\/\d+/);
    await page.getByLabel('Name of applicant').fill('Ben Carter');
    await page.getByLabel('Postal address').fill('44 Fictional Taylors Road');
    await page.getByLabel('Email address').fill('ben@example.invalid');
    await page.getByLabel('Phone (work or mobile)').fill('+672 3 55501');
    await page.getByLabel('ABN / ACN number').fill('00 000 000 000');
    await page.getByLabel('Position held by the person signing').fill('Director');
    await page.getByLabel('Name of road that the pipeline or conduit will be installed in').fill('Taylors Road');
    await page.getByLabel(/Location of proposed pipeline or conduit crossing/).fill('Portion DEMO-44');
    await page.getByLabel('Size and type of proposed pipe or conduit').fill('Fictional 50 mm PE water main in a 100 mm conduit');
    await page.getByLabel('Description of land adjacent to the road reserve (Portion numbers)').fill('Portion DEMO-44 and DEMO-45');
    await page.getByRole('checkbox', { name: /no work is to be carried out/ }).check();
    await page.getByRole('checkbox', { name: /costs of restoring the road pavement/ }).check();
    await page.getByRole('checkbox', { name: 'I declare that the information is correct' }).check();
    await page.getByLabel('Sketch or drawing showing location, dimensions and levels', { exact: false }).setInputFiles(pdf);
    await expect(page.getByText(/Attached:/).first()).toBeVisible();
    await page.getByRole('button', { name: 'Review before submitting' }).click();
    await page.getByRole('button', { name: 'Submit request', exact: true }).click();
    await expect(page).toHaveURL(/\/my\/cases\/\d+/);
    const id = new URL(page.url()).pathname.split('/').pop()!;

    const olga = await persona(browser, baseURL, 'Olga'); staff.push(olga);
    await olga.goto(`/staff/cases/${id}`);
    await ready(olga);
    await advance(olga);
    await advance(olga);
    expect((await caseStatus(olga, id)).current_step).toBe('decision');
    const priya = await persona(browser, baseURL, 'Priya'); staff.push(priya);
    const helen = await persona(browser, baseURL, 'Helen'); staff.push(helen);
    await prepareAndIssue(priya, helen, id);
    await expect.poll(async () => (await caseStatus(page, id)).status, { timeout: 45000 }).toBe('completed');
    await page.goto(`/my/cases/${id}?tab=documents.decisions`);
    await expect(page.getByRole('link', { name: 'Download issued decision' })).toBeVisible();
  } finally { for (const p of staff) await p.context().close(); }
});
