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

/** Navigates with the sidebar (client-side), like a user would. */
export async function nav(page: Page, label: string) {
  await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: new RegExp(`^${label}`) }).click();
}

/** Settings › a category: the Settings index lists the categories, and each one is its own page. */
export async function openSettings(page: Page, category: string) {
  await nav(page, 'Settings');
  await page.getByRole('navigation', { name: 'Settings categories' }).getByRole('link', { name: category, exact: true }).click();
  await expect(page.getByRole('heading', { level: 1, name: category })).toBeVisible();
}
