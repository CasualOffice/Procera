import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-39 — background audit export from the Audit log (BPMS-064).

test.describe('audit export @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-39 an admin starts a background export with the current filters (content events) and downloads it when ready', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'x.ui', title: 'For export', content: { blocks: [] } } });
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Audit log');
    await page.getByRole('form', { name: 'Filter events' }).getByLabel('Category').selectOption({ label: 'Content' });
    await page.getByRole('button', { name: 'Export', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Export the audit log' });
    await expect(dialog).toContainText('Uses the current filters');
    await axe(page);
    await dialog.getByRole('button', { name: 'Start export' }).click();
    const exports = page.getByRole('region', { name: 'Your exports' });
    await expect(exports).toBeVisible();
    await expect.poll(async () => { await request.post('/__test/audit/exports/run'); return exports.getByText('Ready').count(); }, { timeout: 30_000 }).toBeGreaterThan(0);
    const download = page.waitForEvent('download');
    await exports.getByRole('button', { name: /^Download export/ }).first().click();
    expect((await download).suggestedFilename()).toMatch(/^audit-export-\d+\.csv$/);
  });
});
