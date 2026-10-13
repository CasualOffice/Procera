import { type Page, expect, test } from '@playwright/test';
import { PW, hasDatabase, mfaSession, seedUser, serverNow } from '../../fixtures/accounts';
import { totp } from '../../fixtures/totp';
import { WEB, axe } from '../../fixtures/ui';
import { freshAuth, sysAdmin } from '../../fixtures/workspace';

// E2E-R1-UI-76..77 — System console › Policy floors (BPMS-065, CFG-006; docs/20 §1 layer 2): every
// platform floor grouped like the workspace Settings categories, with what it means in plain words.
// A system admin raises one with a reason and a fresh verification; an auditor can only read.

const KEY = 'auth.password.min_length';

/** Opens `path` signed out, signs in with password and a recovery code, and lands back on it. */
async function signInTo(page: Page, path: string, email: string, recovery: string) {
  await page.goto(`${WEB}${path}`);
  await expect(page).toHaveURL(/\/sign-in\?next=/);
  await page.getByLabel('Work email').fill(email);
  await page.getByLabel(/^Password/).fill(PW);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await page.getByLabel('Code').fill(recovery);
  await page.getByRole('button', { name: 'Verify' }).click();
  await expect(page).toHaveURL(new RegExp(`${path}$`));
}

test.describe('system policy floors @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-76 a system admin raises the minimum password length floor with a reason and step-up', async ({ page, request }) => {
    const sys = await sysAdmin(request);
    const usedStep = Math.floor((await serverNow(request)) / 30_000);
    const email = (await (await sys.c.get('/api/v1/me', { headers: sys.headers })).json()).user.email as string;
    const before = (await (await sys.c.get('/sysapi/v1/settings', { headers: sys.headers })).json()).items.find((f: { key: string }) => f.key === KEY);
    // Two characters more; still well below the shared test password, so parallel tests keep signing up.
    const raised = (before.value as number) + 2;
    try {
      await signInTo(page, '/system/workspaces', email, sys.recovery![0]!);
      await page.getByRole('navigation', { name: 'System' }).getByRole('link', { name: 'Policy floors' }).click();
      await expect(page.getByRole('heading', { level: 1, name: 'Policy floors' })).toBeVisible();
      const security = page.getByRole('table', { name: 'Sign-in & security floors' });
      await expect(page.getByRole('heading', { level: 2, name: 'Sign-in & security' })).toBeVisible();
      await expect(page.getByRole('table', { name: 'Data, retention & files floors' })).toBeVisible();
      const row = security.getByRole('row').filter({ hasText: 'Minimum password length' });
      await expect(row).toContainText(`Workspaces can’t set less than ${before.value}.`);
      await axe(page);

      // The sign-in is older than the system step-up window (5 minutes); reload to pick it up.
      expect((await request.post('/__test/session/age', { data: { email, seconds: 600 } })).status()).toBe(204);
      await page.reload();
      await row.getByRole('button', { name: 'Edit Minimum password length floor' }).click();
      const dialog = page.getByRole('dialog', { name: 'Minimum password length floor' });
      await dialog.getByRole('spinbutton', { name: 'Minimum password length' }).fill(String(raised));
      await expect(dialog.getByText(`Workspaces can’t set less than ${raised}.`)).toBeVisible();
      await expect(dialog.getByRole('button', { name: 'Save floor' })).toBeDisabled();
      await dialog.getByLabel('Reason for the change').fill('Longer passwords for everyone');
      await axe(page);
      await dialog.getByRole('button', { name: 'Save floor' }).click();

      const stepUp = page.getByRole('dialog', { name: 'Confirm it’s you' });
      await expect(stepUp).toBeVisible();
      // Signing in used the code for the step after `usedStep`; move the (frozen) test clock past it.
      if (Math.floor((await serverNow(request)) / 30_000) <= usedStep) {
        expect((await request.post('/__test/clock', { data: { advance: 'PT31S' } })).ok()).toBe(true);
      }
      await stepUp.getByLabel('Authentication code').fill(totp(sys.secret, await serverNow(request), 1));
      await stepUp.getByRole('button', { name: 'Verify' }).click();
      await expect(page.getByRole('status').filter({ hasText: 'Minimum password length: floor saved' })).toBeVisible();
      await expect(row).toContainText(`Workspaces can’t set less than ${raised}.`);

      const after = (await (await sys.c.get('/sysapi/v1/settings', { headers: sys.headers })).json()).items.find((f: { key: string }) => f.key === KEY);
      expect([after.value, after.revision]).toEqual([raised, before.revision + 1]);

      await page.getByRole('navigation', { name: 'System' }).getByRole('link', { name: 'System audit' }).click();
      await page.getByLabel('Action').fill('platform.floor_changed');
      await page.getByRole('button', { name: 'Apply' }).click();
      await expect(page.getByRole('table', { name: 'Audit events' }).getByText('Platform floor changed').first()).toBeVisible();
    } finally {
      const fresh = await freshAuth(sys);
      await sys.c.put(`/sysapi/v1/settings/${KEY}`, { headers: fresh, data: { value: before.value, reason: 'restore after E2E-R1-UI-76' } });
    }
  });

  test('E2E-R1-UI-77 a system auditor sees the floors but cannot edit them', async ({ page, request }) => {
    const email = await seedUser(request, 'sys-auditor');
    await request.post('/__test/system-roles', { data: { email, role: 'system.auditor' } });
    const auditor = await mfaSession(email);
    await signInTo(page, '/system/floors', email, auditor.recovery[0]!);
    await expect(page.getByRole('heading', { level: 1, name: 'Policy floors' })).toBeVisible();
    await expect(page.getByText('Your system role can see the floors but not change them.')).toBeVisible();
    await expect(page.getByRole('table', { name: 'Sign-in & security floors' }).getByRole('row').filter({ hasText: 'Minimum password length' })).toBeVisible();
    await expect(page.getByRole('button', { name: /^Edit .* floor$/ })).toHaveCount(0);
    await axe(page);
    // The API refuses the change too.
    const res = await auditor.c.put(`/sysapi/v1/settings/${KEY}`, { headers: await freshAuth(auditor), data: { value: 20, reason: 'not allowed' } });
    expect(res.status()).toBe(403);
  });
});
