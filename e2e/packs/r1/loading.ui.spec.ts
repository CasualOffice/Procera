import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-42: on a slow connection every page shows placeholders shaped like its content while it
// loads (docs/05 §3, UX-005), never a blank area or a bare spinner, and then the content.

const SLOW = /\/api\/v1\/(artifacts|members|campaigns|announcements|me\/acknowledgements|admin\/overview|admin\/people|settings|audit\/events|notifications)(\?|$|\/)/;

test.describe('loading states @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-42 every main page shows a content-shaped skeleton while its data loads', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();

    // Data requests wait at a gate the test opens once the skeleton has been seen: no timing guesses.
    let open: () => void = () => undefined;
    let gate: Promise<void> = Promise.resolve();
    const hold = () => { gate = new Promise<void>((r) => { open = r; }); };
    await page.route(SLOW, async (route) => {
      if (route.request().method() === 'GET') await gate;
      await route.continue();
    });

    const pages: [string, string, RegExp | string][] = [
      ['Library', 'Loading documents', 'Library'],
      ['Members', 'Loading members', 'Members'],
      ['Campaigns', 'Loading campaigns', 'Campaigns'],
      ['Manage announcements', 'Loading announcements', 'Manage announcements'],
      ['Overview', 'Loading overview', 'Overview'],
      ['People & access', 'Loading people', /People/],
      ['Settings', 'Loading settings', 'Settings'],
      ['Audit log', 'Loading events', 'Audit log'],
      ['Notifications', 'Loading notifications', 'Notifications'],
      ['Home', 'Loading your tasks', /^Good (morning|afternoon|evening)/],
    ];
    for (const [link, skeleton, heading] of pages) {
      hold();
      await nav(page, link);
      await expect(page.getByRole('status', { name: skeleton }), `${link} shows a skeleton`).toBeVisible();
      open();
      await expect(page.getByRole('status', { name: skeleton }), `${link} replaces it with content`).toHaveCount(0);
      await expect(page.getByRole('heading', { level: 1, name: heading }).first()).toBeVisible();
    }

    // A full reload shows a placeholder frame first, not a blank page.
    hold();
    await page.reload();
    await expect(page.getByRole('status', { name: /Loading/ }).first()).toBeVisible();
    open();
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
  });
});
