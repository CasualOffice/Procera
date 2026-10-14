import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase } from '../../fixtures/accounts';
import { signInForm } from '../../fixtures/journey';
import { WEB, axe, nav } from '../../fixtures/ui';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R5-UI-03 — requesting time off (BPMS-224): a member opens Time off, chooses Request time off,
// picks two days; the dialog counts them and shows the balance after; a date that clashes would be
// listed as a problem; they send it and see it waiting with the days held; after their manager
// approves it, it shows Approved and the days as taken; axe on the page and the dialog.

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

test.describe('requesting time off @r5 @ui @hr', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R5-UI-03 a member requests two days off, sees them held, then approved', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const me = (await (await owner.c.get('/api/v1/me/permissions', { headers: h })).json()).membership_id as string;
    const now = new Date((await (await request.get('/version')).json()).server_time);
    const mon = plus(now, 7 + ((8 - now.getUTCDay()) % 7));
    const { types } = await (await owner.c.post('/api/v1/leave-types/defaults', { headers: h })).json();
    const annual = types.find((t: { key: string }) => t.key === 'annual');
    const nina = await member(request, owner, ws.id, 'Nina');
    expect((await owner.c.patch(`/api/v1/members/${nina.id}`, { headers: h, data: { manager_membership_id: me } })).ok()).toBe(true);
    expect((await owner.c.post(`/api/v1/members/${nina.id}/time-off/adjustments`, { headers: h, data: { leave_type_id: annual.id, days: 10, reason: 'Opening balance' } })).status()).toBe(201);

    await page.goto(`${WEB}/`);
    await signInForm(page, nina.email);
    await nav(page, 'Time off');
    await expect(page.getByRole('heading', { level: 1, name: 'Time off' })).toBeVisible();
    const card = page.getByRole('region', { name: 'Annual leave' });
    const before = Number((await card.getByLabel(/available$/).getAttribute('aria-label'))!.split(' ')[0]);
    await page.getByRole('button', { name: 'Request time off' }).click();
    const dlg = page.getByRole('dialog', { name: 'Request time off' });
    await dlg.getByLabel('First day').fill(iso(mon));
    await dlg.getByLabel('Last day').fill(iso(plus(mon, 1)));
    await expect(dlg.getByText(`2 days · ${before} days available, ${before - 2} days after`)).toBeVisible();
    await dlg.getByRole('textbox', { name: /^Note for your manager/ }).fill('Visiting family');
    await axe(page);
    await dlg.getByRole('button', { name: 'Send request' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Requested 2 days of Annual leave' })).toBeVisible();
    const mine = page.getByRole('region', { name: 'My requests' });
    await expect(mine.getByRole('listitem').first()).toContainText('Waiting for approval');
    await expect(card.getByLabel(`${before - 2} days available`)).toBeVisible();
    await expect(card).toContainText('2 days pending');
    await axe(page);

    // A clash is shown before sending.
    await page.getByRole('button', { name: 'Request time off' }).click();
    await dlg.getByLabel('First day').fill(iso(plus(mon, 1)));
    await dlg.getByLabel('Last day').fill(iso(plus(mon, 1)));
    await expect(dlg.getByRole('list', { name: 'Problems' })).toContainText('You already have time off');
    await expect(dlg.getByRole('button', { name: 'Send request' })).toBeDisabled();
    await dlg.getByRole('button', { name: 'Cancel' }).click();

    // The manager approves it.
    const items = (await (await owner.c.get('/api/v1/work?state=open', { headers: h })).json()).items as { id: string; revision: number }[];
    expect((await owner.c.post(`/api/v1/work/${items[0]!.id}/decisions`, { headers: { ...h, 'if-match': `"${items[0]!.revision}"` }, data: { outcome: 'approved' } })).status()).toBe(200);
    await nav(page, 'Home');
    await nav(page, 'Time off');
    await expect(mine.getByRole('listitem').first()).toContainText('Approved');
    await expect(card).toContainText('2 days taken');
  });
});
