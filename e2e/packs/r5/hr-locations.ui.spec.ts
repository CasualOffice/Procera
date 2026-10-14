import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R5-UI-01 — Admin › Holidays and locations (BPMS-222): from an empty workspace the owner creates
// a holiday calendar, adds a holiday, pastes two more (one optional) and sees them grouped by month;
// then adds a location that works Monday to Saturday on that calendar and sees its work week and
// calendar in the list; axe on both tabs and the dialogs.

test.describe('holidays and locations @r5 @ui @hr', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R5-UI-01 an owner sets up a holiday calendar and a location', async ({ page, request }) => {
    const { owner, ownerEmail } = await workspaceWithOwner(request);
    const year = new Date().getFullYear();
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Admin');
    await page.getByRole('link', { name: 'Holidays and locations' }).first().click();
    await expect(page.getByRole('heading', { level: 1, name: 'Holidays and locations' })).toBeVisible();
    await expect(page.getByText('No holiday calendars yet')).toBeVisible();
    await axe(page);

    await page.getByRole('button', { name: 'New calendar' }).click();
    const nc = page.getByRole('dialog', { name: 'New holiday calendar' });
    await nc.getByRole('textbox', { name: /^Name/ }).fill('India');
    await nc.getByRole('button', { name: 'Create calendar' }).click();
    await expect(page.getByText(`No holidays in ${year}`)).toBeVisible();

    await page.getByRole('button', { name: 'Add holiday' }).click();
    const add = page.getByRole('dialog', { name: 'Add a holiday' });
    await add.getByLabel(/^Date/).fill(`${year}-10-02`);
    await add.getByRole('textbox', { name: /^Name/ }).fill('Gandhi Jayanti');
    await axe(page);
    await add.getByRole('button', { name: 'Add holiday' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Gandhi Jayanti added.' })).toBeVisible();

    await page.getByRole('button', { name: 'Import' }).first().click();
    const imp = page.getByRole('dialog', { name: 'Import holidays' });
    await imp.getByRole('textbox', { name: 'Holidays' }).fill(`${year}-01-26,Republic Day\n${year}-11-09,Diwali (second day),optional\nnot a date,Oops`);
    await imp.getByRole('button', { name: 'Import' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Imported: 2 added, 1 skipped.' })).toBeVisible();
    await expect(page.getByText(/Line 3: not a date,Oops/)).toBeVisible();

    const jan = page.getByRole('list', { name: /^Holidays in January/ });
    await expect(jan.getByText('Republic Day')).toBeVisible();
    const nov = page.getByRole('list', { name: /^Holidays in November/ });
    await expect(nov.getByText('Diwali (second day)')).toBeVisible();
    await expect(nov.getByText('Optional')).toBeVisible();
    await expect(page.getByText(`3 holidays in ${year} · 1 optional · Not used by any location yet`)).toBeVisible();
    await axe(page);

    await page.getByRole('tab', { name: 'Locations' }).click();
    await expect(page.getByText('No locations yet')).toBeVisible();
    await page.getByRole('button', { name: 'Add location' }).first().click();
    const loc = page.getByRole('dialog', { name: 'Add a location' });
    await loc.getByRole('textbox', { name: /^Name/ }).fill('Bangalore office');
    await loc.getByRole('button', { name: 'Saturday' }).click();
    await expect(loc.getByRole('button', { name: 'Saturday' })).toHaveAttribute('aria-pressed', 'true');
    await axe(page);
    await loc.getByRole('button', { name: 'Add location' }).click();
    const row = page.getByRole('list', { name: 'Work locations' }).getByRole('listitem').filter({ hasText: 'Bangalore office' });
    await expect(row).toContainText('India');
    await expect(row.getByLabel('Works Monday, Tuesday, Wednesday, Thursday, Friday, Saturday')).toBeVisible();
    await axe(page);

    await page.getByRole('tab', { name: 'Holidays' }).click();
    await expect(page.getByText(/Used by Bangalore office/)).toBeVisible();
  });
});
