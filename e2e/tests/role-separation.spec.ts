import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { Page } from '@playwright/test';
import { test, expect, login, ready } from '../helpers';

const pdf = readFileSync(resolve(import.meta.dirname, '../../server/seed-data/docs/site-plan.pdf'));

async function call(page: Page, method: string, url: string, data?: unknown) {
  const me = await (await page.request.get('/api/me')).json();
  const res = await page.request.fetch(url, { method, data, headers: { 'X-CSRF-Token': me.csrf_token ?? '', 'Idempotency-Key': crypto.randomUUID() } });
  if (!res.ok()) throw new Error(`${method} ${url}: ${res.status()} ${await res.text()}`);
  const text = await res.text();
  return text ? JSON.parse(text) : null;
}
/** Alexey lodges a planning certificate request through the API the form uses (setup only). */
async function lodge(page: Page): Promise<number> {
  const def = (await call(page, 'GET', '/api/public/services/planning-certificate')).definition;
  const answers: Record<string, unknown> = {};
  for (const f of def.fields) {
    if (!f.required) continue;
    answers[f.key] = f.type === 'checkbox' ? true : f.type === 'number' ? 1 : f.type === 'select' ? f.options[0].value : f.type === 'multiselect' ? [f.options[0].value] : f.type === 'email' ? 'alexey@example.invalid' : f.type === 'property_ref' ? 'Portion DEMO-78, Taylors Road' : 'Fictional role separation check';
  }
  const id = (await call(page, 'POST', '/api/services/planning-certificate/drafts', { applicant_org_id: null })).id;
  await call(page, 'PUT', `/api/cases/${id}/draft`, { answers });
  const me = await (await page.request.get('/api/me')).json();
  for (const doc of def.documents.filter((d: { required: boolean }) => d.required)) {
    const res = await page.request.post(`/api/cases/${id}/documents`, { headers: { 'X-CSRF-Token': me.csrf_token }, multipart: { requirement_key: doc.key, title: doc.label, file: { name: 'plan.pdf', mimeType: 'application/pdf', buffer: pdf } } });
    expect(res.ok(), await res.text()).toBe(true);
  }
  await call(page, 'POST', `/api/cases/${id}/submit`, {});
  return id;
}

// The shared Money tab: the applicant pays online; staff only see what the applicant still owes.
test('Money tab: applicant pays, staff see the applicant balance without a Pay button', async ({ page, browser, baseURL }) => {
  test.setTimeout(120000);
  await login(page, 'Alexey');
  const id = await lodge(page);
  const staff = await browser.newContext({ baseURL });
  try {
    const olga = await staff.newPage();
    await login(olga, 'Olga');
    const revision = (await call(olga, 'GET', `/api/cases/${id}`)).case.revision;
    await call(olga, 'POST', `/api/cases/${id}/actions/advance`, { expected_revision: revision });
    const invoice = (await call(olga, 'GET', `/api/cases/${id}/money`)).invoices.find((i: { kind: string; outstanding_cents: number }) => i.kind === 'invoice' && i.outstanding_cents > 0);
    expect(invoice, 'the payment step invoices the request').toBeDefined();
    await olga.goto(`/staff/cases/${id}?tab=finance.money`);
    await ready(olga);
    await expect(olga.getByRole('heading', { name: "Applicant's balance" })).toBeVisible();
    await expect(olga.getByText('Awaiting payment from applicant').first()).toBeVisible();
    await expect(olga.getByRole('button', { name: /^Pay / })).toHaveCount(0);
    await expect(olga.getByRole('heading', { name: 'Your balance' })).toHaveCount(0);
    const me = await (await olga.request.get('/api/me')).json();
    const refused = await olga.request.post(`/api/cases/${id}/checkout`, { data: { invoice_id: invoice.id }, headers: { 'X-CSRF-Token': me.csrf_token } });
    expect(refused.status()).toBe(403);

    await page.goto(`/my/cases/${id}?tab=finance.money`);
    await ready(page);
    await expect(page.getByRole('heading', { name: 'Your balance' })).toBeVisible();
    await expect(page.getByRole('button', { name: /^Pay / })).toBeVisible();
  } finally { await staff.close(); }
});
