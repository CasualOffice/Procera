import { type APIRequestContext } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-20 — deciding several at once in My work (BPMS-216): kind filters, a preview of every
// selected request, one decision per item with its revision, so an item decided elsewhere meanwhile
// is reported rather than overwritten; rejecting needs a reason.

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

test.describe('my work bulk decisions @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-20 a manager approves several requests at once; one decided elsewhere is reported', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    const team = [await person(request, owner, ws.id, 'Ann One'), await person(request, owner, ws.id, 'Ben Two'), await person(request, owner, ws.id, 'Cy Three')];
    const refs: string[] = [];
    for (const m of team) {
      expect((await owner.c.patch(`/api/v1/members/${m.id}`, { headers: owner.headers, data: { manager_membership_id: bo.id } })).ok()).toBe(true);
      const r = await m.session.c.post('/api/v1/requests', { headers: { ...m.session.headers, 'idempotency-key': key() }, data: { type: 'hr.leave-request', input: leave } });
      expect(r.status(), await r.text()).toBe(201);
      refs.push((await r.json()).display_id);
    }

    await signIn(page, bo.email, bo.recovery[0]!);
    await nav(page, 'My work');
    const kinds = page.getByRole('navigation', { name: 'Kinds of work' });
    await expect(kinds.getByRole('link', { name: /^All\s*3$/ })).toHaveAttribute('aria-current', 'page');
    await expect(kinds.getByRole('link', { name: /^Approvals\s*3$/ })).toBeVisible();
    await page.getByRole('button', { name: 'Decide several' }).click();
    for (const ref of refs) await page.getByRole('checkbox', { name: new RegExp(`^Select ${ref}`) }).check();
    const bar = page.getByRole('group', { name: 'Decide several' });
    await expect(bar).toContainText('3 selected');

    // Rejecting needs a reason (then cancel).
    await bar.getByRole('button', { name: 'Reject 3…' }).click();
    let dialog = page.getByRole('dialog', { name: 'Reject 3 requests' });
    await dialog.getByRole('button', { name: 'Reject 3' }).click();
    await expect(dialog.getByText('Say why, so each requester knows.')).toBeVisible();
    await dialog.getByRole('button', { name: 'Cancel' }).click();

    // Meanwhile, one of them is approved elsewhere.
    const items = (await (await bo.session.c.get('/api/v1/work', { headers: bo.session.headers })).json()).items as { id: string; revision: number; request: { display_id: string } }[];
    const first = items.find((w) => w.request.display_id === refs[0])!;
    expect((await bo.session.c.post(`/api/v1/work/${first.id}/decisions`, { headers: { ...bo.session.headers, 'if-match': `"${first.revision}"` }, data: { outcome: 'approved' } })).status()).toBe(200);

    await bar.getByRole('button', { name: 'Approve 3…' }).click();
    dialog = page.getByRole('dialog', { name: 'Approve 3 requests' });
    await expect(dialog.getByRole('list', { name: 'Requests to approve' }).getByRole('listitem')).toHaveCount(3);
    await axe(page);
    await dialog.getByRole('button', { name: 'Approve 3' }).click();
    await expect(dialog).toContainText('2 decided; 1 couldn’t be.');
    await expect(dialog.getByRole('listitem').filter({ hasText: refs[0] })).toContainText('Someone decided it already');
    await dialog.getByRole('button', { name: 'Done' }).click();
    await expect(page.getByText('Approved 2 requests.')).toBeVisible();
    await expect(page.getByText('Nothing is waiting for you')).toBeVisible();
    for (const m of team) {
      const mine = (await (await m.session.c.get('/api/v1/requests?scope=mine', { headers: m.session.headers })).json()).items as { status: string }[];
      expect(mine[0]!.status).toBe('approved');
    }
  });
});
