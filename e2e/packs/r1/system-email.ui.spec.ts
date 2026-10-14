import { type Page, expect, test } from '@playwright/test';
import { PW, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { WEB, axe } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-64..65 — email delivery settings and first-time setup in the system console (BPMS-065,
// CFG-006; docs/20 §3). E2E builds deliver only through PROCERA_SMTP_URL, so saving settings here never
// sends the shared queue; the relay points at a closed local port and the test send reports that.

/** Opens `path` signed out, signs in with password and a recovery code, and lands back on it. */
async function signInTo(page: Page, path: string, email: string, recovery: string) {
  await page.goto(`${WEB}${path}`);
  await expect(page).toHaveURL(/\/sign-in\?next=/);
  await page.getByLabel('Work email').fill(email);
  await page.getByLabel(/^Password/).fill(PW);
  await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  await page.getByLabel('Code').fill(recovery);
  await page.getByRole('button', { name: 'Verify' }).click();
  await expect(page).toHaveURL(new RegExp(`${path}$`));
}

test.describe('system email @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-64 a system admin saves the email settings, the password is never shown, and a failing test send explains why', async ({ page, request }) => {
    const { sys } = await workspaceWithOwner(request);
    const email = (await (await sys.c.get('/api/v1/me', { headers: sys.headers })).json()).user.email as string;
    await signInTo(page, '/system/workspaces', email, sys.recovery![0]!);
    await page.getByRole('navigation', { name: 'System' }).getByRole('link', { name: 'Email & platform' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Email & platform' })).toBeVisible();
    await expect(page.getByRole('heading', { level: 2, name: 'Platform basics' })).toBeVisible();
    await expect(page.getByText('Public address')).toBeVisible();
    await axe(page);

    await page.getByLabel('SMTP host').fill('localhost');
    await page.getByRole('button', { name: /Encryption/ }).click();
    await page.getByRole('option', { name: 'None (only for a relay on this server)' }).click();
    await page.getByLabel('Port').fill('1');
    await page.getByLabel('Username').fill('mailer');
    await page.getByLabel(/^Password/).fill('never-shown-secret');
    await page.getByLabel('From address').fill('no-reply@procera.test');
    await page.getByLabel('Sender name').fill('Procera Test');
    await page.getByLabel('Reason for the change').fill('Set up delivery');
    await page.getByRole('button', { name: 'Save email settings' }).click();
    await expect(page.getByText('Email settings saved')).toBeVisible();
    await expect(page.getByText('Configured', { exact: true })).toBeVisible();
    await expect(page.getByText('Leave blank to keep the current password.')).toBeVisible();
    await expect(page.getByLabel(/^Password/)).toHaveValue('');
    const api = await (await sys.c.get('/sysapi/v1/email', { headers: sys.headers })).text();
    expect(api).toContain('"password_set":true');
    expect(api).not.toContain('never-shown-secret');

    await page.getByRole('button', { name: 'Send test email' }).click();
    const dialog = page.getByRole('dialog', { name: 'Send a test email' });
    await dialog.getByLabel('Send to').fill(uniqueEmail('probe'));
    await dialog.getByRole('button', { name: 'Send test email' }).click();
    await expect(dialog.getByText('The test email wasn’t sent')).toBeVisible();
    await expect(dialog.getByText(/refused the connection/)).toBeVisible();
    await axe(page);
    await dialog.getByRole('button', { name: 'Close', exact: true }).first().click();

    await page.getByRole('navigation', { name: 'System' }).getByRole('link', { name: 'System audit' }).click();
    await page.getByLabel('Action').fill('platform.email_');
    await page.getByRole('button', { name: 'Apply' }).click();
    const events = page.getByRole('table', { name: 'Audit events' });
    await expect(events.getByText('Platform email changed').first()).toBeVisible();
    await expect(events.getByText('Platform email tested').first()).toBeVisible();
  });

  test('E2E-R1-UI-65 first-time setup continues to an optional email step and the first workspace', async ({ page, request }) => {
    const { sys } = await workspaceWithOwner(request);
    const email = (await (await sys.c.get('/api/v1/me', { headers: sys.headers })).json()).user.email as string;
    await signInTo(page, '/system/setup', email, sys.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: 'Finish setting up' })).toBeVisible();
    await expect(page.getByRole('heading', { level: 2, name: 'Set up email' })).toBeVisible();
    await expect(page.getByLabel('SMTP host')).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Skip for now' }).click();

    await expect(page.getByRole('heading', { level: 2, name: 'Create your first workspace' })).toBeVisible();
    await page.getByRole('button', { name: 'Create workspace' }).click();
    const dialog = page.getByRole('dialog', { name: 'New workspace' });
    const name = `Setup Co ${Date.now().toString(36)}`;
    const owner = uniqueEmail('setup-owner');
    await dialog.getByLabel('Workspace name').fill(name);
    await dialog.getByLabel('Owner email').fill(owner);
    await dialog.getByRole('button', { name: 'Create workspace' }).click();
    await expect(page.getByRole('heading', { level: 2, name: `${name} is ready` })).toBeVisible();
    await expect(page.getByText(`We invited ${owner} to own ${name}`)).toBeVisible();
    await axe(page);
    await page.getByRole('link', { name: 'Go to the system console' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Workspaces' })).toBeVisible();
  });
});
