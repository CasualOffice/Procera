import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R5-UI-04 — Team calendar and Org chart (BPMS-225): Ann (who manages Cy) opens Team calendar from
// Time off; her row says "(you)", her approved sick leave links to the request and Cy's pending
// casual leave shows with its type; the Org chart shows Cy under Ann under the owner, and searching
// for Cy highlights them; axe on both.

const iso = (d: Date) => d.toISOString().slice(0, 10);
const plus = (d: Date, n: number) => new Date(d.getTime() + n * 86_400_000);

async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = `${name.toLowerCase()}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}@acme.test`;
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

test.describe('team calendar @r5 @ui @hr', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R5-UI-04 a manager sees their team’s time off on the calendar and their place in the org chart', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const me = (await (await owner.c.get('/api/v1/me/permissions', { headers: h })).json()).membership_id as string;
    const now = new Date((await (await request.get('/version')).json()).server_time);
    const mon = plus(now, 7 + ((8 - now.getUTCDay()) % 7));
    await owner.c.post('/api/v1/leave-types/defaults', { headers: h });
    const [ann, cy] = [await member(request, owner, ws.id, 'Ann'), await member(request, owner, ws.id, 'Cy')];
    for (const [who, boss] of [[ann.id, me], [cy.id, ann.id]]) {
      expect((await owner.c.patch(`/api/v1/members/${who}`, { headers: h, data: { manager_membership_id: boss } })).ok()).toBe(true);
    }
    const [a, c] = [await enter(ann.email, ws.id), await enter(cy.email, ws.id)];
    const ask = async (s: Session, input: Record<string, unknown>) => {
      const r = await s.c.post('/api/v1/requests', { headers: { ...s.headers, 'idempotency-key': `calui-${Date.now()}-${Math.random()}` }, data: { type: 'hr.leave-request', input } });
      expect(r.status(), await r.text()).toBe(201);
      return (await r.json()).id as string;
    };
    const annReq = await ask(a, { leave_type: 'sick', start_date: iso(mon), end_date: iso(mon) });
    const items = (await (await owner.c.get('/api/v1/work?state=open', { headers: h })).json()).items as { id: string; revision: number; request: { id: string } }[];
    const w = items.find((x) => x.request.id === annReq)!;
    expect((await owner.c.post(`/api/v1/work/${w.id}/decisions`, { headers: { ...h, 'if-match': `"${w.revision}"` }, data: { outcome: 'approved' } })).status()).toBe(200);
    await ask(c, { leave_type: 'casual', start_date: iso(plus(mon, 1)), end_date: iso(plus(mon, 1)) });

    await signIn(page, ann.email, a.recovery[0]!);
    await nav(page, 'Time off');
    await page.getByRole('navigation', { name: 'Time off views' }).getByRole('link', { name: 'Team calendar' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Team calendar' })).toBeVisible();
    if (mon.getUTCMonth() !== now.getUTCMonth()) await page.getByRole('button', { name: 'Next month' }).click();
    const grid = page.getByRole('table', { name: /^Team calendar/ });
    await expect(grid.getByRole('rowheader', { name: /Ann \(you\)/ })).toBeVisible();
    await expect(grid.getByRole('link', { name: `Ann, ${iso(mon)}: Sick leave` })).toBeVisible();
    await expect(grid.getByRole('link', { name: `Cy, ${iso(plus(mon, 1))}: Casual leave (pending)` })).toBeVisible();
    await axe(page);

    await page.getByRole('navigation', { name: 'Time off views' }).getByRole('link', { name: 'Org chart' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Org chart' })).toBeVisible();
    const tree = page.getByRole('tree', { name: 'Org chart' });
    const olivia = tree.getByRole('treeitem').filter({ hasText: 'Olivia' }).first();
    await expect(olivia.getByRole('treeitem').filter({ hasText: 'Ann' }).first()).toBeVisible();
    await page.getByRole('textbox', { name: /^Find someone/ }).fill('Cy');
    // Only Cy matches; the branches above them stay open.
    await expect(tree.locator('.oc-card.is-hit')).toHaveCount(1);
    await expect(tree.locator('.oc-card.is-hit')).toContainText('Cy');
    await expect(tree.getByText('Ann', { exact: true })).toBeVisible();
    await axe(page);
  });
});
