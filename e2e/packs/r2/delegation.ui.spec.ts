import { type APIRequestContext } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-18 — delegating one step (BPMS-212): the approver hands the step to a colleague from
// the decision panel with a note; it leaves their list, the colleague decides it, and the timeline
// and audit record the delegation.

const key = () => `e2e-${Date.now()}-${Math.random().toString(36).slice(2)}`;
const leave = { leave_type: 'annual', start_date: '2026-12-21', end_date: '2026-12-23' };

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = await enter(email, ws);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, recovery: s.recovery, session: s as Session, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

const kinds = async (p: { session: Session }) =>
  ((await (await p.session.c.get('/api/v1/notifications', { headers: p.session.headers })).json()).items as { kind: string }[]).map((n) => n.kind);

test.describe('delegating a step @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-18 the approver delegates the step to a colleague with a note; the colleague decides it', async ({ page, browser, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const ann = await person(request, owner, ws.id, 'Ann Asker');
    const cleo = await person(request, owner, ws.id, 'Cleo Colleague');
    expect((await owner.c.patch(`/api/v1/members/${ann.id}`, { headers: owner.headers, data: { manager_membership_id: bo.id } })).ok()).toBe(true);
    const r = await ann.session.c.post('/api/v1/requests', { headers: { ...ann.session.headers, 'idempotency-key': key() }, data: { type: 'hr.leave-request', input: leave } });
    expect(r.status(), await r.text()).toBe(201);
    const { id, display_id: ref } = await r.json();

    await signIn(page, bo.email, bo.recovery[0]!);
    await nav(page, 'My work');
    await page.getByRole('navigation', { name: 'Open work' }).getByRole('link').filter({ hasText: ref }).click();
    const panel = page.getByRole('region', { name: 'Your decision' });
    await panel.getByRole('button', { name: 'Delegate…' }).click();
    const dialog = page.getByRole('dialog', { name: 'Delegate this step' });
    await dialog.getByRole('button', { name: 'Delegate', exact: true }).click();
    await expect(dialog.getByText('Choose who takes this step.')).toBeVisible();
    await dialog.getByRole('button', { name: /Delegate to/ }).click();
    await expect(page.getByRole('option', { name: /Ann Asker/ })).toHaveCount(0); // the requester is never offered
    await page.getByRole('option', { name: /Cleo Colleague/ }).click();
    await dialog.getByRole('textbox', { name: 'Note' }).fill('I am at the offsite');
    await axe(page);
    await dialog.getByRole('button', { name: 'Delegate', exact: true }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByRole('status').filter({ hasText: `Delegated ${ref}` })).toContainText('off your list');
    await expect(page.getByRole('navigation', { name: 'Open work' }).getByRole('link').filter({ hasText: ref })).toHaveCount(0);

    const other = await (await browser.newContext()).newPage();
    await signIn(other, cleo.email, cleo.recovery[0]!);
    await nav(other, 'My work');
    await other.getByRole('navigation', { name: 'Open work' }).getByRole('link').filter({ hasText: ref }).click();
    await other.getByRole('region', { name: 'Your decision' }).getByRole('button', { name: 'Approve' }).click();
    const detail = async () => (await (await ann.session.c.get(`/api/v1/requests/${id}`, { headers: ann.session.headers })).json());
    await expect.poll(async () => (await detail()).status).toBe('approved');
    expect(JSON.stringify((await detail()).timeline)).toContain('delegated by Bo Boss: I am at the offsite');
  });
});
