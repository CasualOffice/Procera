import { expect, test } from '@playwright/test';
import { client, hasDatabase } from '../../fixtures/accounts';
import { axe, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-UI-01 — Account › API tokens (BPMS-144): the owner makes a read-only token from the account
// menu, copies it from the show-once dialog, a script reads with it, then the owner revokes it in the
// list and it stops working (axe on the page and the dialog).

test.describe('api tokens @r4 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-UI-01 an owner makes a read-only token, a script uses it, and revoking it stops it', async ({ page, request }) => {
    const { owner, ownerEmail } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery[0]!);
    await page.getByRole('button', { name: /^Account:/ }).click();
    await page.getByRole('menuitem', { name: 'API tokens' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'API tokens' })).toBeVisible();
    await expect(page.getByRole('menu')).toHaveCount(0);
    await expect(page.getByText('No tokens yet')).toBeVisible();
    await axe(page);

    await page.getByRole('button', { name: 'New token' }).click();
    const dialog = page.getByRole('dialog', { name: 'New API token' });
    await dialog.getByRole('textbox', { name: /^Name/ }).fill('Nightly report');
    await dialog.getByRole('button', { name: 'Read only' }).click();
    await expect(dialog.getByText(/\(\d+ chosen\)/)).not.toHaveText('(0 chosen)');
    await axe(page);
    await dialog.getByRole('button', { name: 'Make token' }).click();

    const once = page.getByRole('dialog', { name: 'Copy your token' });
    const token = (await once.getByLabel('Your new token').textContent())!.trim();
    expect(token).toMatch(/^prc_live_/);
    const script = await client();
    const auth = { authorization: `Bearer ${token}` };
    expect((await script.get('/api/v1/requests?scope=mine', { headers: auth })).status()).toBe(200);
    expect((await script.post('/api/v1/members/invitations', { headers: auth, data: { email: 'x@example.com', roles: ['member'] } })).status()).toBe(403);
    await once.getByRole('button', { name: 'I’ve copied it' }).click();

    const mine = page.getByRole('list', { name: 'Your tokens' });
    await expect(mine.getByText('Nightly report')).toBeVisible();
    await expect(mine.getByText(token.slice(-4))).toBeVisible();
    await expect(page.getByText('No other active tokens')).toBeVisible(); // owners manage tokens; their own are listed once
    await mine.getByRole('button', { name: 'Revoke Nightly report' }).click();
    await page.getByRole('alertdialog').getByRole('button', { name: 'Revoke token' }).click();
    await expect(page.getByRole('status').filter({ hasText: '“Nightly report” was revoked.' })).toBeVisible();
    await expect(mine.getByText('Revoked', { exact: true })).toBeVisible();
    expect((await script.get('/api/v1/requests?scope=mine', { headers: auth })).status()).toBe(401);
  });
});
