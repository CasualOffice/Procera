import { type APIRequestContext, type Browser, type Page } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { PNG_1PX, tinyPdf } from '../../fixtures/files';
import { expect, test } from '../../fixtures/test-controls';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-20..22 — requests slice 3 through the web UI (BPMS-060/101/103): "Who approves this" on
// the expense form follows the amount across the finance threshold; a leave request carries an image
// and a PDF that scan, attach and preview for the approver, who attaches a file with the decision;
// the approver sees the linked policy version.

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = await enter(email, ws);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, recovery: s.recovery, session: s as Session, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

/** Owner, employee Ben and his manager Olivia Owner. */
async function team(request: APIRequestContext) {
  const { ws, owner } = await workspaceWithOwner(request);
  const olivia = await person(request, owner, ws.id, 'Olivia Owner');
  const ben = await person(request, owner, ws.id, 'Ben Employee');
  expect((await owner.c.patch(`/api/v1/members/${ben.id}`, { headers: owner.headers, data: { manager_membership_id: olivia.id } })).ok()).toBe(true);
  return { ws, owner, olivia, ben };
}

async function openForm(page: Page, name: string) {
  await nav(page, 'Requests');
  await page.getByRole('navigation', { name: 'Request views' }).getByRole('link', { name: 'New request', exact: true }).click();
  await page.getByRole('link', { name, exact: true }).click();
  await expect(page.getByRole('heading', { level: 1, name })).toBeVisible();
  return page.getByRole('form', { name });
}

const inTwoWeeks = () => new Date(Date.now() + 14 * 86_400_000).toISOString().slice(0, 10);

async function secondPage(browser: Browser): Promise<Page> {
  return (await browser.newContext()).newPage();
}

test.describe('requests slice 3 @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-20 the expense form says who approves it and follows the amount across 500', async ({ page, request }) => {
    const { ben } = await team(request);
    await signIn(page, ben.email, ben.recovery[0]!);
    const form = await openForm(page, 'Expense claim');
    const who = page.getByRole('region', { name: 'Who approves this' });
    await expect(who).toContainText(/Olivia Owner \(your manager\), then .+ if the amount is over 500 EUR/);
    await axe(page);
    const amount = form.getByRole('textbox', { name: /^Amount/ });
    await amount.fill('900');
    await amount.press('Tab');
    await expect(who).toContainText(/\(the amount is over 500 EUR\)/);
    await amount.fill('120');
    await amount.press('Tab');
    await expect(who.getByTestId('who-approves')).toHaveText('Olivia Owner (your manager)');
    await expect(who).toContainText('Not needed now:');
  });

  test('E2E-R2-UI-21 an image and a PDF attach to a leave request; the approver previews both and attaches a file with the decision', async ({ page, browser, request, controls }) => {
    const { ben, olivia } = await team(request);
    await signIn(page, ben.email, ben.recovery[0]!);
    const form = await openForm(page, 'Leave request');
    await form.getByRole('radiogroup', { name: /Type of leave/ }).getByText('Annual', { exact: true }).click();
    await form.getByLabel(/^First day/).fill(inTwoWeeks());
    await form.getByLabel(/^Last day/).fill(inTwoWeeks());
    await form.getByLabel('Attachments', { exact: true }).setInputFiles([
      { name: 'note.png', mimeType: 'image/png', buffer: PNG_1PX },
      { name: 'certificate.pdf', mimeType: 'application/pdf', buffer: tinyPdf('Doctor note') },
    ]);
    const picked = form.getByRole('list', { name: 'Attachments: files' });
    await expect(picked.getByRole('listitem')).toHaveCount(2);
    await expect(picked.getByText('Ready')).toHaveCount(2);
    await expect(picked.getByRole('button', { name: 'Remove note.png' })).toBeVisible();
    await axe(page);
    await form.getByRole('button', { name: 'Submit request' }).click();
    await expect(page.getByRole('status').filter({ hasText: /^Success: Submitted / })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Attachments' }).getByText('note.png')).toBeVisible();
    await controls.drain();

    const mgr = await secondPage(browser);
    await signIn(mgr, olivia.email, olivia.recovery[0]!);
    await nav(mgr, 'My work');
    await mgr.getByRole('navigation', { name: 'Open work' }).getByRole('link').first().click();
    const panel = mgr.getByRole('region', { name: 'Your decision' });
    await expect(panel).toBeVisible();
    const files = panel.getByRole('region', { name: 'Files on this request' });
    await files.getByRole('button', { name: 'Preview note.png' }).click();
    await expect(mgr.getByRole('dialog', { name: 'note.png' }).getByRole('img', { name: 'Preview of note.png' })).toBeVisible();
    await mgr.keyboard.press('Escape');
    await files.getByRole('button', { name: 'Preview certificate.pdf' }).click();
    const pdf = mgr.getByRole('dialog', { name: 'certificate.pdf' });
    await expect(pdf.getByText('Doctor note')).toBeAttached();
    await mgr.keyboard.press('Escape');

    await panel.getByLabel('Attach files to your decision', { exact: true }).setInputFiles({ name: 'signed.pdf', mimeType: 'application/pdf', buffer: tinyPdf('Signed') });
    await expect(panel.getByRole('list', { name: 'Attach files to your decision: files' }).getByText('Ready')).toBeVisible();
    await panel.getByRole('button', { name: 'Approve' }).click();
    await expect(mgr.getByRole('status').filter({ hasText: /Approved .+ off your list/ })).toBeVisible();

    await page.reload();
    const timeline = page.getByRole('region', { name: 'Timeline' });
    await expect(timeline.getByRole('button', { name: 'Preview signed.pdf' })).toBeVisible();
  });

  test('E2E-R2-UI-22 the approver sees the linked policy version', async ({ page, request }) => {
    const { owner, ben, olivia } = await team(request);
    const created = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key: `leave-${Date.now()}`, title: 'Leave and absence', content: { blocks: [{ type: 'paragraph', text: 'Ask early.' }] } } })).json();
    await owner.c.post(`/api/v1/versions/${created.version.id}/submit`, { headers: owner.headers });
    expect((await owner.c.post(`/api/v1/versions/${created.version.id}/publish`, { headers: owner.headers, data: {} })).status()).toBe(200);
    expect((await owner.c.put('/api/v1/settings/requests.types.policy', { headers: owner.headers, data: { value: { 'hr.leave-request': created.artifact.id } } })).status()).toBe(200);
    const leave = { leave_type: 'annual', start_date: inTwoWeeks(), end_date: inTwoWeeks() };
    expect((await ben.session.c.post('/api/v1/requests', { headers: { ...ben.session.headers, 'idempotency-key': `ui22-${Date.now()}` }, data: { type: 'hr.leave-request', input: leave } })).status()).toBe(201);

    await signIn(page, olivia.email, olivia.recovery[0]!);
    await nav(page, 'My work');
    await page.getByRole('navigation', { name: 'Open work' }).getByRole('link').first().click();
    const panel = page.getByRole('region', { name: 'Your decision' });
    const link = panel.getByRole('link', { name: 'Leave and absence (v1.0)' });
    await expect(link).toBeVisible();
    await expect(link).toHaveAttribute('href', `/library/${created.artifact.id}`);
    await panel.getByRole('button', { name: 'Approve' }).click();
    await expect(page.getByRole('status').filter({ hasText: /Approved .+ off your list/ })).toBeVisible();
  });
});
