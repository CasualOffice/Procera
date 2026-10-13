import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { pdfPages, upload } from '../../fixtures/files';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-UI-03..04 — the signing ceremony in the browser (BPMS-107, FR-DS04): a member finds the
// document under My work › Signatures, agrees to sign electronically, adopts a typed signature,
// signs the signature field, fills the required text and ticks the checkbox (Finish stays off until
// then), and finishes; as the only signer the envelope completes and the signed copy is offered.
// Declining needs a reason and ends the envelope (axe on the page and the dialogs).

async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  const reg = await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } });
  expect(reg.status()).toBe(201);
  const s = (await enter(email, ws)) as Session & { recovery: string[] };
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, session: s, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

async function sendTo(owner: Session, membership: string, title: string) {
  const h = owner.headers;
  const file = await upload(owner, `${title}.pdf`, pdfPages([`${title} — page 1`]));
  const e = await (await owner.c.post('/api/v1/envelopes', { headers: h, data: { title, message: 'Please sign by Friday.', file_id: file.id } })).json();
  const r = await (await owner.c.put(`/api/v1/envelopes/${e.id}/recipients`, { headers: h, data: { recipients: [{ membership_id: membership }] } })).json();
  const rid = r.recipients[0].id;
  await owner.c.put(`/api/v1/envelopes/${e.id}/fields`, {
    headers: h,
    data: {
      fields: [
        { recipient_id: rid, kind: 'signature', page: 1, x: 0.1, y: 0.62, w: 0.35, h: 0.08 },
        { recipient_id: rid, kind: 'date_signed', page: 1, x: 0.55, y: 0.65, w: 0.3, h: 0.05 },
        { recipient_id: rid, kind: 'text', page: 1, x: 0.1, y: 0.45, w: 0.5, h: 0.06, label: 'Start date' },
        { recipient_id: rid, kind: 'checkbox', page: 1, x: 0.1, y: 0.35, w: 0.06, h: 0.05, label: 'I accept the terms' },
      ],
    },
  });
  expect((await owner.c.post(`/api/v1/envelopes/${e.id}/send`, { headers: h })).status()).toBe(200);
  return e.id as string;
}

test.describe('signing ceremony @r4 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-UI-03 a member signs from My work › Signatures with a typed signature; Finish waits for every field', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const ann = await member(request, owner, ws.id, 'Ann Asker');
    const env = await sendTo(owner, ann.id, 'Offer letter');
    await signIn(page, ann.email, ann.session.recovery[0]!);

    await nav(page, 'My work');
    await page.getByRole('link', { name: /^Signatures/ }).click();
    const waiting = page.getByRole('list', { name: 'Waiting for your signature' });
    await expect(waiting.getByText('Offer letter')).toBeVisible();
    await axe(page);
    await waiting.getByRole('link', { name: 'Review and sign' }).click();

    await expect(page.getByRole('heading', { level: 1, name: 'Offer letter' })).toBeVisible();
    await expect(page.getByText('Please sign by Friday.')).toBeVisible();
    const finish = page.getByRole('button', { name: 'Finish' });
    await expect(finish).toBeDisabled();
    await expect(page.getByText('3 fields left')).toBeVisible();
    const doc = page.getByRole('region', { name: 'Document: Offer letter' });
    await expect(doc.getByRole('button', { name: 'Sign here' })).toBeVisible();
    // Consent first; then adopt a typed signature from the signature field itself.
    await page.getByText(/I agree to sign electronically/).click();
    await doc.getByRole('button', { name: 'Sign here' }).click();
    const adopt = page.getByRole('dialog', { name: 'Adopt your signature' });
    await expect(adopt.getByRole('textbox', { name: 'Full name' })).toHaveValue('Ann Asker');
    await axe(page);
    await adopt.getByRole('button', { name: 'Adopt and sign' }).click();
    await expect(doc.getByRole('button', { name: 'Signature: signed' })).toBeVisible();
    await expect(page.getByText('2 fields left')).toBeVisible();
    // Next field takes you to what's left, top to bottom, and works from the keyboard.
    await page.getByRole('button', { name: 'Next field' }).click();
    await expect(doc.getByRole('checkbox', { name: 'I accept the terms (required)' })).toBeFocused();
    await page.keyboard.press('Space');
    await expect(doc.getByRole('checkbox', { name: 'I accept the terms (required)' })).toBeChecked();
    await page.getByRole('button', { name: 'Next field' }).click();
    await expect(doc.getByRole('textbox', { name: 'Start date (required)' })).toBeFocused();
    await page.keyboard.type('1 December 2026');
    await expect(page.getByText('All fields done')).toBeVisible();
    await expect(finish).toBeEnabled();
    await finish.click();

    await expect(page.getByRole('status').filter({ hasText: 'You signed Offer letter.' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Download the signed copy' })).toBeVisible();
    const done = await (await owner.c.get(`/api/v1/envelopes/${env}`, { headers: owner.headers })).json();
    expect(done.state).toBe('completed');
    expect(done.fields.find((f: { kind: string }) => f.kind === 'text').value).toBe('1 December 2026');
    // The list now offers the signed copy.
    await nav(page, 'My work');
    await page.getByRole('link', { name: /^Signatures/ }).click();
    await expect(page.getByRole('button', { name: 'Download the signed copy of Offer letter' })).toBeVisible();
  });

  test('E2E-R4-UI-04 declining needs a reason and ends the document', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const bo = await member(request, owner, ws.id, 'Bo Boss');
    const env = await sendTo(owner, bo.id, 'Policy update');
    await signIn(page, bo.email, bo.session.recovery[0]!);
    await nav(page, 'My work');
    await page.getByRole('link', { name: /^Signatures/ }).click();
    await page.getByRole('list', { name: 'Waiting for your signature' }).getByRole('link', { name: 'Review and sign' }).click();
    await page.getByRole('button', { name: 'Decline' }).click();
    const dialog = page.getByRole('dialog', { name: 'Decline to sign Policy update?' });
    const go = dialog.getByRole('button', { name: 'Decline' });
    await expect(go).toBeDisabled();
    await dialog.getByRole('textbox', { name: 'Reason' }).fill('The dates are wrong');
    await axe(page);
    await go.click();
    await expect(page.getByText('This document was declined, so nobody else can sign it.')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Finish' })).toHaveCount(0);
    const e = await (await owner.c.get(`/api/v1/envelopes/${env}`, { headers: owner.headers })).json();
    expect(e).toMatchObject({ state: 'declined' });
    expect(e.recipients[0].decline_reason).toBe('The dates are wrong');
  });
});
