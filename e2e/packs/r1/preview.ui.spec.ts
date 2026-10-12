import { type Page, expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { pdfPages, tinyDocx, tinyXlsx, upload, zipStored } from '../../fixtures/files';
import { axe, signIn, WEB } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-66..69 — in-app document preview: pdf.js, DOCX, XLSX/CSV and the fallback (BPMS-061).

type Owner = Awaited<ReturnType<typeof workspaceWithOwner>>;

/** A draft SOP with these files attached, opened in the browser; returns the Attachments region. */
async function docWith(page: Page, w: Owner, files: [string, Buffer][]) {
  const { ownerEmail, owner } = w;
  const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'sop', key: `ops.p${Date.now()}`, title: 'Preview test', content: { blocks: [{ type: 'paragraph', text: 'See the files.' }] } } })).json();
  const uploaded = [];
  for (const [name, bytes] of files) {
    const f = await upload(owner, name, bytes);
    expect(f.state).toBe('clean');
    expect((await owner.c.post(`/api/v1/files/${f.id}/links`, { headers: owner.headers, data: { object_type: 'artifact_version', object_id: r.version.id } })).status()).toBe(204);
    uploaded.push(f);
  }
  await signIn(page, ownerEmail, owner.recovery![0]!);
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
  await page.goto(`${WEB}/library/${r.artifact.id}`);
  const list = page.getByRole('region', { name: 'Attachments' });
  await expect(list.getByRole('listitem')).toHaveCount(files.length);
  return { list, uploaded };
}

