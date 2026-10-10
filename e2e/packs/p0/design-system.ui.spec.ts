import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

// E2E-P0-05..09 — design-system foundations are accessible (BPMS-010, docs/05 §7, docs/23).
// Runs against the built web app (vite preview). Set WEB_BASE_URL to point elsewhere.
const WEB = process.env.WEB_BASE_URL ?? 'http://127.0.0.1:14173';
const WCAG = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'];

test.describe('design system @a11y', () => {
  test('E2E-P0-05 gallery has no WCAG 2.2 AA violations (light)', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await page.goto(WEB);
    await expect(page.getByRole('heading', { level: 1, name: 'Design system' })).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(WCAG).analyze();
    expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
  });

  test('E2E-P0-06 gallery has no WCAG 2.2 AA violations (dark)', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.goto(WEB);
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    const results = await new AxeBuilder({ page }).withTags(WCAG).analyze();
    expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
  });

  test('E2E-P0-07 keyboard: skip link first, focus is always visible', async ({ page }) => {
    await page.goto(WEB);
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
    await page.goto(WEB);
    const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    expect(overflow).toBeLessThanOrEqual(0);
  });

  test('E2E-P0-09 interactive targets are at least 24×24px', async ({ page }) => {
    await page.goto(WEB);
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
