import { mkdirSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Locator, Page } from '@playwright/test';
import { test as base, expect, login, advance, ready, futureDate } from '../helpers';

// Evidence screenshots for the audit-2 fixes N-01..N-06, taken from the real UI of a fresh seeded demo.
// Run with: npx playwright test -c screenshots.config.ts tests/audit2-evidence.spec.ts
const directory = resolve(import.meta.dirname, '../../docs/screenshots/audit2');
const pdfPath = resolve(import.meta.dirname, '../../server/seed-data/docs/site-plan.pdf');
const pdf = readFileSync(pdfPath);
const viewport = { width: 1440, height: 900 };

/** One signed-in page per persona for the whole file, so each TOTP login happens once. */
const test = base.extend<object, { as: (name: string) => Promise<Page> }>({
  as: [async ({ browser }, use) => {
    const pages = new Map<string, Page>();
    await use(async (name) => {
      let page = pages.get(name);
      if (!page) {
        const context = await browser.newContext({ baseURL: process.env.E2E_BASE_URL, viewport });
        page = await context.newPage();
        await login(page, name);
        pages.set(name, page);
      }
      return page;
    });
    for (const page of pages.values()) await page.context().close();
  }, { scope: 'worker' }],
});
test.describe.configure({ mode: 'serial' });
test.beforeAll(() => mkdirSync(directory, { recursive: true }));

async function capture(page: Page, name: string, fullPage = false) {
  await ready(page);
  await expect(page.locator('main')).not.toContainText('Loading…');
  await expect(page.getByRole('button', { name: 'Dismiss', exact: true })).toHaveCount(0, { timeout: 20000 });
  await page.screenshot({ path: resolve(directory, name), animations: 'disabled', fullPage });
}
async function scrollTo(target: Locator) {
  // The staff/resident header stays pinned (about 73px) while scrolling.
  await target.first().evaluate(el => window.scrollTo(0, window.scrollY + el.getBoundingClientRect().top - 96));
}
function card(page: Page, title: string) {
  return page.getByRole('heading', { name: title, exact: true }).locator('xpath=ancestor::section[1]');
}

async function call(page: Page, method: string, url: string, data?: unknown) {
  const me = await (await page.request.get('/api/me')).json();
  const res = await page.request.fetch(url, { method, data, headers: { 'X-CSRF-Token': me.csrf_token ?? '', 'Idempotency-Key': crypto.randomUUID() } });
  if (!res.ok()) throw new Error(`${method} ${url}: ${res.status()} ${await res.text()}`);
  const text = await res.text();
  return text ? JSON.parse(text) : null;
}
async function revision(page: Page, id: number): Promise<number> {
  return (await call(page, 'GET', `/api/cases/${id}`)).case.revision;
}
async function caseOf(page: Page, id: number | string) {
  return (await call(page, 'GET', `/api/cases/${id}`)).case;
}
async function act(page: Page, id: number, action: string) {
  await call(page, 'POST', `/api/cases/${id}/actions/${action}`, { expected_revision: await revision(page, id), reason: 'Fictional demonstration assessment recorded.' });
}
type Cond = { field: string; equals: unknown } | undefined;
type Field = { key: string; type: string; required?: boolean; show_if?: Cond; options?: { value: string }[]; columns?: Field[] };
function sample(f: Field): unknown {
  switch (f.type) {
    case 'checkbox': return true;
    case 'number': return 100;
    case 'select': return f.options![0].value;
    case 'multiselect': return [f.options![0].value];
    case 'date': return futureDate(0);
    case 'email': return 'alexey@example.invalid';
    case 'phone': return '+672 3 55501';
    case 'location': return { lat: -29.04, lng: 167.95, description: 'Fictional site on Taylors Road' };
    default: return ({ applicant_name: 'Alexey Turner', first_name: 'Alexey', last_name: 'Turner', postal_address: 'Fictional 10 Taylors Road', property_ref: 'Portion DEMO-77, Taylors Road' } as Record<string, string>)[f.key] ?? 'Fictional browser demonstration';
  }
}
/** Setup only: the signed-in resident lodges `slug` through the same API the form uses, with every other
 * required (and shown) field and document filled with fictional values. */
