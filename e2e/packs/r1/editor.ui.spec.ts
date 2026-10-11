import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, signIn } from '../../fixtures/ui';
import { WEB } from '../../fixtures/ui';
import { PNG_1PX } from '../../fixtures/files';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-28 — the block editor (BPMS-042): toolbar formatting, numbered steps, a callout and a table, saved and rendered.

test.describe('block editor @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-28 an author formats with the toolbar (bold, numbered steps, callout, table), saves, and the document renders them', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.fridge', title: 'Fridge cleaning', content: { blocks: [] } } })).json();
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible(); // signed in
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    const editor = page.getByRole('textbox', { name: 'Content' });
    const bar = page.getByRole('toolbar', { name: 'Formatting' });
    await editor.click();
    await page.keyboard.type('Clean every ');
    await bar.getByRole('button', { name: 'Bold' }).click();
    await page.keyboard.type('Friday');
    await bar.getByRole('button', { name: 'Bold' }).click();
    await expect(bar.getByRole('button', { name: 'Bold' })).toHaveAttribute('aria-pressed', 'false');
    await page.keyboard.press('Enter');
    await bar.getByRole('button', { name: 'Numbered steps' }).click();
    await page.keyboard.type('Empty the shelves');
    await page.keyboard.press('Enter');
    await page.keyboard.type('Wipe with vinegar');
    await page.keyboard.press('Enter');
    await page.keyboard.press('Enter'); // leave the list
    await bar.getByRole('button', { name: 'Callout' }).click();
    await page.keyboard.type('Bin anything without a name.');
    await page.keyboard.press('Enter');
    await page.keyboard.press('Enter');
    await bar.getByRole('button', { name: 'Table' }).click();
    await page.keyboard.type('Week');
    await page.keyboard.press('Tab');
    await page.keyboard.type('Who');
    await axe(page);
    await page.getByRole('button', { name: 'Save draft' }).click();
    await expect(page.getByText('Saved.')).toBeVisible();

    await expect(page.locator('.doc-body strong', { hasText: 'Friday' })).toBeVisible();
    await expect(page.locator('.doc-body ol li')).toHaveText(['Empty the shelves', 'Wipe with vinegar']);
    await expect(page.getByRole('complementary', { name: 'Note' })).toContainText('Bin anything without a name.');
    await expect(page.getByRole('columnheader', { name: 'Week' })).toBeVisible();
    await axe(page);
    const saved = await (await owner.c.get(`/api/v1/versions/${r.version.id}`, { headers: owner.headers })).json();
    expect(saved.content.blocks.map((b: { type: string }) => b.type)).toEqual(['paragraph', 'list', 'callout', 'table']);
    expect(saved.content.blocks[0].runs).toEqual([{ text: 'Clean every ' }, { text: 'Friday', bold: true }]);
  });

  test('E2E-R1-UI-44 a reader exports a published SOP from the Export menu as a Word document', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.lockup', title: 'Lock-up', content: { blocks: [{ type: 'heading', level: 2, text: 'Steps' }, { type: 'list', ordered: true, items: ['Close windows', 'Set alarm'] }] } } })).json();
    await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: owner.headers });
    await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: owner.headers, data: {} });
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await page.getByRole('button', { name: 'Export' }).click();
    const menu = page.getByRole('menu', { name: 'Export' });
    await expect(menu.getByRole('menuitem')).toHaveText(['Print or PDF', 'Word document (.docx)', 'Markdown (.md)']);
    await axe(page);
    const download = page.waitForEvent('download');
    await menu.getByRole('menuitem', { name: /Word document/ }).click();
    const file = await download;
    expect(file.suggestedFilename()).toBe('ops.lockup-v1.docx');
    const path = await file.path();
    const head = (await import('node:fs')).readFileSync(path!).subarray(0, 2).toString('latin1');
    expect(head).toBe('PK');
  });

  test('E2E-R1-UI-45 an author uploads a screenshot from the editor, describes it, saves, and readers see the image with its description', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.printer', title: 'Printer setup', content: { blocks: [{ type: 'paragraph', text: 'Open the settings.' }] } } })).json();
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    await page.getByRole('textbox', { name: 'Content' }).click();
    await page.keyboard.press('End');
    await page.getByRole('toolbar', { name: 'Formatting' }).getByRole('button', { name: 'Image' }).click();
    const dlg = page.getByRole('dialog', { name: 'Insert image' });
    await expect(dlg.getByText('No images on this draft yet')).toBeVisible();
    await dlg.locator('#img-input').setInputFiles({ name: 'settings.png', mimeType: 'image/png', buffer: PNG_1PX });
    await expect(dlg.getByRole('button', { name: 'settings.png' })).toHaveAttribute('aria-pressed', 'true');
    await expect(dlg.getByRole('button', { name: 'Insert' })).toBeDisabled(); // a description is required
    await dlg.getByLabel(/^Description/).fill('Printer settings screen with the Network tab open');
    await dlg.getByLabel('Caption (optional)').fill('Step 2');
    await axe(page);
    await dlg.getByRole('button', { name: 'Insert' }).click();
    await expect(page.getByRole('textbox', { name: 'Content' }).getByRole('img', { name: 'Printer settings screen with the Network tab open' })).toBeVisible();
    await page.getByRole('button', { name: 'Save draft' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible();
    await expect(page.getByRole('textbox', { name: 'Content' })).toHaveCount(0); // out of the editor: the reader view
    await expect(page.getByRole('img', { name: 'Printer settings screen with the Network tab open' })).toBeVisible();
    await expect(page.getByText('Step 2')).toBeVisible();
    await page.reload();
    await expect(page.getByRole('img', { name: 'Printer settings screen with the Network tab open' })).toBeVisible();
  });
});
