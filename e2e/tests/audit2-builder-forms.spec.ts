import { resolve } from 'node:path';
import type { Page } from '@playwright/test';
import { test, expect, login, advance, ready } from '../helpers';

const PDF = resolve(import.meta.dirname, '../../server/seed-data/docs/site-plan.pdf');

async function call(page: Page, method: string, path: string, data?: unknown) {
  const me = await (await page.request.get('/api/me')).json();
  const response = await page.request.fetch(path, { method, data, headers: { 'X-CSRF-Token': me.csrf_token } });
  expect(response.ok(), `${method} ${path}: ${await response.text()}`).toBeTruthy();
  return response.json();
}

test('N-04: a Builder-made service with a field task and a service-response letter runs end to end', async ({ page, browser, baseURL }) => {
  test.setTimeout(300000);
  const resident = await browser.newContext({ baseURL });
  const staff = await browser.newContext({ baseURL });
  const field = await browser.newContext({ baseURL });
  try {
    await login(page, 'Mark');
    await page.goto('/admin/services');
    await page.getByRole('link', { name: 'Create service', exact: true }).click();
    await page.getByLabel('Name', { exact: false }).first().fill('Fictional verge mowing request');
    await page.getByLabel('Slug', { exact: false }).fill('audit2-verge-mowing');
    await page.getByRole('button', { name: 'Create blank draft' }).click();
    await expect(page).toHaveURL(/\/admin\/services\/\d+/);
    await page.getByLabel('Summary', { exact: false }).fill('Ask Council to mow a fictional road verge.');
    await page.getByLabel('What the applicant receives').fill('A site visit and a written response.');
    await page.getByLabel('Who can apply').fill('Norfolk Island residents.');
    await page.getByLabel('Price calculation', { exact: true }).fill('No fee for this fictional demonstration.');
    await page.getByRole('tab', { name: 'Fields', exact: true }).click();
    await page.getByRole('button', { name: 'Add field', exact: true }).click();
    await page.getByLabel('Key', { exact: false }).fill('verge');
    await page.getByLabel('Label', { exact: false }).first().fill('Verge location');
    await page.getByRole('checkbox', { name: 'Required while shown' }).check();

    await page.getByRole('tab', { name: 'Workflow steps', exact: true }).click();
    // Field task step, moved before the terminal step.
    await page.getByRole('button', { name: 'Add workflow step', exact: true }).click();
    await page.getByLabel('Kind', { exact: true }).nth(2).selectOption('task');
    await page.getByLabel('Task kind').selectOption('general');
    await page.getByLabel('Staff label').nth(2).fill('Mow verge');
    await page.getByLabel('Applicant label').nth(2).fill('Council is visiting the verge');
    await page.getByRole('button', { name: 'Move item 3 up' }).click();
    // Service-response letter step (the handler the audit found broken), also before the terminal step.
    await page.getByRole('button', { name: 'Add workflow step', exact: true }).click();
    await page.getByLabel('Kind', { exact: true }).nth(3).selectOption('module');
    await page.getByLabel('Module handler').selectOption('documents.letter_issued:service_response');
    await page.getByLabel('Staff label').nth(3).fill('Write response');
    await page.getByLabel('Applicant label').nth(3).fill('Preparing your response');
    await page.getByRole('button', { name: 'Move item 4 up' }).click();
    await page.getByRole('button', { name: 'Validate and publish' }).click();
    await expect(page.getByText('Immutable version', { exact: true })).toBeVisible();

    // A second field worker so staff can change the assignee.
    const worker = await call(page, 'POST', '/api/admin/users', { email: 'verge.worker@example.invalid', display_name: 'Fictional Verge Worker', kind: 'staff', password: 'fictional-verge-password', job_title: 'Field worker' });
    await call(page, 'POST', `/api/admin/users/${worker.id}/roles`, { role: 'field_worker' });

    const alexey = await resident.newPage();
    await login(alexey, 'Alexey');
    await alexey.goto('/services');
    await alexey.getByRole('link', { name: /Fictional verge mowing request/ }).click();
    await alexey.getByRole('button', { name: /Start request/ }).click();
    await alexey.getByLabel('Verge location').fill('Fictional verge outside 12 Taylors Road');
    await alexey.getByRole('button', { name: 'Review before submitting' }).click();
    await alexey.getByRole('button', { name: 'Submit request', exact: true }).click();
    await expect(alexey).toHaveURL(/\/my\/cases\/\d+/);
    const caseId = new URL(alexey.url()).pathname.split('/').pop();

    const olga = await staff.newPage();
    await login(olga, 'Olga');
    await olga.goto(`/staff/cases/${caseId}`);
    await ready(olga);
    await advance(olga);
    await expect(olga.getByText('The field task for this step is not finished yet.')).toBeVisible();
    await olga.getByRole('tab', { name: 'Tasks', exact: true }).click();
    const panel = olga.getByRole('tabpanel');
    await expect(panel).toContainText('Council field task');
    await panel.getByLabel('Assign field worker').selectOption({ label: 'Fictional Verge Worker' });
    await panel.getByRole('button', { name: 'Assign', exact: true }).click();
    await expect.poll(async () => (await call(olga, 'GET', `/api/cases/${caseId}/tasks`))[0].assigned_to).toBe(worker.id);
    await olga.reload();
    await olga.getByRole('tab', { name: 'Timeline', exact: true }).click();
    await expect(olga.getByRole('tabpanel')).toContainText('Field task reassigned');
    await olga.getByRole('tab', { name: 'Tasks', exact: true }).click();
    await olga.getByRole('tabpanel').getByLabel('Assign field worker').selectOption({ label: 'Jake Rowe' });
    await olga.getByRole('tabpanel').getByRole('button', { name: 'Assign', exact: true }).click();
    await expect.poll(async () => (await call(olga, 'GET', `/api/cases/${caseId}/tasks`))[0].assigned_to).not.toBe(worker.id);
    const taskId = (await call(olga, 'GET', `/api/cases/${caseId}/tasks`))[0].id;

    // Jake sees it in Field tasks and completes it.
    const jake = await field.newPage();
    await login(jake, 'Jake');
    await jake.goto('/staff/field');
    await expect(jake.locator(`a[href="/staff/field/${taskId}"]`).first()).toBeVisible();
    await jake.goto(`/staff/field/${taskId}`);
    await expect(jake.getByRole('heading', { name: 'Checklist', exact: true })).toBeVisible();
    for (const item of await jake.locator('section').filter({ has: jake.getByRole('heading', { name: 'Checklist', exact: true }) }).getByRole('checkbox').all()) {
      if (!(await item.isChecked())) await item.click();
      await expect(item).toBeChecked();
      await expect(item).toBeEnabled();
    }
    await jake.getByLabel('Result', { exact: false }).fill('Verge mowed and clippings removed.');
    await jake.getByRole('button', { name: 'Save result', exact: true }).click();
    await expect(jake.getByRole('button', { name: 'Mark done', exact: true })).toBeEnabled();
    await jake.getByRole('button', { name: 'Mark done', exact: true }).click();
    await expect.poll(async () => (await (await jake.request.get(`/api/field/tasks/${taskId}`)).json()).status, { timeout: 45000 }).toBe('done');

    // Letter step: blocked until the response is issued from the Response letters tab.
    await olga.reload();
    await expect(olga.getByText('Issue the service response letter before continuing.')).toBeVisible();
    await olga.getByRole('button', { name: 'Complete this step', exact: true }).click();
    await olga.getByRole('dialog').getByLabel('Reason or completion note').fill('Trying to skip the response.');
    await olga.getByRole('button', { name: 'Confirm action', exact: true }).click();
    await expect(olga.getByRole('dialog')).toContainText('Issue the service response letter before continuing.');
    await olga.getByRole('button', { name: 'Back', exact: true }).click();
    await olga.getByRole('tab', { name: 'Response letters', exact: true }).click();
    await olga.getByLabel('Response for the applicant').fill('The fictional verge was mowed on Tuesday.');
    await olga.getByRole('button', { name: 'Issue service response letter' }).click();
    await expect(olga.getByRole('tabpanel')).toContainText('Issued');
    await expect.poll(async () => (await call(olga, 'GET', `/api/cases/${caseId}`)).case.status).toBe('completed');

    await alexey.reload();
    await alexey.getByRole('tab', { name: 'Documents', exact: true }).click();
    await expect(alexey.getByRole('tabpanel')).toContainText('Service response');
  } finally {
    await resident.close(); await staff.close(); await field.close();
  }
});

