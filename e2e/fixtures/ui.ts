import AxeBuilder from '@axe-core/playwright';
import { type Page, expect } from '@playwright/test';
import { PW } from './accounts';

export const WEB = process.env.WEB_BASE_URL ?? 'http://127.0.0.1:14173';
const WCAG = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'];

export async function axe(page: Page) {
  const r = await new AxeBuilder({ page }).withTags(WCAG).analyze();
  expect(r.violations, JSON.stringify(r.violations, null, 2)).toEqual([]);
}

/** Signs in through the UI with password, then a recovery code (single use, no clock dependence). */
export async function signIn(page: Page, email: string, recovery: string) {
  await page.goto(`${WEB}/`);
  await expect(page).toHaveURL(/\/sign-in$/);
  await page.getByLabel('Work email').fill(email);
  await page.getByLabel('Password').fill(PW);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('heading', { level: 1, name: 'Two-step verification' })).toBeVisible();
  await page.getByLabel('Code').fill(recovery);
  await page.getByRole('button', { name: 'Verify' }).click();
}

/** Pages reached through a tab of another area (docs/05 §2.1): [sidebar item, tab]. */
const VIA_TAB: Record<string, [string, string]> = {
  Policies: ['Library', 'Applies to me'],
  'Manage announcements': ['Announcements', 'Manage'],
};

/**
 * Navigates like a person would (client-side): the sidebar shows everyday items, and admin pages
 * sit behind "Admin" (admin mode has "Back to workspace"). Tab pages go through their area.
 */
export async function nav(page: Page, label: string) {
  const main = page.getByRole('navigation', { name: 'Main' });
  await expect(main.getByRole('link').first()).toBeVisible();
  const via = VIA_TAB[label];
  if (via) {
    await nav(page, via[0]);
    await page.getByRole('navigation', { name: /views$/ }).getByRole('link', { name: via[1], exact: true }).click();
    return;
  }
  const link = main.getByRole('link', { name: new RegExp(`^${label}`) });
  if ((await link.count()) === 0) {
    const admin = main.getByRole('link', { name: 'Admin', exact: true });
    await ((await admin.count()) > 0 ? admin : main.getByRole('link', { name: 'Back to workspace' })).click();
  }
  await link.first().click();
}
