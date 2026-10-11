import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-29 — Encryption in the browser (BPMS-030).

test.describe('encryption @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-29 an admin sees the workspace key and rotates it with a reason', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Encryption');
    await expect(page.getByRole('heading', { level: 1, name: 'Encryption' })).toBeVisible();
    const table = page.getByRole('table', { name: 'Key versions' });
    await expect(table.getByRole('row', { name: /Version 1.*In use/ })).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Rotate key' }).click();
    const dialog = page.getByRole('dialog', { name: 'Rotate the encryption key' });
    await dialog.getByLabel('Reason').fill('Quarterly rotation');
    await axe(page);
    await dialog.getByRole('button', { name: 'Rotate key' }).click();
    await expect(page.getByText('Key rotated. Version 2 is now in use.')).toBeVisible();
    await expect(table.getByRole('row', { name: /Version 2.*In use/ })).toBeVisible();
    await expect(table.getByRole('row', { name: /Version 1/ })).toContainText(/Retired|Destroyed/);
  });
});
