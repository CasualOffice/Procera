import { type Page, expect, test } from '@playwright/test';
import { PW, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { WEB, axe } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-40..41 — the system console in the browser (BPMS-065; docs/05 §2.3, §3.20).

/** Opens /system signed out, signs in with password and a recovery code, and lands back on the console. */
async function signInToSystem(page: Page, email: string, recovery: string) {
  await page.goto(`${WEB}/system`);
  await expect(page).toHaveURL(/\/sign-in\?next=%2Fsystem/);
  await page.getByLabel('Work email').fill(email);
  await page.getByLabel('Password').fill(PW);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await page.getByLabel('Code').fill(recovery);
  await page.getByRole('button', { name: 'Verify' }).click();
  await expect(page).toHaveURL(/\/system\/workspaces$/);
}

const sysNav = (page: Page, label: string) => page.getByRole('navigation', { name: 'System' }).getByRole('link', { name: label }).click();

test.describe('system console @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-40 a system admin finds a workspace, makes it read-only with a reason, and reactivates it', async ({ page, request }) => {
    const { sys, ws } = await workspaceWithOwner(request);
    const me = await sys.c.get('/api/v1/me', { headers: sys.headers });
    const email = (await me.json()).user.email as string;

    await signInToSystem(page, email, sys.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: 'Workspaces' })).toBeVisible();
    await expect(page.getByText('System', { exact: true }).first()).toBeVisible();
    await page.getByRole('searchbox', { name: 'Search workspaces' }).fill(ws.slug);
    const table = page.getByRole('table', { name: 'Workspaces' });
    await expect(table.getByRole('row')).toHaveCount(2);
    await expect(table.getByText('1 of 2,000')).toBeVisible();
    await axe(page);

    await page.getByRole('button', { name: /^Read-only/ }).click();
    await expect(page.getByText('No workspaces match')).toBeVisible();
    await page.getByRole('button', { name: /^All/ }).click();

    await table.getByRole('button', { name: `${ws.name}, open details` }).click();
    const drawer = page.getByRole('dialog', { name: ws.name });
    await expect(drawer.getByText('Workspace created')).toBeVisible();
    await drawer.getByRole('button', { name: 'Make read-only' }).click();
    const dialog = page.getByRole('dialog', { name: 'Make this workspace read-only' });
    await dialog.getByLabel('Reason').fill('Billing review');
    await axe(page);
    await dialog.getByRole('button', { name: 'Make read-only' }).click();
    await expect(drawer.getByText(`${ws.name} is read-only`)).toBeVisible();
    await expect(drawer.getByText('Read-only: Billing review')).toBeVisible();
    await expect(drawer.getByRole('list', { name: 'Lifecycle history' }).getByText('Billing review')).toBeVisible();

    await drawer.getByRole('button', { name: 'Reactivate' }).click();
    await page.getByRole('dialog', { name: 'Reactivate this workspace' }).getByRole('button', { name: 'Reactivate' }).click();
    await expect(drawer.getByText(`${ws.name} is active again`)).toBeVisible();
    await expect(drawer.getByRole('list', { name: 'Lifecycle history' }).getByText('Workspace restored')).toBeVisible();
    const after = await (await sys.c.get(`/sysapi/v1/workspaces/${ws.id}`, { headers: sys.headers })).json();
    expect(after.state).toBe('active');
  });

  test('E2E-R1-UI-41 quotas, platform health and the system audit; non-system users never see the console', async ({ page, request }) => {
    const { sys, ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const email = (await (await sys.c.get('/api/v1/me', { headers: sys.headers })).json()).user.email as string;
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: uniqueEmail('seat'), roles: ['member'] } });

    await signInToSystem(page, email, sys.recovery![0]!);
    await page.getByRole('searchbox', { name: 'Search workspaces' }).fill(ws.slug);
    await page.getByRole('button', { name: `${ws.name}, open details` }).click();
    const drawer = page.getByRole('dialog', { name: ws.name });
    await drawer.getByRole('button', { name: 'Edit quotas' }).click();
    const dialog = page.getByRole('dialog', { name: 'Edit quotas' });
    await expect(dialog.getByText('Default for Standard: 2,000.')).toBeVisible();
    await dialog.getByLabel('Member limit').fill('2');
    await dialog.getByRole('button', { name: 'Save quotas' }).click();
    await expect(drawer.getByText('Quotas saved')).toBeVisible();
    await drawer.getByRole('button', { name: 'Close' }).click();
    // The limit holds at invite time.
    const over = await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: uniqueEmail('over'), roles: ['member'] } });
    expect(over.status()).toBe(429);

    await sysNav(page, 'Quotas & usage');
    await expect(page.getByRole('heading', { level: 1, name: 'Quotas & usage' })).toBeVisible();
    const row = page.getByRole('table', { name: 'Usage by workspace' }).getByRole('row').filter({ hasText: ws.slug });
    await expect(row.getByText('At limit')).toBeVisible();
    await expect(page.getByRole('table', { name: 'Quota defaults' }).getByRole('row')).toHaveCount(5);
    await axe(page);

    await sysNav(page, 'Platform health');
    await expect(page.getByRole('heading', { level: 1, name: 'Platform health' })).toBeVisible();
    const checks = page.getByRole('definition').first();
    await expect(checks).toBeVisible();
    await expect(page.getByText(/migrations applied/)).toBeVisible();
    await expect(page.getByText(/responded in \d+ ms/)).toBeVisible();
    await axe(page);

    await sysNav(page, 'System audit');
    await expect(page.getByRole('heading', { level: 1, name: 'System audit' })).toBeVisible();
    await expect(page.getByText('Chain verified')).toBeVisible();
    await page.getByLabel('Action').fill('workspace.quotas_changed');
    await page.getByRole('button', { name: 'Apply' }).click();
    const events = page.getByRole('table', { name: 'Audit events' });
    await expect(events.getByText('Workspace quotas changed').first()).toBeVisible();
    await axe(page);

    // A workspace owner (no system role) is told the console doesn't exist.
    await page.context().clearCookies();
    await signInToSystemAs(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: 'Page not found' })).toBeVisible();
    await axe(page);
  });
});

/** Like signInToSystem, for someone the console will turn away. */
async function signInToSystemAs(page: Page, email: string, recovery: string) {
  await page.goto(`${WEB}/system`);
  await expect(page).toHaveURL(/\/sign-in\?next=%2Fsystem/);
  await page.getByLabel('Work email').fill(email);
  await page.getByLabel('Password').fill(PW);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await page.getByLabel('Code').fill(recovery);
  await page.getByRole('button', { name: 'Verify' }).click();
}
