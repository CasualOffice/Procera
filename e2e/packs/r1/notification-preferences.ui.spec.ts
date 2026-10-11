import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-25 — notification preferences in the browser (BPMS-048; FR-N04).

test.describe('notification preferences @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-25 a member chooses a daily digest and quiet hours, saves, and resets to the workspace defaults', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Notifications');
    await page.getByRole('link', { name: 'Preferences' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Notification preferences' })).toBeVisible();
    await expect(page.getByRole('radio', { name: /Send right away/ })).toHaveAttribute('aria-checked', 'true');
    await axe(page);

    await page.getByRole('radio', { name: /Daily digest/ }).click();
    await expect(page.getByRole('button', { name: /Digest time/ })).toBeVisible();
    await page.getByText('Hold emails during quiet hours').click(); // React Aria hides the native input
    await page.getByRole('button', { name: 'Save preferences' }).click();
    await expect(page.getByText('Preferences saved')).toBeVisible();
    await axe(page);

    await page.reload();
    await expect(page.getByRole('radio', { name: /Daily digest/ })).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByRole('checkbox', { name: 'Hold emails during quiet hours' })).toBeChecked();
    await page.getByRole('button', { name: 'Use workspace defaults' }).click();
    await expect(page.getByRole('radio', { name: /Send right away/ })).toHaveAttribute('aria-checked', 'true');
    await expect(page.getByRole('button', { name: 'Use workspace defaults' })).toHaveCount(0);
  });
});
