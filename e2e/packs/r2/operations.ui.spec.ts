import { type APIRequestContext } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-14 — Operations (PR-09, docs/05 §3.18): technical incidents apart from business
// exceptions. An admin retries one failed step and dismisses another, each with a reason; the
// business exceptions link to Requests oversight. A member can't list incidents.

const key = () => `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`;
const leave = { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' };

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  return { email, session: (await enter(email, ws)) as Session };
}

test.describe('operations @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-14 an admin retries and dismisses technical incidents, apart from business exceptions', async ({ page, request }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    const r = await ann.session.c.post('/api/v1/requests', { headers: { ...ann.session.headers, 'idempotency-key': key() }, data: { type: 'hr.leave-request', input: leave } });
    expect(r.status(), await r.text()).toBe(201);
    const { id, display_id: displayId } = await r.json();
    for (let i = 0; i < 2; i++) expect((await request.post('/__test/incidents', { data: { workspace: ws.id, request: id } })).status()).toBe(201);
    expect((await ann.session.c.get('/api/v1/operations/incidents', { headers: ann.session.headers })).status()).toBe(403);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Operations');
    await expect(page.getByRole('heading', { level: 1, name: 'Operations' })).toBeVisible();
    const list = page.getByRole('list', { name: 'Open incidents' });
    await expect(list.getByRole('listitem')).toHaveCount(2);
    await expect(list.getByRole('listitem').first()).toContainText('The request couldn’t move to its next step');
    await expect(list.getByRole('listitem').first()).toContainText(displayId);
    await expect(list.getByRole('listitem').first()).toContainText('e2e: the stimulus could not be read');
    const exceptions = page.getByRole('region', { name: 'Business exceptions' });
    await expect(exceptions.getByRole('link', { name: /steps with nobody: open in Requests oversight/ })).toBeVisible();
    await axe(page);

    await list.getByRole('button', { name: `Retry: The request couldn’t move to its next step (${displayId})` }).first().click();
    let dialog = page.getByRole('dialog', { name: 'Retry incident' });
    await dialog.getByRole('button', { name: 'Retry' }).click();
    await expect(dialog.getByText('Say why, for the audit log.')).toBeVisible();
    await dialog.getByRole('textbox', { name: 'Reason' }).fill('Mail server was down; back now');
    await dialog.getByRole('button', { name: 'Retry' }).click();
    await expect(page.getByText('Retrying now. If it fails again it comes back here.')).toBeVisible();
    await expect(list.getByRole('listitem')).toHaveCount(1);

    await list.getByRole('button', { name: /^Dismiss:/ }).click();
    dialog = page.getByRole('dialog', { name: 'Dismiss incident' });
    await dialog.getByRole('textbox', { name: 'Reason' }).fill('Handled by phone');
    await dialog.getByRole('button', { name: 'Dismiss' }).click();
    await expect(page.getByText('Incident dismissed.')).toBeVisible();
    await expect(page.getByText('No incidents')).toBeVisible();

    const audit = await (await owner.c.get('/api/v1/audit/events?action=incident.dismissed', { headers: owner.headers })).json();
    expect(JSON.stringify(audit)).toContain('Handled by phone');
  });
});
