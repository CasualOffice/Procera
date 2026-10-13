import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { emailLink } from '../../fixtures/journey';
import { axe } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-UI-07 — deciding from the email (BPMS-145): the manager opens the Reject link from their
// approval email in a browser that isn't signed in; the page shows the request, its answers and who
// is deciding, keeps Reject off until a reason is given, and confirms; the request is rejected with
// that comment; opening the same link again says it was used. axe on the page.

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = (await enter(email, ws)) as Session;
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, session: s, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

test.describe('email approvals @r4 @ui @email', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-UI-07 a manager rejects from the email link without signing in; the link then reads as used', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    await owner.c.patch(`/api/v1/members/${ann.id}`, { headers: owner.headers, data: { manager_membership_id: bo.id } });
    const r = await ann.session.c.post('/api/v1/requests', {
      headers: { ...ann.session.headers, 'idempotency-key': `ui-${Date.now()}` },
      data: { type: 'hr.leave-request', input: { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' } },
    });
    const { id, display_id } = await r.json();
    const approve = await emailLink(request, ws.id, bo.email, 'work_assigned', /\/approve\/[A-Za-z0-9_-]+\?outcome=approved/);
    await page.goto(approve.replace('outcome=approved', 'outcome=rejected'));

    await expect(page.getByRole('heading', { level: 1, name: new RegExp(display_id) })).toBeVisible();
    await expect(page.getByText(/Ann Asker asked for/)).toBeVisible();
    await expect(page.getByText('You’re deciding as')).toContainText('Bo Boss');
    await expect(page.getByRole('radio', { name: 'Reject' })).toBeChecked();
    const go = page.getByRole('button', { name: `Reject ${display_id}` });
    await expect(go).toBeDisabled();
    await page.getByRole('textbox', { name: /Why are you rejecting it/ }).fill('The team is short that week');
    await axe(page);
    await go.click();
    await expect(page.getByRole('heading', { level: 1, name: `Rejected ${display_id}` })).toBeVisible();

    const detail = await (await ann.session.c.get(`/api/v1/requests/${id}`, { headers: ann.session.headers })).json();
    expect(detail.status).toBe('rejected');
    expect(detail.steps.find((s: { outcome: string | null }) => s.outcome === 'rejected').comment).toBe('The team is short that week');
    await page.goto(approve);
    await expect(page.getByText('This link has already been used.')).toBeVisible();
  });
});
