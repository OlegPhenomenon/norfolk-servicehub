import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Browser, Page } from '@playwright/test';
import { test, expect, login, advance, ready } from '../helpers';

// Audit-2 slice A in the browser: fee assessment and payment gate, DA-only scope, and a public exhibition that
// blocks the request until it has closed and every comment has a recorded consideration outcome.
const pdf = readFileSync(resolve(import.meta.dirname, '../../server/seed-data/docs/site-plan.pdf'));

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
/** Alexey lodges a development application through the same API the form uses (setup only). */
async function lodge(page: Page, approvals: string[], cost: number): Promise<number> {
  const def = (await call(page, 'GET', '/api/public/services/development-application')).definition;
  const answers: Record<string, unknown> = {};
  for (const f of def.fields) {
    if (!f.required) continue;
    answers[f.key] = f.type === 'checkbox' ? true : f.type === 'number' ? 100 : f.type === 'select' ? f.options[0].value : f.type === 'multiselect' ? [f.options[0].value] : f.key === 'applicant_name' ? 'Alexey Turner' : 'Fictional browser demonstration';
  }
  Object.assign(answers, { approvals_sought: approvals, estimated_cost: cost, property_ref: 'Portion DEMO-77, Taylors Road' });
  const id = (await call(page, 'POST', '/api/services/development-application/drafts', { applicant_org_id: null })).id;
  await call(page, 'PUT', `/api/cases/${id}/draft`, { answers });
  const me = await (await page.request.get('/api/me')).json();
  for (const doc of def.documents.filter((d: { required: boolean }) => d.required)) {
    const res = await page.request.post(`/api/cases/${id}/documents`, { headers: { 'X-CSRF-Token': me.csrf_token }, multipart: { requirement_key: doc.key, title: doc.label, file: { name: 'plan.pdf', mimeType: 'application/pdf', buffer: pdf } } });
    expect(res.ok(), await res.text()).toBe(true);
  }
  await call(page, 'POST', `/api/cases/${id}/submit`, {});
  return id;
}
async function staff(browser: Browser, baseURL: string | undefined, name: string) {
  const context = await browser.newContext({ baseURL });
  const page = await context.newPage();
  await login(page, name);
  return { context, page };
}
async function routeTab(page: Page, id: number) {
  await page.goto(`/staff/cases/${id}?tab=documents.route`);
  await ready(page);
}

