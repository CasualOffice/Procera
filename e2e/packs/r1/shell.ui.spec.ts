import AxeBuilder from '@axe-core/playwright';
import { type Page, expect, test } from '@playwright/test';
import { PW, hasDatabase, seedUser } from '../../fixtures/accounts';
import { inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-01..05 — sign-in, workspace selection and the app shell in the browser (BPMS-031; docs/05 §2.1).
const WEB = process.env.WEB_BASE_URL ?? 'http://127.0.0.1:14173';
const WCAG = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'];

async function axe(page: Page) {
  const r = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  expect(r.violations, JSON.stringify(r.violations, null, 2)).toEqual([]);
}

/** Signs in through the UI with password, then a recovery code (single use, no clock dependence). */
async function signIn(page: Page, email: string, recovery: string) {
  await page.goto(`${WEB}/`);
  await expect(page).toHaveURL(/\/sign-in$/);
  await page.getByLabel('Work email').fill(email);
  await page.getByLabel('Password').fill(PW);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Two-step verification' })).toBeVisible();
  await page.getByLabel('Code').fill(recovery);
  await page.getByRole('button', { name: 'Verify' }).click();
}

test.describe('app shell @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // each scenario provisions workspaces and signs in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-01 sign in with MFA lands on Home with permission-shaped navigation; pages are accessible', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await page.goto(`${WEB}/sign-in`);
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Procera' })).toBeVisible();
    await axe(page);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    const nav = page.getByRole('navigation', { name: 'Main' });
    await expect(nav.getByRole('link', { name: 'Home' })).toHaveAttribute('aria-current', 'page');
    await expect(nav.getByRole('link', { name: 'Library' })).toBeVisible();
    await expect(nav.getByRole('link', { name: 'Members' })).toBeVisible();
    await expect(page.getByRole('button', { name: /Workspace: Acme/ })).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Set up Acme' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Summary' })).toBeVisible();
    await axe(page);
    // The session survives a reload (refresh cookie, access token only in memory).
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
  });

  test('E2E-R1-UI-02 wrong password and wrong code are explained without revealing which part was wrong', async ({ page, request }) => {
    const { ownerEmail } = await workspaceWithOwner(request);
    await page.goto(`${WEB}/sign-in`);
    await page.getByLabel('Work email').fill(ownerEmail);
    await page.getByLabel('Password').fill('not the password at all');
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('alert')).toContainText('don’t match an account');
    await page.getByLabel('Password').fill(PW);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await page.getByLabel('Code').fill('000000');
    await page.getByRole('button', { name: 'Verify' }).click();
    await expect(page.getByRole('alert')).toContainText('code didn’t work');
    await axe(page);
  });

  test('E2E-R1-UI-03 several workspaces: choose one, then switch from the header', async ({ page, request }) => {
    const a = await workspaceWithOwner(request);
    const b = await workspaceWithOwner(request);
    // Invite A's owner into B and accept with A's owner session.
    await b.owner.c.post('/api/v1/members/invitations', { headers: b.owner.headers, data: { email: a.ownerEmail, roles: ['member'] } });
    const token = await inviteToken(request, b.ws.id, a.ownerEmail);
    expect((await a.owner.c.post('/api/v1/invitations/accept', { headers: a.owner.headers, data: { token } })).status()).toBe(200);
    // Rename B so the two are distinguishable.
    await b.sys.c.patch(`/sysapi/v1/workspaces/${b.ws.id}`, { headers: b.sys.headers, data: { name: 'Beta Corp' } });

    await signIn(page, a.ownerEmail, a.owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: 'Choose a workspace' })).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Beta Corp' }).click();
    await expect(page.getByRole('button', { name: /Workspace: Beta Corp/ })).toBeVisible();
    // A plain member of Beta sees no Admin.
    await expect(page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Members' })).toHaveCount(0);
    await page.getByRole('button', { name: /Workspace: Beta Corp/ }).click();
    await page.getByRole('menuitem', { name: 'Acme' }).click();
    await expect(page.getByRole('button', { name: /Workspace: Acme/ })).toBeVisible();
    await expect(page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Members' })).toBeVisible();
  });

  test('E2E-R1-UI-04 the bell shows unread notifications; the inbox marks them read', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const email = await seedUser(request, 'joiner');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
    const token = await inviteToken(request, ws.id, email);
    const login = await (await request.post('/api/v1/auth/login', { data: { email, password: PW } })).json();
    await request.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${login.access_token}` }, data: { token } });
    await request.post('/__test/drain');

    await signIn(page, ownerEmail, owner.recovery![0]!);
    const bell = page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: /Notifications, \d+ unread/ });
    await expect(bell).toBeVisible();
    await bell.click();
    await expect(page.getByRole('heading', { level: 1, name: 'Notifications' })).toBeVisible();
    await expect(page.getByText(/joined Acme/)).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Mark all as read' }).click();
    await expect(page.getByText('All caught up')).toBeVisible();
  });

  test('E2E-R1-UI-05 sign out from the account menu ends the session; reloading stays signed out; theme choice persists', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await page.getByRole('button', { name: /Account:/ }).click();
    await page.getByRole('menuitem', { name: 'Sign out' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Sign in to Procera' })).toBeVisible();
    await page.reload();
    await expect(page).toHaveURL(/\/sign-in$/);
  });

  test('E2E-R1-UI-06 theme can be switched from the account menu and survives a reload; dark theme is accessible', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await page.getByRole('button', { name: /Account:/ }).click();
    await page.getByRole('menuitem', { name: 'Dark' }).click();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.reload();
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await expect(page.getByRole('heading', { level: 1, name: 'Home' })).toBeVisible();
    await axe(page);
  });
});
