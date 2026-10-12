import { type APIRequestContext, type Page } from '@playwright/test';
import { PW, client, hasDatabase, serverNow, uniqueEmail } from '../../fixtures/accounts';
import { expect, test } from '../../fixtures/test-controls';
import { WEB, axe, nav, openSettings, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R2-UI-10..11 — workspace request settings and out of office through the web UI (BPMS-103,
// BPMS-086): an admin turns a request type off in Settings › Requests and it disappears from New
// request; a manager sets out of office with a delegate on Account › Profile, and the delegate
// approves a new request on their behalf from My work.

async function person(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase().replace(/\s+/g, '-'));
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const s = await enter(email, ws);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, recovery: s.recovery, session: s as Session, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

async function openNewRequest(page: Page) {
  await nav(page, 'Requests');
  await page.getByRole('navigation', { name: 'Request views' }).getByRole('link', { name: 'New request', exact: true }).click();
}

test.describe('workspace requests and out of office @r2 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs people in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R2-UI-10 an admin turns a request type off in Settings › Requests and it disappears from New request', async ({ page, browser, request }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const ben = await person(request, owner, ws.id, 'Ben Employee');

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await openSettings(page, 'Requests');
    const types = page.getByRole('list', { name: 'Request types' });
    await expect(types.getByRole('switch', { name: 'Offer Leave request' })).toBeChecked();
    await expect(page.getByRole('heading', { level: 2, name: 'Out of office' })).toBeVisible();
    await axe(page);
    await types.getByRole('switch', { name: 'Offer Leave request' }).click();
    await expect(types.getByRole('switch', { name: 'Offer Leave request' })).not.toBeChecked();
    await expect(types.getByRole('listitem').filter({ hasText: 'Leave request' })).toContainText('Nobody (turned off)');
    // Who can ask: the expense claim is limited to a role; the summary says so.
    await types.getByRole('button', { name: 'Who can ask for Expense claim' }).click();
    const dialog = page.getByRole('dialog', { name: 'Who can ask for expense claims' });
    await dialog.getByRole('button', { name: 'Specific people' }).click();
    await dialog.getByRole('group', { name: 'Roles' }).getByRole('button', { name: 'Member', exact: true }).click();
    await dialog.getByRole('button', { name: 'Save' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(types.getByRole('listitem').filter({ hasText: 'Expense claim' })).toContainText('Who can ask: Member');

    const emp = await (await browser.newContext()).newPage();
    await signIn(emp, ben.email, ben.recovery[0]!);
    await openNewRequest(emp);
    await expect(emp.getByRole('link', { name: 'Expense claim', exact: true })).toBeVisible();
    await expect(emp.getByRole('link', { name: 'Leave request', exact: true })).toHaveCount(0);
    // A bookmarked form for the type says it isn't available.
    await emp.goto(`${WEB}/requests/new/hr.leave-request`);
    await expect(emp.getByText('This request type isn’t available', { exact: true })).toBeVisible();
  });

  test('E2E-R2-UI-11 a manager sets out of office with a delegate; the delegate approves a new request on their behalf', async ({ page, browser, request, controls }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const ana = await person(request, owner, ws.id, 'Ana Manager');
    const ben = await person(request, owner, ws.id, 'Ben Employee');
    const dan = await person(request, owner, ws.id, 'Dan Delegate');
    expect((await owner.c.patch(`/api/v1/members/${ben.id}`, { headers: owner.headers, data: { manager_membership_id: ana.id } })).ok()).toBe(true);

    // Ana sets her out of office on Account › Profile (dates from the server's today: the shared clock may run ahead).
    const now = await serverNow(ana.session.c);
    const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);
    await signIn(page, ana.email, ana.recovery[0]!);
    await page.getByRole('button', { name: /^Account: Ana Manager/ }).click();
    await page.getByRole('menuitem', { name: 'Profile' }).click();
    const section = page.getByRole('region', { name: 'Out of office' });
    await section.getByRole('button', { name: 'Set out of office' }).click();
    const dialog = page.getByRole('dialog', { name: 'Out of office' });
    await dialog.getByLabel('First day away').fill(day(now));
    await dialog.getByLabel('Last day away').fill(day(now + 30 * 86_400_000));
    await dialog.getByRole('button', { name: /Delegate/ }).click();
    await page.getByRole('option', { name: 'Dan Delegate', exact: true }).click();
    await dialog.getByLabel(/^Note/).fill('Call Dan for anything urgent');
    await axe(page);
    await dialog.getByRole('button', { name: 'Save out of office' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Out of office saved.' })).toBeVisible();
    await expect(section).toContainText('Away');
    await expect(section).toContainText('Dan Delegate decides your requests meanwhile.');

    // Ben asks for leave; it goes straight to Dan, on Ana's behalf.
    const leave = { leave_type: 'annual', start_date: day(now + 14 * 86_400_000), end_date: day(now + 16 * 86_400_000) };
    const res = await ben.session.c.post('/api/v1/requests', { headers: { ...ben.session.headers, 'idempotency-key': `ui11-${Date.now()}` }, data: { type: 'hr.leave-request', input: leave } });
    expect(res.status(), await res.text()).toBe(201);
    const { id, display_id: ref } = await res.json();
    await controls.drain();

    const del = await (await browser.newContext()).newPage();
    await signIn(del, dan.email, dan.recovery[0]!);
    await nav(del, 'My work');
    const row = del.getByRole('navigation', { name: 'Open work' }).getByRole('link').filter({ hasText: ref });
    await expect(row).toContainText('Away');
    await expect(row).toContainText('For Ana Manager');
    await row.click();
    await expect(del.getByRole('status').filter({ hasText: 'You’re deciding this for Ana Manager.' })).toBeVisible();
    await del.getByRole('region', { name: 'Your decision' }).getByRole('button', { name: 'Approve' }).click();
    await expect(del.getByRole('status').filter({ hasText: `Approved ${ref}. It’s off your list.` })).toBeVisible();

    // The request's steps and timeline record the decision as Dan's, on Ana's behalf.
    await del.goto(`${WEB}/requests/${id}`);
    await expect(del.getByRole('list', { name: 'Approval steps' })).toContainText('Approved by Dan Delegate on behalf of Ana Manager');
    await expect(del.getByRole('region', { name: 'Timeline' })).toContainText('Dan Delegate decides on behalf of Ana Manager, who is away');
  });

  test('E2E-R2-UI-12 status from the account menu: away shows on your avatar and Home, and to others on People and Members', async ({ page, browser, request }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const ben = await person(request, owner, ws.id, 'Ben Employee');
    await person(request, owner, ws.id, 'Dan Delegate');
    const now = await serverNow(ben.session.c);
    const day = (ms: number) => new Date(ms).toISOString().slice(0, 10);

    // Ben sets out of office from the account menu, without visiting Profile.
    await signIn(page, ben.email, ben.recovery[0]!);
    await page.getByRole('button', { name: /^Account: Ben Employee$/ }).click();
    await page.getByRole('menuitem', { name: 'Out of office' }).click();
    const dialog = page.getByRole('dialog', { name: 'Out of office' });
    await dialog.getByLabel('First day away').fill(day(now));
    await dialog.getByLabel('Last day away').fill(day(now + 7 * 86_400_000));
    await dialog.getByRole('button', { name: /Delegate/ }).click();
    await page.getByRole('option', { name: 'Dan Delegate' }).click();
    await dialog.getByRole('button', { name: 'Save out of office' }).click();
    await expect(dialog).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Account: Ben Employee, out of office' })).toBeVisible();
    await nav(page, 'Home');
    const banner = page.getByRole('status').filter({ hasText: 'You’re out of office until' });
    await expect(banner).toContainText('Dan Delegate is deciding your requests');
    await axe(page);

    // Others see it: a hover card on Home › People, and a tag on Members.
    const other = await (await browser.newContext()).newPage();
    await signIn(other, ownerEmail, owner.recovery![0]!);
    const avatar = other.getByRole('group', { name: /active members/ }).locator('.person-hover').filter({ hasText: 'Ben Employee' });
    await avatar.hover();
    await expect(other.getByRole('tooltip').filter({ hasText: 'Ben Employee' })).toContainText('Dan Delegate covers');
    await other.goto(`${WEB}/admin/members`);
    await expect(other.getByRole('row').filter({ hasText: 'Ben Employee' })).toContainText('Out of office until');

    // Back early: one press on Home clears it everywhere.
    await banner.getByRole('button', { name: 'I’m back' }).click();
    await expect(banner).toHaveCount(0);
    await expect(page.getByRole('button', { name: /^Account: Ben Employee$/ })).toBeVisible();
  });
});
