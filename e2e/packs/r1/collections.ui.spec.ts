import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-27 — collections, owner and RACI in the browser (BPMS-041).

test.describe('collections @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-27 an admin creates a collection, files a document in it, filters the library by it, and sets the RACI', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.keys', title: 'Key handover', content: { blocks: [{ type: 'paragraph', text: 'Sign the log.' }] } } })).json();
    await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.misc', title: 'Unfiled note', content: { blocks: [{ type: 'paragraph', text: 'x' }] } } });

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Library');
    const tree = page.getByRole('navigation', { name: 'Collections' });
    await tree.getByRole('button', { name: 'New collection' }).click();
    const dialog = page.getByRole('dialog', { name: 'New collection' });
    await dialog.getByLabel('Name').fill('Facilities');
    await dialog.getByRole('button', { name: 'Create' }).click();
    await expect(tree.getByRole('link', { name: /Facilities/ })).toBeVisible();
    await axe(page);

    await page.goto(page.url().replace(/\/library.*$/, `/library/${r.artifact.id}`));
    await page.getByRole('button', { name: 'Change collection' }).click();
    const move = page.getByRole('dialog', { name: 'Move to a collection' });
    await move.getByRole('button', { name: /Collection/ }).click();
    await page.getByRole('option', { name: 'Facilities' }).click();
    await move.getByRole('button', { name: 'Move' }).click();
    await expect(page.locator('dd').filter({ hasText: 'Facilities' })).toBeVisible(); // the Collection property

    await page.getByRole('button', { name: 'Edit RACI' }).click();
    const raci = page.getByRole('dialog', { name: 'RACI' });
    await raci.getByRole('button', { name: /^Accountable/ }).click();
    await raci.getByText('Olivia', { exact: true }).click();
    await raci.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText(/RACI saved/)).toBeVisible();
    await axe(page);

    await nav(page, 'Library');
    await tree.getByRole('link', { name: /Facilities/ }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Facilities' })).toBeVisible();
    const table = page.getByRole('table', { name: 'Documents' });
    await expect(table.getByRole('link', { name: 'Key handover' })).toBeVisible();
    await expect(table.getByRole('link', { name: 'Unfiled note' })).toHaveCount(0);
  });
});
