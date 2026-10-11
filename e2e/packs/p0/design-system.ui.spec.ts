import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

// E2E-P0-05..10 — design-system foundations are accessible (BPMS-010, docs/05 §7, docs/23).
// Runs against the built web app (vite preview). Set WEB_BASE_URL to point elsewhere.
const WEB = process.env.WEB_BASE_URL ?? 'http://127.0.0.1:14173';
const WCAG = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'];

test.describe('design system @a11y', () => {
  test('E2E-P0-05 gallery has no WCAG 2.2 AA violations (light)', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto(`${WEB}/design`);
    await expect(page.getByRole('heading', { level: 1, name: 'Design system' })).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(WCAG).analyze();
    expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
  });

  test('E2E-P0-06 gallery has no WCAG 2.2 AA violations (dark)', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.goto(`${WEB}/design`);
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(WCAG).analyze();
    expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
  });

  test('E2E-P0-07 keyboard: skip link first, focus is always visible', async ({ page }) => {
    await page.goto(`${WEB}/design`);
    await page.keyboard.press('Tab');
    const skip = page.getByRole('link', { name: 'Skip to main content' });
    await expect(skip).toBeFocused();
    await page.keyboard.press('Enter');
    await expect(page.locator('#main')).toBeFocused();
    // Tab through the next interactive elements; each must show a visible focus outline.
    for (let i = 0; i < 6; i++) {
      await page.keyboard.press('Tab');
      const outline = await page.evaluate(() => {
        const el = document.activeElement as HTMLElement | null;
        if (!el || el === document.body) return 'none';
        const s = getComputedStyle(el);
        return `${s.outlineStyle}:${s.outlineWidth}`;
      });
      expect(outline, `focus indicator on tab stop ${i + 1}`).not.toMatch(/^none|:0px$/);
    }
  });

  test('E2E-P0-08 reflows at 320px without horizontal scrolling', async ({ page }) => {
    await page.setViewportSize({ width: 320, height: 800 });
    await page.goto(`${WEB}/design`);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow).toBeLessThanOrEqual(0);
  });

  test('E2E-P0-09 interactive targets are at least 24×24px', async ({ page }) => {
    await page.goto(`${WEB}/design`);
    const small = await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('main button, main a, main input, main label.checkbox, .shell-nav a, header a, header button')]
        // Visually hidden native inputs (React Aria) are not the pointer target; their visible label is measured instead.
        .filter((el) => {
          for (let n: HTMLElement | null = el; n; n = n.parentElement) {
            const s = getComputedStyle(n);
            if (s.clipPath.startsWith('inset(50%') || s.clip.startsWith('rect(0px') || s.opacity === '0') return false;
          }
          return true;
        })
        .map((el) => ({ el: el.textContent?.trim() || el.getAttribute('aria-label') || el.tagName, r: el.getBoundingClientRect() }))
        .filter(({ r }) => r.width > 0 && (r.width < 24 || r.height < 24))
        .map(({ el, r }) => `${el} ${Math.round(r.width)}×${Math.round(r.height)}`),
    );
    expect(small).toEqual([]);
  });
});

test.describe('design system components @a11y', () => {
  test('E2E-P0-10 confirm dialog traps focus, Esc restores focus to the trigger', async ({ page }) => {
    await page.goto(`${WEB}/design`);
    const trigger = page.getByRole('region', { name: 'Dialogs' }).getByRole('button', { name: 'Delete policy' });
    await trigger.click();
    const dialog = page.getByRole('alertdialog', { name: /Delete “Travel policy v3”/ });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused(); // safe default first
    for (let i = 0; i < 4; i++) {
      await page.keyboard.press('Tab');
      expect(await dialog.evaluate((d) => d.contains(document.activeElement))).toBe(true);
    }
    expect((await new AxeBuilder({ page }).include('[role="alertdialog"]').withTags(WCAG).analyze()).violations).toEqual([]);
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(trigger).toBeFocused();
    await trigger.click();
    await page.getByRole('alertdialog').getByRole('button', { name: 'Delete policy' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Policy deleted' })).toBeVisible();
  });

  test('E2E-P0-11 data grid: aria-sort, filter announcements, select all', async ({ page }) => {
    await page.goto(`${WEB}/design`);
    const grid = page.getByRole('table', { name: 'All requests' });
    const due = grid.getByRole('columnheader', { name: /Due/ });
    await expect(due).toHaveAttribute('aria-sort', 'none');
    await due.getByRole('button').click();
    await expect(due).toHaveAttribute('aria-sort', 'ascending');
    await expect(grid.getByRole('row').nth(1)).toContainText('IT-2026-000310');
    await due.getByRole('button').click();
    await expect(due).toHaveAttribute('aria-sort', 'descending');
    await page.getByRole('searchbox', { name: 'Search' }).fill('Purchase');
    await expect(page.getByRole('status').filter({ hasText: 'rows' })).toHaveText('2 of 5 rows');
    await page.getByRole('searchbox', { name: 'Search' }).fill('');
    await grid.getByRole('checkbox', { name: 'Select all rows' }).check();
    await expect(page.getByRole('status').filter({ hasText: 'rows' })).toHaveText('5 of 5 rows · 5 selected');
  });

  test('E2E-P0-12 select is operable with the keyboard only', async ({ page }) => {
    await page.goto(`${WEB}/design`);
    const trigger = page.getByRole('button', { name: /Department/ });
    await trigger.focus();
    await page.keyboard.press('ArrowDown');
    const listbox = page.getByRole('listbox');
    await expect(listbox).toBeVisible();
    await page.keyboard.press('ArrowDown');
    await page.keyboard.press('Enter');
    await expect(listbox).toBeHidden();
    await expect(trigger).toContainText('Human resources');
  });

  test('E2E-P0-13 tabs move with arrow keys and expose the selected state', async ({ page }) => {
    await page.goto(`${WEB}/design`);
    const first = page.getByRole('tab', { name: 'Overview' });
    await first.focus();
    await page.keyboard.press('ArrowRight');
    const sop = page.getByRole('tab', { name: 'SOP' });
    await expect(sop).toBeFocused();
    await expect(sop).toHaveAttribute('aria-selected', 'true');
    await expect(page.getByRole('tabpanel')).toContainText('The governed procedure');
  });

  test('E2E-P0-10 the UI scales with the display, not the window; laptops and short windows stay at 16px; text never shrinks', async ({ browser }) => {
    const root = async (screen: number, window: number, height = 900) => {
      const ctx = await browser.newContext({ viewport: { width: window, height }, screen: { width: screen, height: 1440 } });
      const page = await ctx.newPage();
      await page.goto(`${WEB}/design`);
      await expect(page.getByRole('heading', { level: 1, name: 'Design system' })).toBeVisible();
      const px = await page.evaluate(() => parseFloat(getComputedStyle(document.documentElement).fontSize));
      await ctx.close();
      return px;
    };
    expect(await root(1440, 1440)).toBe(16);
    expect(await root(1280, 800)).toBe(16); // small displays never go below 16px
    expect(await root(1920, 1920)).toBeCloseTo(17.71, 1);
    expect(await root(2560, 2560)).toBe(20);
    expect(await root(2560, 1280)).toBe(20); // a half-width window on a big display keeps its scale
    expect(await root(2560, 1280, 720)).toBe(16); // a short window keeps the base size so dialogs fit
  });
});
