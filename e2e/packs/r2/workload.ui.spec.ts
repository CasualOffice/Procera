import { type APIRequestContext } from '@playwright/test';
import { PW, client, hasDatabase, serverNow, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-13 — Team workload (FR-A03): a manager in the directory (no grant needed) sees their reporting line's open steps,
// who is away holding work, and moves two of an away person's steps to a colleague in one go.
// The workload API is scoped: a manager can't list someone outside their line; admins see all.

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

test.describe('team workload @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-13 a manager sees their team’s workload and moves an away person’s steps in bulk', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const mia = await person(request, owner, ws.id, 'Mia Manager');
    const ann = await person(request, owner, ws.id, 'Ann Lead');
    const ben = await person(request, owner, ws.id, 'Ben Peer');
    const cy = await person(request, owner, ws.id, 'Cy Junior');
    const dan = await person(request, owner, ws.id, 'Dan Elsewhere');
    const reports = async (who: string, manager: string) =>
      expect((await owner.c.patch(`/api/v1/members/${who}`, { headers: owner.headers, data: { manager_membership_id: manager } })).ok()).toBe(true);
    await reports(ann.id, mia.id); await reports(ben.id, mia.id); await reports(cy.id, ann.id);
    for (let i = 0; i < 2; i++) {
      const r = await cy.session.c.post('/api/v1/requests', { headers: { ...cy.session.headers, 'idempotency-key': key() }, data: { type: 'hr.leave-request', input: leave } });
      expect(r.status(), await r.text()).toBe(201);
    }
    // Ann goes away with Dan covering: her two steps are a coverage gap.
    const now = await serverNow(owner.c);
    const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);
    expect((await owner.c.put(`/api/v1/members/${ann.id}/out-of-office`, { headers: owner.headers, data: { starts_on: day(now), ends_on: day(now + 5 * 86_400_000), delegate: dan.id } })).status()).toBe(200);

    // Scoping: Mia can't list someone outside her line; the owner sees everyone.
    expect((await mia.session.c.get(`/api/v1/work/workload/${dan.id}`, { headers: mia.session.headers })).status()).toBe(404);
    expect((await cy.session.c.get('/api/v1/work/workload', { headers: cy.session.headers })).status()).toBe(403);
    const all = await (await owner.c.get('/api/v1/work/workload?scope=all', { headers: owner.headers })).json();
    expect(all.people.map((p: { name: string }) => p.name)).toContain('Dan Elsewhere');

    await signIn(page, mia.email, mia.recovery[0]!);
    await nav(page, 'Team');
    await expect(page.getByRole('heading', { level: 1, name: 'Team' })).toBeVisible();
    const stats = page.getByRole('definition').filter({ hasText: /^\d+$/ });
    await expect(page.getByLabel('Workload summary')).toContainText('Away with open steps1');
    await expect(stats).toHaveCount(5);
    const annRow = page.getByRole('row').filter({ hasText: 'Ann Lead' });
    await expect(annRow).toContainText('Dan Elsewhere covers · 2 still with them');
    await expect(page.getByRole('row').filter({ hasText: 'Cy Junior' })).toBeVisible(); // indirect report
    await expect(page.getByRole('link', { name: 'Dan Elsewhere', exact: true })).toHaveCount(0);
    await axe(page);

    await annRow.getByRole('link', { name: 'Ann Lead' }).click();
    const steps = page.getByRole('region', { name: 'Steps with Ann Lead' });
    await expect(steps).toContainText('is away until');
    await steps.getByRole('checkbox', { name: 'Select all rows' }).check();
    await steps.getByRole('button', { name: 'Reassign 2…' }).click();
    const dialog = page.getByRole('dialog', { name: 'Reassign 2 steps' });
    await expect(dialog.getByRole('list', { name: 'Steps to reassign' }).getByRole('listitem')).toHaveCount(2);
    // Under load the list can close as the dialog settles; reopen it until the option is picked.
    await expect(async () => {
      await dialog.getByRole('button', { name: /Person/ }).click();
      await page.getByRole('option', { name: /Ben Peer/ }).click({ timeout: 3000 });
    }).toPass({ timeout: 30_000 });
    await dialog.getByRole('textbox', { name: 'Reason' }).fill('Ann is on leave this week');
    await dialog.getByRole('button', { name: 'Reassign' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(steps).toContainText('2 steps are now with Ben Peer.');
    await expect(steps).toContainText('Ann Lead has nothing waiting');
    await expect(page.getByRole('row').filter({ hasText: 'Ben Peer' }).getByRole('cell').nth(1)).toHaveText('2');
    await expect(page.getByLabel('Workload summary')).toContainText('Away with open steps0');
    // Within the team only: Mia can't hand one of Ben's steps to someone outside her line.
    const bens = (await (await mia.session.c.get(`/api/v1/work/workload/${ben.id}`, { headers: mia.session.headers })).json()).items as { id: string }[];
    expect(bens).toHaveLength(2);
    const out = await mia.session.c.post(`/api/v1/work/${bens[0]!.id}/reassign`, { headers: mia.session.headers, data: { membership_id: dan.id, reason: 'outside' } });
    expect(out.status()).toBe(403);
  });

  test('E2E-R2-UI-16 a Team Manager is granted for a department only, and their team is that department', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const tess = await person(request, owner, ws.id, 'Tess Teamlead');
    const sam = await person(request, owner, ws.id, 'Sam Support');
    const nia = await person(request, owner, ws.id, 'Nia Night');
    await person(request, owner, ws.id, 'Oz Outsider');
    const h = owner.headers;
    // Workspace-wide is refused: a Team Manager answers for a department.
    const wide = await owner.c.post('/api/v1/role-grants', { headers: h, data: { membership_id: tess.id, role_key: 'team.manager', scope_kind: 'workspace' } });
    expect(wide.status()).toBe(422);
    expect(await wide.text()).toContain('granted for a department');
    const ops = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Operations' } })).json();
    const night = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Night shift', parent_id: ops.id } })).json();
    for (const [who, d] of [[sam.id, ops.id], [nia.id, night.id], [tess.id, ops.id]] as const) {
      expect((await owner.c.patch(`/api/v1/members/${who}`, { headers: h, data: { department_id: d } })).status()).toBe(200);
    }
    const g = await owner.c.post('/api/v1/role-grants', { headers: h, data: { membership_id: tess.id, role_key: 'team.manager', scope_kind: 'department', scope_id: ops.id } });
    expect(g.status(), await g.text()).toBe(201);
    // No workspace-wide oversight from it.
    expect((await tess.session.c.get('/api/v1/work/workload?scope=all', { headers: tess.session.headers })).status()).toBe(403);

    await signIn(page, tess.email, tess.recovery[0]!);
    await nav(page, 'Team');
    await expect(page.getByRole('link', { name: 'Sam Support', exact: true })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Nia Night', exact: true })).toBeVisible(); // sub-team
    await expect(page.getByRole('link', { name: 'Oz Outsider', exact: true })).toHaveCount(0);
    await expect(page.getByLabel('Workload summary')).toContainText('People2');
  });
});