test.describe('document preview @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-66 a PDF previews with pdf.js: page navigation, zoom, search and keyboard', async ({ page, request }) => {
    const { list } = await docWith(page, await workspaceWithOwner(request), [['handbook.pdf', pdfPages(['Welcome aboard', 'Holiday rules'])]]);
    await list.getByRole('button', { name: 'Preview handbook.pdf' }).click();
    const dialog = page.getByRole('dialog', { name: 'handbook.pdf' });
    const bar = dialog.getByRole('toolbar', { name: 'PDF controls' });
    await expect(bar).toContainText('Page 1 of 2');
    // The text layer carries the page's words (selectable, readable by assistive technology).
    await expect(dialog.getByText('Welcome aboard')).toBeAttached();
    await expect(bar.getByRole('button', { name: 'Previous page' })).toBeDisabled();
    await bar.getByRole('button', { name: 'Next page' }).click();
    await expect(bar).toContainText('Page 2 of 2');
    await expect(dialog.getByText('Holiday rules')).toBeAttached();

    await bar.getByRole('button', { name: '100%' }).click();
    await expect(bar.getByRole('button', { name: '100%' })).toHaveAttribute('aria-pressed', 'true');
    await bar.getByRole('button', { name: 'Zoom in' }).click();
    await expect(bar).toContainText('125%');
    await bar.getByRole('button', { name: 'Zoom out' }).click();
    await bar.getByRole('button', { name: 'Zoom out' }).click();
    await expect(bar).toContainText('75%');
    await bar.getByRole('button', { name: 'Fit width' }).click();
    await expect(bar.getByRole('button', { name: 'Fit width' })).toHaveAttribute('aria-pressed', 'true');

    // Keyboard: the page region takes focus and the arrow keys turn pages.
    const pages = dialog.getByRole('region', { name: /handbook\.pdf, page/ });
    await pages.focus();
    await page.keyboard.press('ArrowLeft');
    await expect(bar).toContainText('Page 1 of 2');
    await page.keyboard.press('End');
    await expect(bar).toContainText('Page 2 of 2');

    // Search jumps to the page that has the words.
    await page.keyboard.press('Home');
    await bar.getByRole('searchbox', { name: 'Search in the document' }).fill('holiday');
    await page.keyboard.press('Enter');
    await expect(bar).toContainText('Page 2 of 2');
    await expect(bar).toContainText('1 of 1 pages');
    await axe(page);
  });

  test('E2E-R1-UI-67 a Word document shows its text in a sandboxed frame with outbound links removed', async ({ page, request }) => {
    const { list } = await docWith(page, await workspaceWithOwner(request), [['memo.docx', tinyDocx(['Quarterly memo', 'Offices close at six.'], 'Visit the site')]]);
    await list.getByRole('button', { name: 'Preview memo.docx' }).click();
    const dialog = page.getByRole('dialog', { name: 'memo.docx' });
    const frame = dialog.locator('iframe[title="Preview of memo.docx"]');
    await expect(frame).toBeVisible();
    // No scripts in the frame: allow-same-origin only lets the app write the rendered nodes.
    await expect(frame).toHaveAttribute('sandbox', 'allow-same-origin');
    const text = () => frame.evaluate((f: HTMLIFrameElement) => f.contentDocument?.body.textContent ?? '');
    await expect.poll(text).toContain('Quarterly memo');
    await expect.poll(text).toContain('Offices close at six.');
    await expect.poll(text).toContain('Visit the site');
    // The external hyperlink keeps its words but loses its target; nothing loads or runs.
    await expect.poll(() => frame.evaluate((f: HTMLIFrameElement) => [...(f.contentDocument?.querySelectorAll('[href], [src], script') ?? [])].map((e) => e.outerHTML.slice(0, 80)))).toEqual([]);
    await axe(page, [".docx-frame"]); // the frame holds the user's file; axe can't run inside the script-free sandbox
  });

  test('E2E-R1-UI-68 a CSV and a workbook show read-only tables; the workbook switches sheets', async ({ page, request }) => {
    const csv = Buffer.from('Name,Shift\nAna,Mon\n"Ben, Jr",Tue\n');
    const xlsx = tinyXlsx({ Q1: [['Item', 'Cost'], ['Desks', '1200']], Q2: [['Item', 'Cost'], ['Chairs', '800']] });
    const { list } = await docWith(page, await workspaceWithOwner(request), [['roster.csv', csv], ['budget.xlsx', xlsx]]);

    await list.getByRole('button', { name: 'Preview roster.csv' }).click();
    let dialog = page.getByRole('dialog', { name: 'roster.csv' });
    const roster = dialog.getByRole('table', { name: 'roster.csv' });
    await expect(roster.getByRole('cell', { name: 'Ben, Jr' })).toBeVisible();
    await expect(roster.getByRole('rowheader', { name: '3' })).toBeVisible();
    await expect(dialog.getByText('Read-only. 3 rows.')).toBeVisible();
    await axe(page);
    await page.keyboard.press('Escape');
    await expect(dialog).toHaveCount(0);

    await list.getByRole('button', { name: 'Preview budget.xlsx' }).click();
    dialog = page.getByRole('dialog', { name: 'budget.xlsx' });
    await expect(dialog.getByRole('table', { name: 'Q1' }).getByRole('cell', { name: 'Desks' })).toBeVisible();
    await dialog.getByRole('tablist', { name: 'Sheets' }).getByRole('tab', { name: 'Q2' }).click();
    await expect(dialog.getByRole('table', { name: 'Q2' }).getByRole('cell', { name: 'Chairs' })).toBeVisible();
    await axe(page);
  });

  test('E2E-R1-UI-69 a type without a preview, or a zip posing as a Word file, shows the download fallback', async ({ page, request }) => {
    const w = await workspaceWithOwner(request);
    const fake = zipStored([['word/readme.txt', 'not a document']]);
    const { list, uploaded } = await docWith(page, w, [['notes.txt', Buffer.from('plain words')], ['fake.docx', fake]]);
    // The server refuses to serve the look-alike: its zip has no word/document.xml.
    const link = await (await w.owner.c.get(`/api/v1/files/${uploaded[1].id}/preview-url`, { headers: w.owner.headers })).json();
    expect(link.kind).toBe('docx');
    expect((await w.owner.c.get(link.url)).status()).toBe(415);

    for (const [name, type] of [['notes.txt', 'text/plain'], ['fake.docx', 'application/vnd.openxmlformats-officedocument.wordprocessingml.document']]) {
      await list.getByRole('button', { name: `Preview ${name}` }).click();
      const dialog = page.getByRole('dialog', { name });
      await expect(dialog.getByText('No preview for this type — download it.')).toBeVisible();
      await expect(dialog.getByRole('definition').filter({ hasText: name })).toBeVisible();
      await expect(dialog.getByRole('definition').filter({ hasText: type })).toBeVisible();
      await expect(dialog.getByRole('button', { name: 'Download' })).toBeVisible();
      await axe(page);
      await page.keyboard.press('Escape');
      await expect(dialog).toHaveCount(0);
    }
  });
});
