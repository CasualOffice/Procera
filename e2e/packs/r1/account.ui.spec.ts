import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-46 — Account › Profile: name and time zone, used in every workspace.
// E2E-R1-UI-47 — Settings: owners rename the workspace; the switcher follows.

test.describe('account and workspace identity @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-46 a person changes their name and time zone from the account menu', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await page.getByRole('button', { name: /^Account:/ }).click();
    await page.getByRole('menuitem', { name: 'Profile' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Profile' })).toBeVisible();
    await expect(page.getByRole('textbox', { name: 'Email' })).toHaveValue(ownerEmail);
    await axe(page);

    await page.getByRole('textbox', { name: 'Your name' }).fill('Olivia Ortega');
    await page.getByRole('button', { name: /Time zone/ }).click();
    await page.getByRole('option', { name: 'Europe/Berlin' }).click();
    await page.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('Profile saved')).toBeVisible();
    await expect(page.getByRole('button', { name: 'Account: Olivia Ortega' })).toBeVisible();

    await page.reload();
    await expect(page.getByRole('textbox', { name: 'Your name' })).toHaveValue('Olivia Ortega');
    await expect(page.getByRole('button', { name: /Time zone/ })).toContainText('Europe/Berlin');
    const me = await (await owner.c.get('/api/v1/me/profile', { headers: owner.headers })).json();
    expect(me).toMatchObject({ display_name: 'Olivia Ortega', timezone: 'Europe/Berlin' });
  });

  test('E2E-R1-UI-47 an owner renames the workspace in Settings and the switcher shows the new name', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Settings');
    const field = page.getByRole('textbox', { name: 'Workspace name' });
    await expect(field).toHaveValue('Acme');
    await expect(page.getByRole('button', { name: 'Rename' })).toBeDisabled();
    await field.fill('Acme Robotics');
    await page.getByRole('button', { name: 'Rename' }).click();
    await expect(page.getByRole('button', { name: 'Saved' })).toBeVisible();
    await expect(page.getByRole('button', { name: /^Workspace: Acme Robotics\./ })).toBeVisible();
    await axe(page);

    await page.reload();
    await expect(page.getByRole('textbox', { name: 'Workspace name' })).toHaveValue('Acme Robotics');
  });
});