test('N-06: modify-approval captures several applicants, landowners, parcels and modification types', async ({ page, browser, baseURL }) => {
  test.setTimeout(240000);
  const staff = await browser.newContext({ baseURL });
  try {
    await login(page, 'Ben');
    const approval = (await call(page, 'GET', '/api/my/issued-approvals'))[0];
    const org = (await call(page, 'GET', '/api/my/organisations'))[0].id;
    // The approval picker belongs to the documents feature; preset it so this test exercises the form fields.
    const draft = await call(page, 'POST', '/api/services/modify-approval/drafts', { applicant_org_id: org });
    await call(page, 'PUT', `/api/cases/${draft.id}/draft`, { answers: { original_approval: { decision_id: approval.id } } });
    await page.goto(`/my/drafts/${draft.id}`);
    await ready(page);

    const person = async (group: string, row: number, first: string, last: string, email: string) => {
      const scope = page.getByRole('group', { name: `${group}: row ${row}`, exact: true });
      await scope.getByLabel('First name').fill(first);
      await scope.getByLabel('Last name').fill(last);
      await scope.getByLabel('Postal address').fill('PO Box 95, Norfolk Island 2899');
      await scope.getByLabel('Mobile').fill('+672 5 12345');
      await scope.getByLabel('Email').fill(email);
      return scope;
    };
    await page.getByRole('button', { name: 'Add applicants row' }).click();
    await page.getByRole('button', { name: 'Add applicants row' }).click();
    await person('Applicants', 1, 'Ben', 'Carter', 'ben@example.invalid');
    await person('Applicants', 2, 'Bea', 'Carter', 'bea@example.invalid');
    await page.getByLabel('Are all landowners listed above as applicants?').selectOption('no');
    await page.getByRole('button', { name: 'Add landowners row' }).click();
    await page.getByRole('button', { name: 'Add landowners row' }).click();
    for (const [row, first] of [[1, 'Olive'], [2, 'Oscar']] as const) {
      const scope = await person('Landowners', row, first, 'Owner', `${first.toLowerCase()}@example.invalid`);
      await scope.getByRole('checkbox', { name: /This landowner consents/ }).check();
    }
    await page.getByLabel('Property address').fill('44 Taylors Road, Burnt Pine');
    for (const [row, portion] of [[1, '44h'], [2, '44j'], [3, '45']] as const) {
      await page.getByRole('button', { name: 'Add land parcels row' }).click();
      const scope = page.getByRole('group', { name: `Land parcels: row ${row}`, exact: true });
      await scope.getByLabel('Portion number').fill(portion);
      await scope.getByLabel('Lot number').fill(String(row));
      await scope.getByLabel('Section number').fill('9');
      await scope.getByLabel(/Land area/).fill(`${row},000 m²`);
    }
    await page.getByLabel('Land tenure').selectOption('Freehold');
    await page.getByLabel('Zoning').selectOption('Rural');
    await page.getByLabel('What is the land currently used for?').fill('Dwelling house');
    await page.getByRole('checkbox', { name: 'Alterations and additions to existing structure(s)' }).check();
    await page.getByRole('checkbox', { name: 'Modification to condition(s)' }).check();
    await page.getByRole('checkbox', { name: 'Change of approval lapse date' }).check();
    await page.getByLabel('Condition(s): describe the modification and its expected impact').fill('Condition 4: reduce the water tank to 15,000 L.');
    await page.getByLabel('Proposed approval lapse date').fill('2027-06-30');
    await page.getByLabel('Reasons for requiring the change of lapse date').fill('Builder availability.');
    await page.getByLabel(/The proposed modified use or development/).fill('Same dwelling with a smaller tank.');
    await page.getByLabel('Changes in the external environment since the original approval').fill('None.');
    await page.getByLabel('Total estimated cost of building and works (AUD)').fill('45000');
    await page.getByRole('checkbox', { name: 'Trees Act 1997 (NI)' }).check();
    await page.getByRole('checkbox', { name: /declare that the information in this application is correct/ }).check();
    for (const label of ['Copy of title search', 'Signed consent of all landowners', 'Description of expected impacts, with relevant plans and drawings']) {
      await page.getByLabel(label, { exact: false }).first().setInputFiles(PDF);
      await expect(page.getByText('Document attached.', { exact: true })).toBeVisible();
    }
    await page.getByRole('button', { name: 'Review before submitting' }).click();
    await page.getByRole('button', { name: 'Submit request', exact: true }).click();
    await expect(page).toHaveURL(/\/my\/cases\/\d+/);
    const caseId = new URL(page.url()).pathname.split('/').pop();

    const priya = await staff.newPage();
    await login(priya, 'Priya');
    await priya.goto(`/staff/cases/${caseId}`);
    await ready(priya);
    const overview = priya.getByRole('tabpanel');
    for (const text of ['Bea', 'bea@example.invalid', 'Oscar', '44j', '3,000 m²', 'Modification to condition(s)', 'Change of approval lapse date', 'Builder availability.', 'Trees Act 1997 (NI)']) {
      await expect(overview).toContainText(text);
    }
    await priya.getByRole('tab', { name: 'Documents', exact: true }).click();
    const documents = priya.getByRole('tabpanel');
    await expect(documents).toContainText('Required and optional documents');
    await expect(documents).toContainText('every landowner signs to consent');
    await expect(documents).toContainText('Signed consent of all landowners');
  } finally { await staff.close(); }
});
