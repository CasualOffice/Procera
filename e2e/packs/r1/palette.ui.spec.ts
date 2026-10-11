import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { WEB, axe, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-12..13 — the command palette (⌘K, docs/05 §2.4) and the shared states (UX-005) in the
// browser (BPMS-031, BPMS-049).

const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });

test.describe('command palette and states @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-12 ⌘K finds a document by its content and opens it; commands navigate; recent items are offered; fully keyboard driven', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key: 'remote-work', title: 'Remote work', content: doc('Everyone may work from home two days a week with manager agreement.') } });
    expect(r.status()).toBe(201);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { name: 'Needs you' })).toBeVisible();

    // Keyboard: Ctrl/⌘ K opens; typing searches content; Enter opens the highlighted result.
    await page.keyboard.press('ControlOrMeta+k');
    const palette = page.getByRole('dialog', { name: 'Search and commands' });
    const input = palette.getByRole('combobox');
    await expect(input).toBeFocused();
    await input.fill('manager agreement');
    const hit = palette.getByRole('option', { name: /Remote work/ });
    await expect(hit).toBeVisible();
    await expect(hit.locator('mark').first()).toBeVisible(); // highlighted snippet
    await expect(hit).toHaveAttribute('aria-selected', 'true');
    await axe(page);
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Remote work' })).toBeVisible();
    await expect(palette).toHaveCount(0);

    // The opened document is now a recent item; arrows move, Enter runs a command.
    await page.getByRole('button', { name: /^Search/ }).click();
    await expect(palette.getByRole('group', { name: 'Recent' }).getByRole('option', { name: 'Remote work' })).toBeVisible();
    await input.fill('members');
    await expect(palette.getByRole('option', { name: 'Go to Members' })).toHaveAttribute('aria-selected', 'true');
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Members' })).toBeVisible();

    // No match explains itself; Escape closes and focus returns to the page.
    await page.keyboard.press('ControlOrMeta+k');
    await input.fill('zzqxv');
    await expect(palette.getByText('Nothing matches “zzqxv”')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(palette).toHaveCount(0);
  });

  test('E2E-R1-UI-13 designed states: a new workspace shows composed empty states, an unknown page offers a way back, a failed load offers a retry', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByText('You’re all caught up')).toBeVisible();
    await page.goto(`${WEB}/library`);
    await expect(page.getByText('Your library is empty')).toBeVisible();
    await page.goto(`${WEB}/no-such-page`);
    await expect(page.getByRole('heading', { level: 1, name: 'Page not found' })).toBeVisible();
    await axe(page);
    await page.getByRole('link', { name: 'Back to Home' }).click();
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();

    // A failing request shows the error with a retry, not an empty list.
    await page.route('**/api/v1/notifications', (route) => route.fulfill({ status: 503, contentType: 'application/problem+json', body: JSON.stringify({ status: 503, code: 'unavailable', title: 'Service unavailable', detail: 'The database is not reachable.' }) }));
    await page.goto(`${WEB}/notifications`);
    const alert = page.getByRole('alert').filter({ hasText: 'Couldn’t load notifications' });
    await expect(alert).toBeVisible();
    await expect(alert.getByText('The database is not reachable.')).toBeVisible();
    await axe(page);
    await page.unroute('**/api/v1/notifications');
    await alert.getByRole('button', { name: 'Try again' }).click();
    await expect(alert).toHaveCount(0);
  });
});
