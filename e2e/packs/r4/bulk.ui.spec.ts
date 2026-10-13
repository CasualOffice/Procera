import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { pdfPages } from '../../fixtures/files';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-UI-05 — a group send in the browser (BPMS-107): the owner chooses "Everyone in a group",
// picks a group, places one signature field for "Each person" and sends; the group-send page shows
// 0 of 2 signed with each person waiting; after one signs, the Signing list and the page show
// 1 of 2; "Remind everyone waiting" answers; axe on the editor and the group-send page.

async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = (await enter(email, ws)) as Session;
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { session: s, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

test.describe('e-signing, group send @r4 @ui @signing', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-UI-05 an owner sends a policy to a group: each person gets their own copy and the page tracks who signed', async ({ page, request }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const ann = await member(request, owner, ws.id, 'Ann Asker');
    const bo = await member(request, owner, ws.id, 'Bo Boss');
    const grp = await (await owner.c.post('/api/v1/groups', { headers: owner.headers, data: { name: 'Warehouse team' } })).json();
    for (const m of [ann, bo]) await owner.c.post(`/api/v1/groups/${grp.id}/members`, { headers: owner.headers, data: { membership_id: m.id } });
    await signIn(page, ownerEmail, owner.recovery![0]!);

    await nav(page, 'Signing');
    await page.getByRole('button', { name: 'Send for signature' }).first().click();
    await page.getByLabel('PDF document').setInputFiles({ name: 'safety-policy.pdf', mimeType: 'application/pdf', buffer: pdfPages(['Safety policy']) });
    await expect(page.getByText('Uploaded and scanned')).toBeVisible();
    await page.getByRole('textbox', { name: /^Title/ }).fill('Safety policy');
    await page.getByRole('radiogroup', { name: 'Who signs' }).getByText('Everyone in a group').click();
    await expect(page.getByRole('radiogroup', { name: 'Signing order' })).toHaveCount(0);
    await page.getByRole('button', { name: 'Continue to recipients' }).click();

    await expect(page.getByRole('heading', { level: 1, name: 'Safety policy' })).toBeVisible();
    const send = page.getByRole('button', { name: 'Send to everyone chosen' });
    await expect(send).toBeDisabled();
    await page.getByRole('group', { name: 'Groups' }).getByRole('button', { name: /^Warehouse team/ }).click();
    await expect(page.getByText(/each person signs their own copy\./)).toBeVisible();
    await expect(page.getByRole('radio', { name: /Each person/ })).toBeChecked();
    const page1 = page.locator('[data-place-page="1"]');
    const box = await page1.boundingBox();
    await page1.click({ position: { x: box!.width * 0.3, y: box!.height * 0.6 } });
    await expect(page.getByRole('button', { name: 'Signature for Each person, page 1' })).toBeVisible();
    await expect(page.getByRole('status').filter({ hasText: /^Saved$/ })).toBeVisible();
    await axe(page);
    await send.click();

    await expect(page).toHaveURL(/\/documents\/batches\/[0-9a-f-]+/);
    await expect(page.getByRole('status').filter({ hasText: 'Sent. Each person is asked to sign their own copy.' })).toBeVisible();
    await expect(page.getByText('0 of 2 signed')).toBeVisible();
    const people = page.getByRole('table', { name: 'People in this send' });
    await expect(people.getByRole('row').filter({ hasText: 'Ann Asker' })).toContainText('Waiting');
    await expect(people.getByRole('row').filter({ hasText: 'Bo Boss' })).toContainText('Waiting');
    await axe(page);
    await page.getByRole('button', { name: 'Remind everyone waiting' }).click();
    await expect(page.getByRole('status').filter({ hasText: /reminded recently|Reminded/ })).toBeVisible();

    // Ann signs her own copy (API); the list and the page show it.
    const box2 = (await (await ann.session.c.get('/api/v1/me/signatures', { headers: ann.session.headers })).json()).items[0];
    const cer = await (await ann.session.c.get(`/api/v1/signing/${box2.recipient_id}`, { headers: ann.session.headers })).json();
    expect((await ann.session.c.post(`/api/v1/signing/${box2.recipient_id}/sign`, { headers: ann.session.headers, data: { document_hash: cer.envelope.document_hash, consent: true, adopted: { kind: 'typed', text: 'Ann Asker' } } })).status()).toBe(200);
    await nav(page, 'Signing');
    const list = page.getByRole('table', { name: 'Documents sent for signature' });
    await expect(list.getByRole('row').filter({ hasText: 'Safety policy' })).toContainText('1 of 2 signed');
    await list.getByRole('link', { name: 'Safety policy' }).click();
    await expect(page.getByText('1 of 2 signed')).toBeVisible();
    await expect(people.getByRole('row').filter({ hasText: 'Ann Asker' })).toContainText('Signed');
    await expect(page.getByRole('button', { name: 'Download Ann Asker’s signed copy' })).toBeVisible();
  });
});
