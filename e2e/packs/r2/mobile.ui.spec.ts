import { type APIRequestContext } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { axe, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-21 — the phone bottom bar (BPMS-207): the three everyday destinations a person has,
// My work first with its open count, then Search and More for the rest.

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

test.describe('phone bottom bar @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-21 on a phone, a manager reaches My work (with its count) and Requests from the bottom bar', async ({ browser, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const bo = await person(request, owner, ws.id, 'Bo Boss');
    for (const name of ['Ann One', 'Ben Two']) {
      const m = await person(request, owner, ws.id, name);
      expect((await owner.c.patch(`/api/v1/members/${m.id}`, { headers: owner.headers, data: { manager_membership_id: bo.id } })).ok()).toBe(true);
      const r = await m.session.c.post('/api/v1/requests', { headers: { ...m.session.headers, 'idempotency-key': key() }, data: { type: 'hr.leave-request', input: leave } });
      expect(r.status(), await r.text()).toBe(201);
    }
    const page = await (await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })).newPage();
    await signIn(page, bo.email, bo.recovery[0]!);
    const bar = page.getByRole('navigation', { name: 'Quick' });
    await expect(bar).toBeVisible();
    await expect(bar.getByRole('link')).toHaveText(['Home', /My work/, 'Requests']);
    await expect(bar.getByRole('link', { name: 'My work, 2 open' })).toBeVisible();
    await bar.getByRole('link', { name: 'My work, 2 open' }).tap();
    await expect(page.getByRole('heading', { level: 1, name: 'My work' })).toBeVisible();
    await expect(bar.getByRole('link', { name: 'My work, 2 open' })).toHaveAttribute('aria-current', 'page');
    await axe(page);
    await bar.getByRole('button', { name: 'More destinations' }).tap();
    const more = page.getByRole('menu', { name: 'More destinations' });
    await expect(more.getByRole('menuitem', { name: 'Library' })).toBeVisible();
    await expect(more.getByRole('menuitem', { name: /My work/ })).toHaveCount(0); // already on the bar
  });
});