test('fee assessment and payment gate, then a DA-only scope completes with one decision', async ({ page, browser, baseURL }) => {
  test.setTimeout(240000);
  await login(page, 'Alexey');
  const id = await lodge(page, ['development_approval', 'building_approval'], 120000);
  const olga = await staff(browser, baseURL, 'Olga'), priya = await staff(browser, baseURL, 'Priya'), helen = await staff(browser, baseURL, 'Helen');
  try {
    await routeTab(olga.page, id);
    await advance(olga.page); // Check request → Determine fees
    await expect(olga.page.getByText('Record the fee assessment', { exact: false }).first()).toBeVisible();
    await olga.page.getByRole('tab', { name: 'Fees, scope and exhibition', exact: true }).click();
    await expect(olga.page.getByText('System calculation: Building Development and Works scale')).toBeVisible();
    await expect(olga.page.getByText('$880.00', { exact: false }).first()).toBeVisible();
    await olga.page.getByRole('button', { name: 'Record fee assessment', exact: true }).click();
    await expect(olga.page.getByRole('list', { name: 'Fee assessment history' })).toContainText('Assessment v1');
    await advance(olga.page); // → Receive payment: the invoice is issued
    // Unpaid: the request cannot pass the payment step.
    await olga.page.reload();
    await expect(olga.page.getByText('Payment has not been received yet.', { exact: false }).first()).toBeVisible();

    await page.goto(`/my/cases/${id}?tab=finance.money`);
    await page.getByRole('button', { name: /^Pay / }).click();
    await expect(page).toHaveURL(/\/mock\/pay\/checkout\//);
    await page.getByRole('button', { name: 'Pay with test card', exact: true }).click();
    await expect(page.getByText('Payment confirmed', { exact: true })).toBeVisible({ timeout: 45000 });

    // Priya confirms the scope as development approval only, with a reason.
    await routeTab(priya.page, id);
    await priya.page.getByRole('checkbox', { name: 'Building approval', exact: true }).uncheck();
    await priya.page.getByLabel('Reason', { exact: true }).fill('Fictional: the works are exempt from building approval.');
    await priya.page.getByRole('button', { name: 'Confirm approval scope', exact: true }).click();
    await expect(priya.page.getByText('Confirmed by Council', { exact: true })).toBeVisible();
    await advance(priya.page); // Assess application → Public exhibition
    await priya.page.getByRole('tab', { name: 'Fees, scope and exhibition', exact: true }).click();
    await priya.page.getByLabel('Reason exhibition is not required').fill('Fictional: notified by letter; no comment period applies.');
    await priya.page.getByRole('button', { name: 'Record exhibition not required', exact: true }).click();
    await expect(priya.page.getByText('Exhibition not required for this request', { exact: true })).toBeVisible();
    await advance(priya.page); // → Issue decisions

    await priya.page.getByRole('tab', { name: 'Decisions', exact: true }).click();
    await priya.page.getByText('Prepare a decision', { exact: true }).click();
    const type = priya.page.getByLabel('Decision type', { exact: false });
    await expect(type.locator('option', { hasText: 'building approval' })).toHaveCount(0);
    await type.selectOption('development_approval');
    await priya.page.getByLabel('Versioned template', { exact: false }).selectOption({ index: 1 });
    await priya.page.getByLabel('Reasons / certificate information', { exact: false }).fill('Fictional assessment: complies.');
    await priya.page.getByRole('button', { name: 'Save draft', exact: true }).click();
    await priya.page.getByRole('button', { name: 'Submit for approval', exact: true }).click();
    await expect(priya.page.getByText('Pending approval', { exact: true }).first()).toBeVisible();

    await helen.page.goto(`/staff/cases/${id}?tab=documents.decisions`);
    await helen.page.getByRole('button', { name: 'Issue decision and PDF', exact: true }).click();
    await expect(helen.page.getByRole('link', { name: 'Download issued decision' })).toBeVisible();
    await page.goto(`/my/cases/${id}`);
    await expect(page.getByRole('heading', { level: 1 }).locator('..')).toContainText('Completed');
  } finally { await Promise.all([olga.context.close(), priya.context.close(), helen.context.close()]); }
});

test('an open exhibition blocks the request until it closes and every comment is considered', async ({ page, browser, baseURL }) => {
  test.setTimeout(240000);
  await login(page, 'Alexey');
  const id = await lodge(page, ['development_approval'], 50000);
  const olga = await staff(browser, baseURL, 'Olga'), priya = await staff(browser, baseURL, 'Priya'), helen = await staff(browser, baseURL, 'Helen'), tom = await staff(browser, baseURL, 'Tom');
  try {
    // Setup through the API: fee assessed, a recorded manager exemption, scope confirmed.
    await call(olga.page, 'POST', `/api/cases/${id}/actions/advance`, { expected_revision: await revision(olga.page, id) });
    await call(olga.page, 'POST', `/api/cases/${id}/building-fee`, { method: 'schedule', expected_revision: await revision(olga.page, id) });
    await call(helen.page, 'POST', `/api/cases/${id}/price-waivers`, { item_code: 'BUILDING_WORKS_FEE', amount_cents: 57000, reason: 'Fictional community exemption', expected_revision: await revision(helen.page, id) });
    await call(olga.page, 'POST', `/api/cases/${id}/actions/advance`, { expected_revision: await revision(olga.page, id) });
    await call(tom.page, 'POST', `/api/cases/${id}/actions/advance`, { expected_revision: await revision(tom.page, id) });
    await call(priya.page, 'POST', `/api/cases/${id}/approval-scope`, { approvals: ['development_approval'], reason: 'Fictional: DA only.', expected_revision: await revision(priya.page, id) });
    await call(priya.page, 'POST', `/api/cases/${id}/actions/advance`, { expected_revision: await revision(priya.page, id) });
    // A short real-time exhibition window, published by a second staff member.
    const docs = await call(priya.page, 'GET', `/api/cases/${id}/documents`);
    const now = Date.now();
    const exhibit = (await call(priya.page, 'POST', '/api/exhibitions', { case_id: id, title: 'Browser exhibition', summary: 'Fictional proposal on exhibition.', opens_at: new Date(now - 60000).toISOString(), closes_at: new Date(now + 45000).toISOString(), expected_revision: await revision(priya.page, id) })).id;
    await call(priya.page, 'POST', `/api/exhibitions/${exhibit}/items`, { source_document_version_id: docs[0].versions[0].id, title: 'Plan', redactions: [], expected_revision: await revision(priya.page, id) });
    await call(helen.page, 'POST', `/api/exhibitions/${exhibit}/publish`, { expected_revision: await revision(helen.page, id) });

    // A member of the public comments through the public notice.
    const visitor = await browser.newContext({ baseURL });
    const publicPage = await visitor.newPage();
    await publicPage.goto(`/notices/${exhibit}`);
    await publicPage.getByLabel('Name', { exact: true }).fill('Fictional neighbour');
    await publicPage.getByLabel('Email', { exact: true }).fill('neighbour@example.invalid');
    await publicPage.getByLabel('Written submission').fill('Fictional concern about stormwater runoff.');
    await publicPage.getByRole('button', { name: /Send/ }).click();
    await expect(publicPage.getByText('Submission received', { exact: true })).toBeVisible();
    await visitor.close();

    // The exhibition step cannot be skipped or completed while the window is open; decisions cannot be issued.
    await priya.page.goto(`/staff/cases/${id}`);
    await ready(priya.page);
    await expect(priya.page.getByText('open until', { exact: false }).first()).toBeVisible();
    await expect(priya.page.getByRole('button', { name: 'Skip optional step' })).toHaveCount(0);
    await priya.page.getByRole('button', { name: 'Complete this step', exact: true }).click();
    await priya.page.getByRole('dialog').getByLabel('Reason or completion note').fill('Trying to finish early.');
    await priya.page.getByRole('button', { name: 'Confirm action', exact: true }).click();
    await expect(priya.page.getByRole('dialog').getByText('open until', { exact: false })).toBeVisible();
    await priya.page.getByRole('dialog').getByRole('button', { name: 'Back', exact: true }).click();
    const templates = await call(priya.page, 'GET', '/api/decision-templates');
    const template = templates.find((t: { decision_type: string }) => t.decision_type === 'development_approval').id;
    const decision = (await call(priya.page, 'POST', `/api/cases/${id}/decisions`, { decision_type: 'development_approval', outcome: 'approved', reasons: 'Fictional assessment.', conditions: '', template_id: template, expected_revision: await revision(priya.page, id) })).id;
    await call(priya.page, 'POST', `/api/cases/${id}/decisions/${decision}/submit`, { expected_revision: await revision(priya.page, id) });
    await helen.page.goto(`/staff/cases/${id}?tab=documents.decisions`);
    await helen.page.getByRole('button', { name: 'Issue decision and PDF', exact: true }).click();
    await expect(helen.page.getByText('before issuing decisions', { exact: false }).first()).toBeVisible();

    // After the window closes the comment still needs a recorded consideration outcome.
    await expect.poll(async () => {
      await priya.page.reload();
      return priya.page.getByText('need a recorded consideration outcome', { exact: false }).count();
    }, { timeout: 90000, intervals: [3000] }).toBeGreaterThan(0);
    await priya.page.goto(`/staff/exhibitions/${exhibit}`);
    await ready(priya.page);
    await priya.page.getByLabel('Consideration outcome').fill('Fictional: stormwater is addressed by a condition.');
    await priya.page.getByRole('button', { name: 'Record consideration', exact: true }).click();
    await expect(priya.page.getByText('Outcome: Fictional: stormwater is addressed by a condition.', { exact: false })).toBeVisible();
    await priya.page.goto(`/staff/cases/${id}`);
    await ready(priya.page);
    await advance(priya.page); // → Issue decisions
    await helen.page.reload();
    await helen.page.getByRole('button', { name: 'Issue decision and PDF', exact: true }).click();
    await expect(helen.page.getByRole('link', { name: 'Download issued decision' })).toBeVisible();
    await page.goto(`/my/cases/${id}`);
    await expect(page.getByRole('heading', { level: 1 }).locator('..')).toContainText('Completed');
  } finally { await Promise.all([olga.context.close(), priya.context.close(), helen.context.close(), tom.context.close()]); }
});
