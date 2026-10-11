import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-26 — Retention in the browser (BPMS-066).

test.describe('retention @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-26 an admin sees each record class, a draft due for deletion, and shortens the draft period with a reason', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const draft = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.old', title: 'Old cleaning rota', content: { blocks: [{ type: 'paragraph', text: 'x' }] } } })).json();
    await request.post('/__test/retention/backdate', { data: { workspace: ws.id, version: draft.version.id, seconds: 170 * 86_400 } });

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Retention');
    await expect(page.getByRole('heading', { level: 1, name: 'Retention' })).toBeVisible();
    const classes = page.getByRole('table', { name: 'Retention classes' });
    await expect(classes.getByRole('row', { name: /Drafts.*180 days.*Deleted automatically/ })).toBeVisible();
    await expect(classes.getByRole('row', { name: /Audit trail.*Never deleted by Procera/ })).toBeVisible();
    await expect(page.getByRole('table', { name: 'Drafts due for deletion' }).getByRole('link', { name: 'Old cleaning rota' })).toBeVisible();
    await axe(page);

    await page.getByRole('button', { name: 'Change retention for drafts' }).click();
    const dialog = page.getByRole('dialog', { name: 'Retention for drafts' });
    await dialog.getByLabel('Keep for (days)').fill('90');
    await dialog.getByLabel('Reason').fill('Drafts go stale faster here');
    await dialog.getByRole('button', { name: 'Save' }).click();
    await expect(classes.getByRole('row', { name: /Drafts.*90 days/ })).toBeVisible();
  });
});
