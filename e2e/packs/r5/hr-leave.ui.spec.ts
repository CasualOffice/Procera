import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R5-UI-02 — Admin › Time off and Time off (BPMS-223): from an empty workspace the owner starts
// with the common leave types; the policies tab shows Standard's allowances, the leave types tab
// marks sick leave private; in Balances the owner adjusts their own annual leave with a reason and
// opens their ledger, where the entry shows; on their own Time off page the balances and "how it's
// calculated" show the same; axe on each view.

test.describe('time off @r5 @ui @hr', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R5-UI-02 an owner sets up time off, adjusts a balance and sees it explained', async ({ page, request }) => {
    const { owner, ownerEmail } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Admin');
    await page.getByRole('link', { name: 'Time off' }).first().click();
    await expect(page.getByRole('heading', { level: 1, name: 'Time off' })).toBeVisible();
    await expect(page.getByText('Set up time off')).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Start with common leave types' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Leave types and a Standard policy are ready' })).toBeVisible();

    // Balances: Olivia (the owner) is listed under Standard; adjust annual leave by +1.
    const grid = page.getByRole('table', { name: 'Balances' });
    const row = grid.getByRole('row').filter({ hasText: 'Olivia' });
    await expect(row).toContainText('Standard');
    await axe(page);
    await row.getByRole('button', { name: 'Adjust Olivia' }).click();
    const dlg = page.getByRole('dialog', { name: 'Adjust Olivia’s balance' });
    await dlg.getByRole('textbox', { name: /^Reason/ }).fill('Worked on the 15 August holiday');
    await axe(page);
    await dlg.getByRole('button', { name: 'Adjust balance' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Annual leave: +1 day for Olivia.' })).toBeVisible();
    await row.getByRole('button', { name: 'Olivia', exact: true }).click();
    const drawer = page.getByRole('dialog', { name: 'Olivia' });
    const annual = drawer.getByRole('region', { name: 'Annual leave' });
    await annual.getByRole('button', { name: 'How it’s calculated' }).click();
    await expect(annual.getByRole('list', { name: 'Annual leave entries' })).toContainText('Adjusted by HR');
    await expect(annual).toContainText('“Worked on the 15 August holiday” — Olivia');
    await axe(page);
    await page.keyboard.press('Escape');

    await page.getByRole('tab', { name: 'Policies' }).click();
    const std = page.getByRole('table', { name: 'Standard allowances' });
    await expect(std.getByRole('row').filter({ hasText: 'Annual leave' })).toContainText('18 days');
    await expect(std.getByRole('row').filter({ hasText: 'Annual leave' })).toContainText('Monthly');
    await expect(std.getByRole('row').filter({ hasText: 'Unpaid leave' })).toContainText('No limit');
    await axe(page);
    await page.getByRole('tab', { name: 'Leave types' }).click();
    await expect(page.getByRole('list', { name: 'Leave types' }).getByRole('listitem').filter({ hasText: 'Sick leave' })).toContainText('Private');
    await axe(page);

    // The owner's own Time off page.
    await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Time off', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Time off' })).toBeVisible();
    await expect(page.getByText(/Standard policy · leave year Jan \d{4} – Dec \d{4}/)).toBeVisible();
    const card = page.getByRole('region', { name: 'Sick leave' });
    await expect(card.getByLabel(/days? available$/)).toBeVisible(); // pro-rated: the owner joined today
    await expect(page.getByRole('complementary', { name: 'Upcoming holidays' })).toContainText('Your location isn’t set yet');
    await axe(page);
  });
});
