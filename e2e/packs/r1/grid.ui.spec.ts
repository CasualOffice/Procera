import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-38 — the shared DataGrid on the Library (BPMS-028): filter builder and chips, sort, saved view, URL state, export.

test.describe('data grid @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-38 filter by status, sort, save a view that comes back after a reload, and export the rows', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    for (const [key, title, publish] of [['a.one', 'Alpha policy', true], ['b.two', 'Bravo policy', true], ['c.three', 'Charlie draft', false]] as const) {
      const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key, title, content: { blocks: [{ type: 'paragraph', text: 'x' }] } } })).json();
      if (publish) {
        await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h });
        await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} });
      }
    }
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Library');
    const table = page.getByRole('table', { name: 'Documents' });
    await expect(table.getByRole('row')).toHaveCount(4);

    await page.getByRole('button', { name: /^Filter/ }).click();
    const builder = page.getByRole('form', { name: 'Add a filter' });
    await builder.getByLabel('Field').selectOption({ label: 'Status' });
    await builder.getByLabel('Published').check();
    await axe(page);
    await builder.getByRole('button', { name: 'Add filter' }).click();
    await expect(page.getByRole('group', { name: 'Active filters' })).toContainText('Status is any of “Published”');
    await expect(table.getByRole('row')).toHaveCount(3);
    await expect(page.getByRole('status').filter({ hasText: 'rows' })).toHaveText('2 of 3 rows');

    await table.getByRole('columnheader', { name: /Name/ }).getByRole('button').click(); // asc (already) → desc
    await expect(table.getByRole('row').nth(1)).toContainText('Bravo policy');

    await page.getByRole('button', { name: 'Save view' }).click();
    const dialog = page.getByRole('dialog', { name: 'Save view' });
    await dialog.getByLabel('Name').fill('Published, Z to A');
    await dialog.getByRole('button', { name: 'Save view' }).click();
    await expect(page.getByRole('combobox', { name: 'View', exact: true })).toHaveValue(/.+/);

    await page.reload();
    await expect(page.getByRole('group', { name: 'Active filters' })).toContainText('Status is any of “Published”');
    await expect(table.getByRole('row').nth(1)).toContainText('Bravo policy');
    await expect(page.getByText('Modified')).toHaveCount(0);

    const download = page.waitForEvent('download');
    await page.getByRole('button', { name: 'Export', exact: true }).click();
    const csv = await (await download).createReadStream();
    let text = '';
    for await (const chunk of csv) text += chunk;
    expect(text.split('\n')[0]).toBe('"Name","Type","Status","Owner","Criticality","Created"');
    expect(text).toContain('"Bravo policy"');
    expect(text).not.toContain('Charlie draft');
  });
});
