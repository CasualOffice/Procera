import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { pdfPages } from '../../fixtures/files';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-UI-02 — Documents › Send for signature (BPMS-107, docs/05 §3.22): the owner uploads a PDF in
// the browser, adds two signers in order, places a signature field for each by clicking the page
// (and nudges one with the keyboard), sends it, and the envelope page shows who signs next; the
// server agrees (fields saved where they were placed, the first signer's turn). axe on the list,
// the draft editor and the envelope page.

async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
}

test.describe('e-signing, sender @r4 @ui @signing', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-UI-02 an owner sends a PDF for signature: uploads it, adds two signers in order, places fields by clicking, and sends; the envelope page shows who signs next', async ({ page, request }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    await member(request, owner, ws.id, 'Ann Asker');
    await member(request, owner, ws.id, 'Bo Boss');
    await signIn(page, ownerEmail, owner.recovery![0]!);

    // Documents: empty, with the way in.
    await nav(page, 'Documents');
    await expect(page.getByRole('heading', { level: 1, name: 'Documents' })).toBeVisible();
    await expect(page.getByText('You haven’t sent anything for signature')).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Send for signature' }).first().click();

    // 1 Document: only a PDF is taken; the title comes from the file name.
    await expect(page.getByRole('heading', { level: 1, name: 'Send for signature' })).toBeVisible();
    const pdfInput = page.getByLabel('PDF document');
    await pdfInput.setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('not a pdf') });
    await expect(page.getByRole('alert').filter({ hasText: 'isn’t a PDF' })).toBeVisible();
    await pdfInput.setInputFiles({ name: 'offer-letter.pdf', mimeType: 'application/pdf', buffer: pdfPages(['Offer letter', 'Terms']) });
    await expect(page.getByText('Uploaded and scanned')).toBeVisible();
    const title = page.getByRole('textbox', { name: /^Title/ });
    await expect(title).toHaveValue('offer-letter');
    await title.fill('Offer letter');
    await page.getByRole('textbox', { name: /^Message/ }).fill('Welcome aboard!');
    await expect(page.getByRole('radio', { name: /One after another/ })).toBeChecked();
    await page.getByRole('button', { name: 'Continue to recipients' }).click();

    // The draft editor (a saved draft).
    await expect(page).toHaveURL(/\/documents\/[0-9a-f-]+\/edit$/);
    const envelopeId = page.url().match(/\/documents\/([0-9a-f-]+)\/edit$/)![1]!;
    await expect(page.getByRole('heading', { level: 1, name: 'Offer letter' })).toBeVisible();
    for (const name of ['Ann Asker', 'Bo Boss']) {
      await page.getByRole('button', { name: /Add a person/ }).click();
      await page.getByRole('option', { name: new RegExp(`^${name}`) }).click();
      await expect(page.getByRole('list', { name: 'Recipients' }).getByText(name, { exact: true })).toBeVisible();
    }
    const rows = page.getByRole('list', { name: 'Recipients' }).getByRole('listitem');
    await expect(rows).toHaveCount(2);
    await expect(rows.nth(0)).toContainText('Ann Asker');
    await expect(rows.nth(1)).toContainText('Bo Boss');
    await expect(page.getByRole('status').filter({ hasText: /^Saved$/ })).toBeVisible();

    // 2 Fields: the first signer is chosen; click the page to place a signature, nudge it with the keyboard.
    await expect(page.getByRole('radio', { name: /Ann Asker/ })).toBeChecked();
    await expect(page.getByRole('radio', { name: 'Signature' })).toBeChecked();
    // Nothing is lost: leave for Documents and come back to the draft through its link (no reload).
    await nav(page, 'Documents');
    await page.getByRole('table', { name: 'Documents sent for signature' }).getByRole('link', { name: 'Offer letter' }).click();
    await expect(page).toHaveURL(new RegExp(`/documents/${envelopeId}/edit$`));
    await expect(page.getByRole('list', { name: 'Recipients' }).getByRole('listitem')).toHaveCount(2);
    const page1 = page.locator('[data-place-page="1"]');
    await expect(page1).toBeVisible();
    const box1 = await page1.boundingBox();
    await page1.click({ position: { x: box1!.width * 0.3, y: box1!.height * 0.6 } });
    const annSig = page.getByRole('button', { name: 'Signature for Ann Asker, page 1' });
    await expect(annSig).toBeFocused();
    await annSig.press('ArrowRight');
    await expect(page.getByRole('heading', { name: 'Selected field' })).toBeVisible();

    await page.getByRole('radio', { name: /Bo Boss/ }).click();
    const page2 = page.locator('[data-place-page="2"]');
    await page2.scrollIntoViewIfNeeded();
    const box2 = await page2.boundingBox();
    await page2.click({ position: { x: box2!.width * 0.3, y: box2!.height * 0.6 } });
    await expect(page.getByRole('button', { name: 'Signature for Bo Boss, page 2' })).toBeVisible();
    // A date for Bo from the keyboard path ("Add to page"), then removed from the side list.
    await page.getByRole('radio', { name: 'Date signed' }).click();
    const pageNo = page.getByLabel('Page', { exact: true });
    await pageNo.fill('2');
    await pageNo.press('Enter');
    await page.getByRole('button', { name: 'Add to page' }).click();
    await expect(page.getByRole('button', { name: 'Date signed for Bo Boss, page 2' })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Fields (3)' })).toBeVisible();
    await page.getByRole('button', { name: 'Remove Date signed for Bo Boss on page 2' }).click();
    await expect(page.getByRole('heading', { name: 'Fields (2)' })).toBeVisible();
    await expect(page.getByRole('status').filter({ hasText: /^Saved$/ })).toBeVisible();
    await axe(page);

    // The server has the fields where they were placed.
    const draft = await (await owner.c.get(`/api/v1/envelopes/${envelopeId}`, { headers: owner.headers })).json();
    expect(draft.state).toBe('draft');
    expect(draft.recipients.map((r: { name: string }) => r.name)).toEqual(['Ann Asker', 'Bo Boss']);
    const fields = draft.fields as { kind: string; page: number; x: number; y: number; w: number; h: number; recipient_id: string }[];
    expect(fields.map((f) => [f.kind, f.page]).sort()).toEqual([['signature', 1], ['signature', 2]]);
    const f1 = fields.find((f) => f.page === 1)!;
    expect(f1.recipient_id).toBe(draft.recipients[0].id);
    expect(f1.x + f1.w / 2).toBeCloseTo(0.305, 1); // centred on the click, nudged right 0.5%
    expect(f1.y + f1.h / 2).toBeCloseTo(0.6, 1);

    // 3 Review and send.
    await expect(page.getByRole('list', { name: 'Who it goes to' }).getByRole('listitem')).toHaveCount(2);
    await page.getByRole('button', { name: 'Send for signature' }).click();

    // The envelope page: Ann signs next, Bo waits.
    await expect(page).toHaveURL(new RegExp(`/documents/${envelopeId}$`));
    await expect(page.getByRole('status').filter({ hasText: 'Sent. Each signer is told when it’s their turn.' })).toBeVisible();
    await expect(page.getByRole('heading', { level: 1, name: 'Offer letter' })).toBeVisible();
    await expect(page.getByText('Waiting for signatures')).toBeVisible();
    const timeline = page.getByRole('list', { name: 'Signers' }).getByRole('listitem');
    await expect(timeline.nth(0)).toContainText('1. Ann Asker');
    await expect(timeline.nth(0)).toContainText('Signs next');
    await expect(timeline.nth(1)).toContainText('2. Bo Boss');
    await expect(timeline.nth(1)).toContainText('Waiting');
    await expect(page.getByText('0 of 2 signed')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Download original' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Cancel…' })).toBeVisible();
    await axe(page);
    const sent = await (await owner.c.get(`/api/v1/envelopes/${envelopeId}`, { headers: owner.headers })).json();
    expect(sent).toMatchObject({ state: 'sent', title: 'Offer letter', message: 'Welcome aboard!', order_mode: 'sequential' });

    // Back on Documents it is listed with its progress.
    await nav(page, 'Documents');
    const row = page.getByRole('table', { name: 'Documents sent for signature' }).getByRole('row').filter({ hasText: 'Offer letter' });
    await expect(row).toContainText('Waiting for signatures');
    await expect(row).toContainText('0 of 2 signed');
  });
});
