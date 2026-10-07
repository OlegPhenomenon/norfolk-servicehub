import AxeBuilder from '@axe-core/playwright';
import { test, expect, login, ready, openSeededCase } from '../helpers';

const visits = [
  ['/', null], ['/services', null], ['/services/rawson-hall-hire', null], ['/demo', null],
  ['/my', 'Alexey'], ['/my/cases/<id>', 'Alexey'], ['/staff', 'Olga'], ['/staff/cases/<id>', 'Olga'],
  ['/staff/field', 'Jake'], ['/staff/finance', 'Tom'], ['/staff/dashboard', 'Helen'],
  ['/admin/services', 'Mark'], ['/notices', null], ['/notices/<id>', null], ['/map', null],
] as const;
for (const [path, persona] of visits) {
  test(`axe: ${path}`, async ({ page }, info) => {
    if (path === '/staff/field') await page.setViewportSize({ width: 390, height: 844 });
    if (persona) await login(page, persona);
    if (path === '/notices/<id>') {
      await page.goto('/notices');
      await page.getByRole('link', { name: 'Read notice and published documents' }).first().click();
      await page.locator('summary').first().click();
      const plan = page.getByRole('img', { name: /published copy with approved redactions/ }).first();
      await expect.poll(() => plan.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0)).toBe(true);
    } else if (path.includes('<id>')) await openSeededCase(page, path.startsWith('/my') ? 'my' : 'staff');
    else await page.goto(path);
    await ready(page);
    // Ensure the audit includes the seeded content, not merely the loading shell.
    await expect(page.locator('main')).not.toContainText('Loading…');
    const result = await new AxeBuilder({ page }).analyze();
    const violations = result.violations.filter(v => v.impact === 'serious' || v.impact === 'critical');
    await info.attach('axe-results', { body: JSON.stringify(result, null, 2), contentType: 'application/json' });
    expect(violations.map(v => ({ id: v.id, impact: v.impact, description: v.description,
      nodes: v.nodes.map(n => ({ target: n.target, summary: n.failureSummary })) }))).toEqual([]);
  });
}