async function lodge(page: Page, slug: string, extra: Record<string, unknown>): Promise<number> {
  const def = (await call(page, 'GET', `/api/public/services/${slug}`)).definition;
  const answers: Record<string, unknown> = { ...extra };
  const shown = (cond: Cond) => {
    if (!cond) return true;
    const answer = answers[cond.field];
    return Array.isArray(answer) ? answer.includes(cond.equals) : answer === cond.equals;
  };
  for (const f of def.fields as Field[]) {
    if (!f.required || f.key in answers || !shown(f.show_if)) continue;
    answers[f.key] = f.type === 'group' ? [Object.fromEntries(f.columns!.filter(c => c.required).map(c => [c.key, sample(c)]))] : sample(f);
  }
  const id = (await call(page, 'POST', `/api/services/${slug}/drafts`, { applicant_org_id: null })).id;
  await call(page, 'PUT', `/api/cases/${id}/draft`, { answers });
  const me = await (await page.request.get('/api/me')).json();
  for (const doc of def.documents.filter((d: { required: boolean; show_if?: Cond }) => d.required && shown(d.show_if))) {
    const res = await page.request.post(`/api/cases/${id}/documents`, { headers: { 'X-CSRF-Token': me.csrf_token }, multipart: { requirement_key: doc.key, title: doc.label, file: { name: 'plan.pdf', mimeType: 'application/pdf', buffer: pdf } } });
    expect(res.ok(), await res.text()).toBe(true);
  }
  await call(page, 'POST', `/api/cases/${id}/submit`, {});
  return id;
}
/** The resident pays the outstanding invoice through DemoPay in the browser. */
async function pay(page: Page, id: number) {
  await page.goto(`/my/cases/${id}?tab=finance.money`);
  await page.getByRole('button', { name: /^Pay / }).click();
  await expect(page).toHaveURL(/\/mock\/pay\/checkout\//);
  await page.getByRole('button', { name: 'Pay with test card', exact: true }).click();
  await expect(page.getByText('Payment confirmed', { exact: true })).toBeVisible({ timeout: 45000 });
  await expect.poll(async () => (await caseOf(page, id)).current_step, { timeout: 45000 }).not.toBe('payment');
}
/** Setup only: Priya prepares and submits a decision, Helen issues it. */
async function decide(priya: Page, helen: Page, id: number, kind: string, supersedes: number | null = null): Promise<number> {
  const template = (await call(priya, 'GET', '/api/decision-templates')).find((t: { decision_type: string }) => t.decision_type === kind).id;
  const decision = (await call(priya, 'POST', `/api/cases/${id}/decisions`, { decision_type: kind, outcome: 'approved', reasons: 'Fictional assessment: complies.', conditions: '', template_id: template, supersedes_decision_id: supersedes, expected_revision: await revision(priya, id) })).id;
  await call(priya, 'POST', `/api/cases/${id}/decisions/${decision}/submit`, { expected_revision: await revision(priya, id) });
  await call(helen, 'POST', `/api/cases/${id}/decisions/${decision}/issue`, { expected_revision: await revision(helen, id) });
  return decision;
}
async function routeTab(page: Page, id: number) {
  await page.goto(`/staff/cases/${id}?tab=documents.route`);
  await ready(page);
}

test('N-01 fee assessment and payment gate; N-02 DA-only scope; N-03 exhibition not required', async ({ as }) => {
  test.setTimeout(300000);
  const alexey = await as('Alexey'), olga = await as('Olga'), priya = await as('Priya'), helen = await as('Helen');
  const id = await lodge(alexey, 'development-application', { approvals_sought: ['development_approval', 'building_approval'], estimated_cost: 120000, property_ref: 'Portion DEMO-77, Taylors Road' });

  await routeTab(olga, id);
  await advance(olga); // Check request → Determine fees
  await olga.getByRole('tab', { name: 'Fees, scope and exhibition', exact: true }).click();
  await expect(olga.getByText('System calculation: Building Development and Works scale')).toBeVisible();
  await expect(olga.getByText('$880.00', { exact: false }).first()).toBeVisible();
  await olga.getByRole('button', { name: 'Record fee assessment', exact: true }).click();
  await expect(olga.getByRole('list', { name: 'Fee assessment history' })).toContainText('Assessment v1');
  await scrollTo(card(olga, 'Fee assessment'));
  await capture(olga, 'n01-fee-assessment.png');

  await advance(olga); // → Receive payment: the invoice is issued
  await olga.reload();
  await ready(olga);
  await expect(olga.getByText('Payment has not been received yet.', { exact: false }).first()).toBeVisible();
  await expect(olga.getByText('Invoice issued — payment outstanding. Decisions cannot be issued until it is paid.')).toBeVisible();
  const complete = olga.getByRole('button', { name: 'Complete this step', exact: true });
  if (await complete.count()) {
    await complete.click();
    await olga.getByRole('dialog').getByLabel('Reason or completion note').fill('Trying to continue before payment.');
    await olga.getByRole('button', { name: 'Confirm action', exact: true }).click();
    await expect(olga.getByRole('dialog')).toContainText('Payment has not been received yet.');
  }
  await capture(olga, 'n01-payment-gate.png');
  if (await olga.getByRole('dialog').count()) await olga.getByRole('dialog').getByRole('button', { name: 'Back', exact: true }).click();

  await pay(alexey, id);

  await routeTab(priya, id);
  await priya.getByRole('checkbox', { name: 'Building approval', exact: true }).uncheck();
  await priya.getByLabel('Reason', { exact: true }).fill('Fictional: the works are exempt from building approval; development approval only.');
  await priya.getByRole('button', { name: 'Confirm approval scope', exact: true }).click();
  await expect(priya.getByText('Confirmed by Council', { exact: true })).toBeVisible();
  const scope = card(priya, 'Approvals in scope');
  await scope.getByText('Scope history', { exact: true }).click();
  await expect(scope).toContainText('Reason: Fictional: the works are exempt from building approval');
  await scrollTo(scope);
  await capture(priya, 'n02-scope-da-only.png');

  await advance(priya); // Assess application → Public exhibition
  await priya.getByRole('tab', { name: 'Fees, scope and exhibition', exact: true }).click();
  await priya.getByLabel('Reason exhibition is not required').fill('Fictional: neighbours notified by letter; no public comment period applies to this minor proposal.');
  await priya.getByRole('button', { name: 'Record exhibition not required', exact: true }).click();
  await expect(priya.getByText('Exhibition not required for this request', { exact: true })).toBeVisible();
  await scrollTo(card(priya, 'Public exhibition'));
  await capture(priya, 'n03-not-required.png');
  await advance(priya); // → Issue decisions
  await decide(priya, helen, id, 'development_approval');
  await expect.poll(async () => (await caseOf(alexey, id)).status, { timeout: 45000 }).toBe('completed');
});

test('N-01 lapse-only modification fee and N-02 approval chains after modifying only the DA', async ({ as }) => {
  test.setTimeout(300000);
  const alexey = await as('Alexey'), olga = await as('Olga'), priya = await as('Priya'), helen = await as('Helen');
  // Setup: a combined DA + BA request, decided through the API.
  const combined = await lodge(alexey, 'development-application', { approvals_sought: ['development_approval', 'building_approval'], estimated_cost: 80000, property_ref: 'Portion DEMO-78, Taylors Road' });
  await act(olga, combined, 'advance');
  await call(olga, 'POST', `/api/cases/${combined}/building-fee`, { method: 'schedule', expected_revision: await revision(olga, combined) });
  await act(olga, combined, 'advance');
  await pay(alexey, combined);
  await call(priya, 'POST', `/api/cases/${combined}/approval-scope`, { approvals: ['development_approval', 'building_approval'], reason: 'Fictional: both approvals needed.', expected_revision: await revision(priya, combined) });
  await act(priya, combined, 'advance');
  await call(priya, 'POST', `/api/cases/${combined}/exhibition-not-required`, { reason: 'Fictional: notified by letter.', expected_revision: await revision(priya, combined) });
  await act(priya, combined, 'advance');
  const da = await decide(priya, helen, combined, 'development_approval');
  await decide(priya, helen, combined, 'building_approval');
  await expect.poll(async () => (await caseOf(alexey, combined)).status, { timeout: 45000 }).toBe('completed');
  const project = (await call(alexey, 'GET', `/api/cases/${combined}/decisions`)).building_project_id;

  // A lapse-only modification of the DA alone.
  const modification = await lodge(alexey, 'modify-approval', { original_approval: { decision_ids: [da] }, modification_types: ['lapse_date'], proposed_lapse_date: '2027-12-01', lapse_date_reasons: 'Fictional: builder availability.', estimated_cost: 45000 });
  await routeTab(olga, modification);
  await advance(olga); // Check request → Determine fees
  await olga.getByRole('tab', { name: 'Fees, scope and exhibition', exact: true }).click();
  await expect(olga.getByText('System calculation: Basic modification (lapse date only)')).toBeVisible();
  await expect(olga.getByText('$250.00', { exact: false }).first()).toBeVisible();
  // Contrast: the same request assessed as a standard modification (conditions too) uses the scale; the
  // re-assessment back to the applicant's lapse-only answer records the flat $250 basic fee.
  const fee = card(olga, 'Fee assessment');
  await fee.getByRole('checkbox', { name: 'Modification to condition(s)', exact: true }).check();
  await fee.getByLabel('Basis / reason').fill('Fictional comparison: as a standard modification (conditions and lapse date).');
  await olga.getByRole('button', { name: 'Record fee assessment', exact: true }).click();
  await expect(olga.getByRole('list', { name: 'Fee assessment history' })).toContainText('Standard modification (Building and Works scale)');
  await fee.getByRole('checkbox', { name: 'Modification to condition(s)', exact: true }).uncheck();
  await fee.getByLabel('Basis / reason').fill('Applicant confirmed only the approval lapse date changes: basic modification.');
  await olga.getByRole('button', { name: 'Record re-assessment', exact: true }).click();
  await expect(olga.getByRole('list', { name: 'Fee assessment history' })).toContainText('Assessment v2 · Basic modification (lapse date only) · $250.00');
  await scrollTo(fee.getByText('System calculation: Basic modification (lapse date only)'));
  await capture(olga, 'n01-modification-fee.png');

  await act(olga, modification, 'advance');
  await pay(alexey, modification);
  await call(priya, 'POST', `/api/cases/${modification}/approval-scope`, { originals: [da], reason: 'Fictional: only the development approval lapse date changes.', expected_revision: await revision(priya, modification) });
  await act(priya, modification, 'advance');
  await call(priya, 'POST', `/api/cases/${modification}/exhibition-not-required`, { reason: 'Fictional: lapse date only.', expected_revision: await revision(priya, modification) });
  await act(priya, modification, 'advance');
  await decide(priya, helen, modification, 'modification_approval', da);
  await expect.poll(async () => (await caseOf(alexey, modification)).status, { timeout: 45000 }).toBe('completed');

  await alexey.goto(`/my/projects/${project}`);
  await ready(alexey);
  const chains = card(alexey, 'Approval chains');
  await expect(chains).toContainText(`Development approval #${da}Current version: decision #`);
  await expect(chains).toContainText(`Original #${da}Superseded`);
  await expect(chains).toContainText('(supersedes');
  await scrollTo(chains);
  await capture(alexey, 'n02-project-chains.png');
});

test('N-03 an open exhibition with a comment blocks the request until closed and considered', async ({ as, browser }) => {
  test.setTimeout(300000);
  const alexey = await as('Alexey'), olga = await as('Olga'), priya = await as('Priya'), helen = await as('Helen');
  const id = await lodge(alexey, 'development-application', { approvals_sought: ['development_approval'], estimated_cost: 50000, property_ref: 'Portion DEMO-79, Taylors Road' });
  await act(olga, id, 'advance');
  await call(olga, 'POST', `/api/cases/${id}/building-fee`, { method: 'schedule', expected_revision: await revision(olga, id) });
  await act(olga, id, 'advance');
  await pay(alexey, id);
  await call(priya, 'POST', `/api/cases/${id}/approval-scope`, { approvals: ['development_approval'], reason: 'Fictional: DA only.', expected_revision: await revision(priya, id) });
  await act(priya, id, 'advance');
  const docs = await call(priya, 'GET', `/api/cases/${id}/documents`);
  const now = Date.now();
  const exhibit = (await call(priya, 'POST', '/api/exhibitions', { case_id: id, title: 'Fictional shed and studio — public exhibition', summary: 'Fictional proposal on exhibition.', opens_at: new Date(now - 60000).toISOString(), closes_at: new Date(now + 60000).toISOString(), expected_revision: await revision(priya, id) })).id;
  await call(priya, 'POST', `/api/exhibitions/${exhibit}/items`, { source_document_version_id: docs[0].versions[0].id, title: 'Site plan', redactions: [], expected_revision: await revision(priya, id) });
  await call(helen, 'POST', `/api/exhibitions/${exhibit}/publish`, { expected_revision: await revision(helen, id) });

  const visitor = await browser.newContext({ baseURL: process.env.E2E_BASE_URL, viewport });
  try {
    const publicPage = await visitor.newPage();
    await publicPage.goto(`/notices/${exhibit}`);
    await publicPage.getByLabel('Name', { exact: true }).fill('Fictional neighbour');
    await publicPage.getByLabel('Email', { exact: true }).fill('neighbour@example.invalid');
    await publicPage.getByLabel('Written submission').fill('Fictional concern about stormwater runoff.');
    await publicPage.getByRole('button', { name: /Send/ }).click();
    await expect(publicPage.getByText('Submission received', { exact: true })).toBeVisible();
  } finally { await visitor.close(); }

  await routeTab(priya, id);
  const exhibition = card(priya, 'Public exhibition');
  await expect(exhibition).toContainText('1 public submission(s), 1 awaiting a consideration outcome.');
  await expect(exhibition).toContainText('open until');
  await expect(priya.getByRole('button', { name: 'Skip optional step' })).toHaveCount(0);
  // The open exhibition, its unconsidered comment and the stage block, with no Skip action offered.
  await scrollTo(exhibition);
  await capture(priya, 'n03-exhibition-blocked.png');
  await priya.getByRole('button', { name: 'Complete this step', exact: true }).click();
  await priya.getByRole('dialog').getByLabel('Reason or completion note').fill('Trying to finish early.');
  await priya.getByRole('button', { name: 'Confirm action', exact: true }).click();
  await expect(priya.getByRole('dialog').getByText('open until', { exact: false })).toBeVisible();
  await priya.getByRole('dialog').getByRole('button', { name: 'Back', exact: true }).click();

  await expect.poll(async () => {
    await priya.reload();
    return priya.getByText('need a recorded consideration outcome', { exact: false }).count();
  }, { timeout: 120000, intervals: [3000] }).toBeGreaterThan(0);
  await priya.goto(`/staff/exhibitions/${exhibit}`);
  await ready(priya);
  await priya.getByLabel('Consideration outcome').fill('Fictional: stormwater is addressed by a condition of approval.');
  await priya.getByRole('button', { name: 'Record consideration', exact: true }).click();
  await expect(priya.getByText('Outcome: Fictional: stormwater is addressed by a condition of approval.', { exact: false })).toBeVisible();
  await priya.goto(`/staff/cases/${id}`);
  await ready(priya);
  await advance(priya); // → Issue decisions
  await decide(priya, helen, id, 'development_approval');
  await expect.poll(async () => (await caseOf(priya, id)).status, { timeout: 45000 }).toBe('completed');
  await routeTab(priya, id);
  await expect(exhibition).toContainText('Closed');
  await expect(exhibition).toContainText('1 public submission(s), 0 awaiting a consideration outcome.');
  // Full page: the Completed status at the top and the closed, considered exhibition at the bottom in one image.
  await capture(priya, 'n03-exhibition-considered.png', true);
});

test('N-04 a Builder-made service with a service-response letter step and a reassignable field task', async ({ as }) => {
  test.setTimeout(300000);
  const mark = await as('Mark');
  await mark.goto('/admin/services');
  await mark.getByRole('link', { name: 'Create service', exact: true }).click();
  await mark.getByLabel('Name', { exact: false }).first().fill('Fictional verge mowing request');
  await mark.getByLabel('Slug', { exact: false }).fill('audit2-evidence-verge-mowing');
  await mark.getByRole('button', { name: 'Create blank draft' }).click();
  await expect(mark).toHaveURL(/\/admin\/services\/\d+/);
  await mark.getByLabel('Summary', { exact: false }).fill('Ask Council to mow a fictional road verge.');
  await mark.getByLabel('What the applicant receives').fill('A site visit and a written response.');
  await mark.getByLabel('Who can apply').fill('Norfolk Island residents.');
  await mark.getByLabel('Price calculation', { exact: true }).fill('No fee for this fictional demonstration.');
  await mark.getByRole('tab', { name: 'Fields', exact: true }).click();
  await mark.getByRole('button', { name: 'Add field', exact: true }).click();
  await mark.getByLabel('Key', { exact: false }).fill('verge');
  await mark.getByLabel('Label', { exact: false }).first().fill('Verge location');
  await mark.getByRole('checkbox', { name: 'Required while shown' }).check();
  await mark.getByRole('tab', { name: 'Workflow steps', exact: true }).click();
  await mark.getByRole('button', { name: 'Add workflow step', exact: true }).click();
  await mark.getByLabel('Kind', { exact: true }).nth(2).selectOption('task');
  await mark.getByLabel('Task kind').selectOption('general');
  await mark.getByLabel('Staff label').nth(2).fill('Mow verge');
  await mark.getByLabel('Applicant label').nth(2).fill('Council is visiting the verge');
  await mark.getByRole('button', { name: 'Move item 3 up' }).click();
  await mark.getByRole('button', { name: 'Add workflow step', exact: true }).click();
  await mark.getByLabel('Kind', { exact: true }).nth(3).selectOption('module');
  await mark.getByLabel('Module handler').selectOption('documents.letter_issued:service_response');
  await mark.getByLabel('Staff label').nth(3).fill('Write response');
  await mark.getByLabel('Applicant label').nth(3).fill('Preparing your response');
  await mark.getByRole('button', { name: 'Move item 4 up' }).click();
  await scrollTo(mark.getByLabel('Module handler'));
  await mark.evaluate(() => window.scrollBy(0, -380));
  await capture(mark, 'n04-builder-letter-step.png');
  await mark.getByRole('button', { name: 'Validate and publish' }).click();
  await expect(mark.getByText('Immutable version', { exact: true })).toBeVisible();
  const worker = await call(mark, 'POST', '/api/admin/users', { email: 'evidence.verge.worker@example.invalid', display_name: 'Fictional Verge Worker', kind: 'staff', password: 'fictional-verge-password', job_title: 'Field worker' });
  await call(mark, 'POST', `/api/admin/users/${worker.id}/roles`, { role: 'field_worker' });

  const alexey = await as('Alexey');
  await alexey.goto('/services');
  await alexey.getByRole('link', { name: /Fictional verge mowing request/ }).click();
  await alexey.getByRole('button', { name: /Start request/ }).click();
  await alexey.getByLabel('Verge location').fill('Fictional verge outside 12 Taylors Road');
  await alexey.getByRole('button', { name: 'Review before submitting' }).click();
  await alexey.getByRole('button', { name: 'Submit request', exact: true }).click();
  await expect(alexey).toHaveURL(/\/my\/cases\/\d+/);
  const caseId = new URL(alexey.url()).pathname.split('/').pop();

  const olga = await as('Olga');
  await olga.goto(`/staff/cases/${caseId}`);
  await ready(olga);
  await advance(olga);
  await expect(olga.getByText('The field task for this step is not finished yet.')).toBeVisible();
  await olga.getByRole('tab', { name: 'Tasks', exact: true }).click();
  await olga.getByRole('tabpanel').getByLabel('Assign field worker').selectOption({ label: 'Fictional Verge Worker' });
  await olga.getByRole('tabpanel').getByRole('button', { name: 'Assign', exact: true }).click();
  await expect.poll(async () => (await call(olga, 'GET', `/api/cases/${caseId}/tasks`))[0].assigned_to).toBe(worker.id);
  await olga.goto(`/staff/cases/${caseId}`);
  await ready(olga);
  await olga.getByRole('tab', { name: 'Tasks', exact: true }).click();
  await expect(olga.getByRole('tabpanel')).toContainText('Fictional Verge Worker');
  await scrollTo(olga.getByRole('heading', { level: 1 }));
  await capture(olga, 'n04-generic-task-reassign.png');
  await olga.getByRole('tabpanel').getByLabel('Assign field worker').selectOption({ label: 'Jake Rowe' });
  await olga.getByRole('tabpanel').getByRole('button', { name: 'Assign', exact: true }).click();
  await expect.poll(async () => (await call(olga, 'GET', `/api/cases/${caseId}/tasks`))[0].assigned_to).not.toBe(worker.id);
  const taskId = (await call(olga, 'GET', `/api/cases/${caseId}/tasks`))[0].id;

  const jake = await as('Jake');
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

  await olga.goto(`/staff/cases/${caseId}`);
  await ready(olga);
  await expect(olga.getByText('Issue the service response letter before continuing.')).toBeVisible();
  await olga.getByRole('tab', { name: 'Response letters', exact: true }).click();
  await olga.getByLabel('Response for the applicant').fill('The fictional verge was mowed on Tuesday and the clippings were removed.');
  await olga.getByRole('button', { name: 'Issue service response letter' }).click();
  await expect(olga.getByRole('tabpanel')).toContainText('Issued');
  await expect.poll(async () => (await caseOf(olga, caseId!)).status).toBe('completed');
  await olga.goto(`/staff/cases/${caseId}?tab=documents.letters`);
  await ready(olga);
  await expect(olga.getByRole('tabpanel')).toContainText('Issued');
  await capture(olga, 'n04-letter-issued.png');
});

test('N-06 modify-approval form with several applicants, landowners, parcels and modification types', async ({ as }) => {
  test.setTimeout(300000);
  const ben = await as('Ben');
  const approvals = await call(ben, 'GET', '/api/my/issued-approvals');
  const org = (await call(ben, 'GET', '/api/my/organisations'))[0].id;
  // The approval picker is exercised elsewhere; preset two originals so the form shows the multi-original case.
  const draft = await call(ben, 'POST', '/api/services/modify-approval/drafts', { applicant_org_id: org });
  await call(ben, 'PUT', `/api/cases/${draft.id}/draft`, { answers: { original_approval: { decision_ids: approvals.slice(0, 2).map((a: { id: number }) => a.id) } } });
  await ben.goto(`/my/drafts/${draft.id}`);
  await ready(ben);
  const person = async (group: string, row: number, first: string, last: string, email: string) => {
    const scope = ben.getByRole('group', { name: `${group}: row ${row}`, exact: true });
    await scope.getByLabel('First name').fill(first);
    await scope.getByLabel('Last name').fill(last);
    await scope.getByLabel('Postal address').fill('PO Box 95, Norfolk Island 2899');
    await scope.getByLabel('Mobile').fill('+672 5 12345');
    await scope.getByLabel('Email').fill(email);
    return scope;
  };
  await ben.getByRole('button', { name: 'Add applicants row' }).click();
  await ben.getByRole('button', { name: 'Add applicants row' }).click();
  await person('Applicants', 1, 'Ben', 'Carter', 'ben@example.invalid');
  await person('Applicants', 2, 'Bea', 'Carter', 'bea@example.invalid');
  await ben.getByLabel('Are all landowners listed above as applicants?').selectOption('no');
  await ben.getByRole('button', { name: 'Add landowners row' }).click();
  await ben.getByRole('button', { name: 'Add landowners row' }).click();
  for (const [row, first] of [[1, 'Olive'], [2, 'Oscar']] as const) {
    const scope = await person('Landowners', row, first, 'Owner', `${first.toLowerCase()}@example.invalid`);
    await scope.getByRole('checkbox', { name: /This landowner consents/ }).check();
  }
  await ben.getByLabel('Property address').fill('44 Taylors Road, Burnt Pine');
  for (const [row, portion] of [[1, '44h'], [2, '44j']] as const) {
    await ben.getByRole('button', { name: 'Add land parcels row' }).click();
    const scope = ben.getByRole('group', { name: `Land parcels: row ${row}`, exact: true });
    await scope.getByLabel('Portion number').fill(portion);
    await scope.getByLabel('Lot number').fill(String(row));
    await scope.getByLabel('Section number').fill('9');
    await scope.getByLabel(/Land area/).fill(`${row},000 m²`);
  }
  await ben.getByLabel('Land tenure').selectOption('Freehold');
  await ben.getByLabel('Zoning').selectOption('Rural');
  await ben.getByLabel('What is the land currently used for?').fill('Dwelling house');
  await ben.getByRole('checkbox', { name: 'Alterations and additions to existing structure(s)' }).check();
  await ben.getByRole('checkbox', { name: 'Modification to condition(s)' }).check();
  await ben.getByRole('checkbox', { name: 'Change of approval lapse date' }).check();
  await ben.getByLabel('Condition(s): describe the modification and its expected impact').fill('Condition 4: reduce the water tank to 15,000 L.');
  await ben.getByLabel('Proposed approval lapse date').fill('2027-06-30');
  await ben.getByLabel('Reasons for requiring the change of lapse date').fill('Builder availability.');
  await ben.getByLabel(/The proposed modified use or development/).fill('Same dwelling with a smaller tank.');
  await ben.getByLabel('Changes in the external environment since the original approval').fill('None.');
  await ben.getByLabel('Total estimated cost of building and works (AUD)').fill('45000');
  await ben.getByRole('checkbox', { name: 'Trees Act 1997 (NI)' }).check();
  await ben.getByRole('checkbox', { name: /declare that the information in this application is correct/ }).check();
  for (const label of ['Copy of title search', 'Signed consent of all landowners', 'Description of expected impacts, with relevant plans and drawings']) {
    await ben.getByLabel(label, { exact: false }).first().setInputFiles(pdfPath);
    await expect(ben.getByText('Document attached.', { exact: true })).toBeVisible();
  }
  await ben.evaluate(() => window.scrollTo(0, 0));
  await capture(ben, 'n06-modify-form-groups.png', true);
  await ben.getByRole('button', { name: 'Review before submitting' }).click();
  await ben.getByRole('button', { name: 'Submit request', exact: true }).click();
  await expect(ben).toHaveURL(/\/my\/cases\/\d+/);
  const caseId = new URL(ben.url()).pathname.split('/').pop();

  const priya = await as('Priya');
  await priya.goto(`/staff/cases/${caseId}`);
  await ready(priya);
  const overview = priya.getByRole('tabpanel');
  for (const text of ['Bea', 'Oscar', '44j', 'Modification to condition(s)', 'Change of approval lapse date', 'Builder availability.', 'NSH-']) {
    await expect(overview).toContainText(text);
  }
  await scrollTo(priya.getByRole('tablist'));
  await capture(priya, 'n06-staff-answers.png');
});

test('N-05 stage notices and pipeline crossing in the catalogue, returned and accepted version, issued decision', async ({ as }) => {
  test.setTimeout(300000);
  const ben = await as('Ben');
  await ben.goto('/services');
  await expect(ben.getByText("Builder's Stage E Compliance Declaration Notice").first()).toBeVisible();
  // A catalogue search narrows the list to the five stage notices and the Form 212 pipeline crossing.
  await ben.getByLabel('Search services').fill('compliance pipeline');
  const cards = ben.locator('main').getByRole('link', { name: /Compliance Declaration Notice|Pipeline or Conduit Crossing/ });
  await expect(cards).toHaveCount(6);
  await scrollTo(ben.getByText(/services found/));
  await capture(ben, 'n05-catalogue-stage-pipeline.png');

  // Stage B notice: returned for a new version, version 2 accepted.
  const projects = await call(ben, 'GET', '/api/my/building-projects');
  const project = projects.find((p: { approvals: unknown[] }) => p.approvals.length > 0);
  await ben.goto('/services/builder-stage-b-notice');
  await ben.getByRole('button', { name: /Start request/ }).click();
  await expect(ben).toHaveURL(/\/my\/drafts\/\d+/);
  await ben.getByLabel('Your building projects').selectOption(project.reference);
  await ben.getByLabel('Person who carried out the building work — first name').fill('Ben');
  await ben.getByLabel('Person who carried out the building work — last name').fill('Carter');
  await ben.getByLabel('Phone number').fill('+672 3 55501');
  await ben.getByLabel('Email', { exact: true }).fill('ben@example.invalid');
  await ben.getByLabel('Portion number').fill('Portion DEMO-44');
  await ben.getByLabel('Property address').fill('44 Fictional Taylors Road');
  await ben.getByRole('checkbox', { name: /I have completed the building work described for Inspection Stage B/ }).check();
  await ben.getByRole('checkbox', { name: /Stage B building work specified in Schedule 3/ }).check();
  await ben.getByRole('checkbox', { name: /in accordance with the building approval/ }).check();
  await ben.getByLabel('Date of the compliance declaration').fill(futureDate(0));
  await ben.getByLabel(/Specified Stage B work/).selectOption('yes');
  await ben.getByLabel("Signed Builder's Stage B compliance declaration notice", { exact: false }).setInputFiles(pdfPath);
  await expect(ben.getByText(/Attached:/).first()).toBeVisible();
  await ben.getByRole('button', { name: 'Review before submitting' }).click();
  await ben.getByRole('button', { name: 'Submit request', exact: true }).click();
  await expect(ben).toHaveURL(/\/my\/cases\/\d+/);
  const id = Number(new URL(ben.url()).pathname.split('/').pop());

  const olga = await as('Olga');
  await olga.goto(`/staff/cases/${id}?tab=documents.documents`);
  await ready(olga);
  await olga.getByText('Comment or ask for a new version', { exact: true }).first().click();
  await olga.getByLabel('Comment', { exact: true }).fill('The builder signature is missing. Please upload the signed notice.');
  await olga.getByRole('checkbox', { name: /Ask for a new version/ }).check();
  await olga.getByRole('button', { name: 'Ask for a new version', exact: true }).click();
  await expect(olga.getByText('New version requested', { exact: true })).toBeVisible();
  await ben.goto(`/my/cases/${id}?tab=documents.documents`);
  await ready(ben);
  const replace = ben.locator('form').filter({ hasText: 'This replaces' }).first();
  await replace.getByLabel('File', { exact: true }).setInputFiles(pdfPath);
  await replace.getByLabel('What changed?').fill('Signed declaration');
  await replace.getByRole('checkbox', { name: /builder signature is missing/ }).check();
  await replace.getByRole('button', { name: 'Upload replacement version', exact: true }).click();
  await expect(ben.getByText('Version 2', { exact: true })).toBeVisible();
  await olga.goto(`/staff/cases/${id}`);
  await ready(olga);
  await advance(olga);
  await pay(ben, id);
  await expect.poll(async () => (await caseOf(olga, id)).current_step, { timeout: 45000 }).toBe('site');
  const task = (await call(olga, 'GET', `/api/cases/${id}/tasks`)).find((t: { kind: string }) => t.kind === 'site_inspection');
  const jake = await as('Jake');
  await jake.goto(`/staff/field/${task.id}`);
  await expect(jake.getByRole('heading', { name: 'Checklist', exact: true })).toBeVisible();
  for (const item of await jake.locator('section').filter({ has: jake.getByRole('heading', { name: 'Checklist', exact: true }) }).getByRole('checkbox').all()) {
    if (!(await item.isChecked())) await item.click();
    await expect(item).toBeChecked();
  }
  await jake.getByLabel('Result', { exact: false }).fill('Stage B framework inspected; work may continue.');
  await jake.getByRole('button', { name: 'Save result', exact: true }).click();
  await jake.getByRole('button', { name: 'Mark done', exact: true }).click();
  await expect.poll(async () => (await caseOf(olga, id)).current_step, { timeout: 45000 }).toBe('decision');
  const priya = await as('Priya'), helen = await as('Helen');
  await decide(priya, helen, id, 'service_response');
  await expect.poll(async () => (await caseOf(olga, id)).status, { timeout: 45000 }).toBe('completed');
  await ben.goto(`/my/cases/${id}?tab=documents.decisions`);
  await ben.getByRole('link', { name: 'View building project and approval history' }).click();
  await expect(ben).toHaveURL(/\/my\/projects\/\d+/);
  await ready(ben);
  const returned = ben.getByText(/Returned for a new version: The builder signature is missing/);
  await expect(returned).toBeVisible();
  await expect(ben.getByText(/Decision based on .* v2/).first()).toBeVisible();
  await scrollTo(returned.locator('xpath=ancestor::li[1]'));
  await ben.evaluate(() => window.scrollBy(0, -120));
  await capture(ben, 'n05-stage-returned-new-version.png');

  // Form 212 pipeline crossing: application to issued decision.
  await ben.goto('/services/pipeline-conduit-crossing');
  await ben.getByRole('button', { name: /Start request/ }).click();
  await expect(ben).toHaveURL(/\/my\/drafts\/\d+/);
  await ben.getByLabel('Name of applicant').fill('Ben Carter');
  await ben.getByLabel('Postal address').fill('44 Fictional Taylors Road');
  await ben.getByLabel('Email address').fill('ben@example.invalid');
  await ben.getByLabel('Phone (work or mobile)').fill('+672 3 55501');
  await ben.getByLabel('ABN / ACN number').fill('00 000 000 000');
  await ben.getByLabel('Position held by the person signing').fill('Director');
  await ben.getByLabel('Name of road that the pipeline or conduit will be installed in').fill('Taylors Road');
  await ben.getByLabel(/Location of proposed pipeline or conduit crossing/).fill('Portion DEMO-44');
  await ben.getByLabel('Size and type of proposed pipe or conduit').fill('Fictional 50 mm PE water main in a 100 mm conduit');
  await ben.getByLabel('Description of land adjacent to the road reserve (Portion numbers)').fill('Portion DEMO-44 and DEMO-45');
  await ben.getByRole('checkbox', { name: /no work is to be carried out/ }).check();
  await ben.getByRole('checkbox', { name: /costs of restoring the road pavement/ }).check();
  await ben.getByRole('checkbox', { name: 'I declare that the information is correct' }).check();
  await ben.getByLabel('Sketch or drawing showing location, dimensions and levels', { exact: false }).setInputFiles(pdfPath);
  await expect(ben.getByText(/Attached:/).first()).toBeVisible();
  await ben.getByRole('button', { name: 'Review before submitting' }).click();
  await ben.getByRole('button', { name: 'Submit request', exact: true }).click();
  await expect(ben).toHaveURL(/\/my\/cases\/\d+/);
  const pipe = Number(new URL(ben.url()).pathname.split('/').pop());
  await olga.goto(`/staff/cases/${pipe}`);
  await ready(olga);
  await advance(olga);
  await advance(olga);
  await decide(priya, helen, pipe, 'service_response');
  await expect.poll(async () => (await caseOf(ben, pipe)).status, { timeout: 45000 }).toBe('completed');
  await ben.goto(`/my/cases/${pipe}?tab=documents.decisions`);
  await ready(ben);
  await expect(ben.getByRole('link', { name: 'Download issued decision' })).toBeVisible();
  await capture(ben, 'n05-pipeline-decision.png');
});

test('N-03 re-audit: the Builder refuses a decision moved above the public exhibition', async ({ as }) => {
  test.setTimeout(120000);
  const mark = await as('Mark');
  const steps: { key: string }[] = (await call(mark, 'GET', '/api/public/services/development-application')).definition.workflow.steps;
  const decision = steps.findIndex((s) => s.key === 'decision');
  await mark.goto('/admin/services');
  await mark.getByRole('link', { name: 'Application for Development and/or Building Approval' }).click();
  await mark.getByRole('button', { name: 'New draft from published' }).click();
  await expect(mark.getByRole('button', { name: /Version \d+ — draft/ }).first()).toBeVisible();
  await mark.getByRole('tab', { name: 'Workflow steps', exact: true }).click();
  await mark.getByRole('button', { name: `Move item ${decision + 1} up` }).click();
  await mark.getByRole('button', { name: 'Validate and publish' }).click();
  await expect(mark.getByRole('alert')).toContainText('Move the decision step below the exhibition step');
  await capture(mark, 'n03-builder-order-publish-refused.png');
  await mark.getByRole('button', { name: 'Validate definition' }).click();
  await expect(mark.getByRole('tabpanel')).toContainText('Move the decision step below the exhibition step');
  await capture(mark, 'n03-builder-order-validation.png');
});
