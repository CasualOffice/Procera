import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { EICAR, PNG_1PX, upload } from '../../fixtures/files';
import { axe, nav, signIn, WEB } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-32..34, 43 — files in the browser (BPMS-060/061/062).

test.describe('files @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-32 an author attaches a file to a draft and downloads it', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.rota', title: 'Cleaning rota', content: { blocks: [{ type: 'paragraph', text: 'See the rota.' }] } } })).json();
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await page.locator('#att-input').setInputFiles({ name: 'rota.txt', mimeType: 'text/plain', buffer: Buffer.from('Mon: Ana\nTue: Ben\n') });
    await expect(page.getByText('rota.txt attached.')).toBeVisible();
    const list = page.getByRole('region', { name: 'Attachments' });
    await expect(list.getByRole('listitem').filter({ hasText: 'rota.txt' })).toBeVisible();
    await expect(list.getByText('Clean')).toBeVisible();
    await axe(page);
    const download = page.waitForEvent('download');
    await list.getByRole('button', { name: 'Download rota.txt' }).click();
    expect((await download).suggestedFilename()).toBe('rota.txt');
  });

  test('E2E-R1-UI-33 a quarantined upload is refused on the document and shown as quarantined in Files', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.inv', title: 'Invoices', content: { blocks: [{ type: 'paragraph', text: 'x' }] } } })).json();
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await page.locator('#att-input').setInputFiles({ name: 'invoice.txt', mimeType: 'text/plain', buffer: Buffer.from(EICAR) });
    await expect(page.getByText(/invoice\.txt was quarantined/)).toBeVisible();
    await nav(page, 'Files');
    await expect(page.getByRole('heading', { level: 1, name: 'Files' })).toBeVisible();
    const row = page.getByRole('table', { name: 'Files' }).getByRole('row', { name: /invoice\.txt/ });
    await expect(row).toContainText('Quarantined');
    await expect(row.getByRole('button', { name: /^Download/ })).toHaveCount(0);
    await axe(page);
  });

  test('E2E-R1-UI-34 an author previews an image attachment in a dialog', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.layout', title: 'Floor layout', content: { blocks: [{ type: 'paragraph', text: 'See the diagram.' }] } } })).json();
    const png = await upload(owner, 'diagram.png', PNG_1PX);
    const txt = await upload(owner, 'notes.txt', Buffer.from('plain words'));
    for (const f of [png, txt]) {
      expect((await owner.c.post(`/api/v1/files/${f.id}/links`, { headers: owner.headers, data: { object_type: 'artifact_version', object_id: r.version.id } })).status()).toBe(204);
    }
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    const list = page.getByRole('region', { name: 'Attachments' });
    await expect(list.getByRole('listitem').filter({ hasText: 'diagram.png' })).toBeVisible();
    // Every clean file offers Preview; a type without one shows the download fallback (E2E-R1-UI-69).
    await expect(list.getByRole('button', { name: 'Preview notes.txt' })).toBeVisible();
    const trigger = list.getByRole('button', { name: 'Preview diagram.png' });
    await trigger.click();
    const dialog = page.getByRole('dialog', { name: 'diagram.png' });
    await expect(dialog).toBeVisible();
    const img = dialog.getByRole('img', { name: 'Preview of diagram.png' });
    await expect(img).toBeVisible();
    await expect.poll(() => img.evaluate((el: HTMLImageElement) => el.complete && el.naturalWidth)).toBe(1);
    await expect(img).toHaveAttribute('src', /^\/api\/v1\/files\/preview\//);
    await axe(page);
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);
    await expect(trigger).toBeFocused();
  });

  test('E2E-R1-UI-43 Files shows retention class and last access per file; a download updates it; unattached files are deleted in bulk', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: 'ops.keys', title: 'Key handover', content: { blocks: [{ type: 'paragraph', text: 'Keys go to reception.' }] } } })).json();
    const loose = [await upload(owner, 'scratch-a.txt', Buffer.from('a')), await upload(owner, 'scratch-b.txt', Buffer.from('b'))];
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await page.locator('#att-input').setInputFiles({ name: 'keys.txt', mimeType: 'text/plain', buffer: Buffer.from('Reception desk') });
    await expect(page.getByText('keys.txt attached.')).toBeVisible();

    await nav(page, 'Files');
    const grid = page.getByRole('table', { name: 'Files' });
    const keys = grid.getByRole('row', { name: /keys\.txt/ });
    await expect(keys).toContainText('Draft');
    await expect(keys).toContainText('Never');
    await expect(grid.getByRole('row', { name: /scratch-a\.txt/ })).toContainText('Not attached');
    const download = page.waitForEvent('download');
    await keys.getByRole('button', { name: 'Download keys.txt' }).click();
    await download;
    await expect(keys).toContainText('just now');

    for (const f of loose) await grid.getByRole('checkbox', { name: `Select row ${f.id}` }).click();
    await page.getByRole('button', { name: 'Delete 2' }).click();
    await expect(page.getByText('2 files deleted.')).toBeVisible();
    await expect(grid.getByRole('row', { name: /scratch-/ })).toHaveCount(0);
    await expect(keys).toBeVisible();
    await axe(page);
  });
});
