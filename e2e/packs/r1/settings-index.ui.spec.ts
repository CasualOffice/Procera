import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { WEB, axe, nav, openSettings, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-61 — Settings as a category index: categories with descriptions and changed counts,
// search across every setting, category pages with a collapsed Advanced section, keys and choices
// for unshipped features hidden, department overrides on the setting, the starting-point banner and
// old anchors redirected (BPMS-027; docs/05 §3.16; docs/design/ia-competitive-analysis.md §6).

const CATEGORIES = ['General & branding', 'Sign-in & security', 'People & directory', 'Documents & reviews', 'Acknowledgements & announcements', 'Notifications', 'Data, retention & files'];

test.describe('settings index @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-61 settings index: find a setting, open a category, change a value, Advanced, hidden keys, department override, starting point', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request, 'standard');
    const fin = await (await owner.c.post('/api/v1/departments', { headers: owner.headers, data: { name: 'Finance' } })).json();
    const set = await owner.c.put(`/api/v1/settings/departments/${fin.id}/governance.review.min_reviewers`, { headers: { ...owner.headers, 'if-match': '"0"' }, data: { value: 2 } });
    expect(set.status()).toBe(200);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Settings');
    await expect(page.getByRole('heading', { level: 1, name: 'Settings' })).toBeVisible();
    // The index: seven categories, each with its one-line description, and the starting point once.
    const cats = page.getByRole('navigation', { name: 'Settings categories' });
    await expect(cats.getByRole('link')).toHaveCount(CATEGORIES.length);
    for (const c of CATEGORIES) await expect(cats.getByRole('link', { name: c, exact: true })).toBeVisible();
    await expect(cats.getByText('Upload limits, malware scanning, and how long records are kept')).toBeVisible();
    await expect(page.getByRole('heading', { level: 2, name: 'Starting point: Standard' })).toBeVisible();
    await expect(page.getByRole('tab')).toHaveCount(0); // no more scope tabs
    await axe(page);

    // Search covers labels, help, keys and synonyms; settings for unshipped features never match.
    const search = page.getByRole('searchbox', { name: 'Search settings' });
    for (const hidden of ['request numbering', 'API tokens', 'out-of-office delegate', 'stuck']) {
      await search.fill(hidden);
      await expect(page.getByRole('status').filter({ hasText: 'No settings match' })).toBeVisible();
    }
    await search.fill('mfa'); // a synonym: neither label says "MFA"
    const results = page.getByRole('region', { name: 'Search results' });
    await expect(results.getByText('2 settings match')).toBeVisible();
    await expect(results.getByRole('link', { name: 'Ways people can confirm the second step' })).toBeVisible();
    await results.getByRole('link', { name: 'Two-step verification required for' }).click();
    await expect(page).toHaveURL(/\/admin\/settings\/security#set-auth-mfa-required_roles$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Sign-in & security' })).toBeVisible();
    await expect(page.getByRole('list', { name: 'Breadcrumb' }).getByRole('link', { name: 'Settings' })).toBeVisible();

    // Only roles this workspace can use are offered for two-step verification.
    const mfa = page.getByRole('listitem').filter({ has: page.getByText('Two-step verification required for', { exact: true }) });
    await mfa.getByRole('button', { name: 'Change Two-step verification required for' }).click();
    await expect(mfa.getByRole('button', { name: 'Reviewer' })).toBeVisible();
    for (const unshipped of ['Case manager', 'Investigator', 'Analyst']) await expect(mfa.getByRole('button', { name: unshipped, exact: true })).toHaveCount(0);
    await mfa.getByRole('button', { name: 'Cancel' }).click();

    // Rare settings wait behind a collapsed Advanced section; the API-token key is hidden even there.
    const advanced = page.getByRole('button', { name: /^Advanced/ });
    await expect(advanced).toHaveAttribute('aria-expanded', 'false');
    await expect(page.getByText('Sign out after, at the latest', { exact: true })).toHaveCount(0);
    await advanced.click();
    await expect(advanced).toHaveAttribute('aria-expanded', 'true');
    await expect(page.getByText('Sign out after, at the latest', { exact: true })).toBeVisible();
    await expect(page.getByText('Log when admins open content', { exact: true })).toBeVisible();
    await expect(page.getByText('Longest lifetime for API tokens')).toHaveCount(0);

    // A category page: the department override shows on the setting itself.
    await page.getByRole('list', { name: 'Breadcrumb' }).getByRole('link', { name: 'Settings' }).click();
    await cats.getByRole('link', { name: 'Documents & reviews', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Documents & reviews' })).toBeVisible();
    const reviewers = page.getByRole('listitem').filter({ has: page.getByText('Approvals needed before publishing', { exact: true }) });
    await reviewers.getByRole('button', { name: 'Different in 1 department: Approvals needed before publishing' }).click();
    const panel = page.getByRole('dialog', { name: 'Approvals needed before publishing by department' });
    const finRow = panel.getByRole('listitem').filter({ has: page.getByText('Finance', { exact: true }) });
    await expect(finRow.getByText('Set for Finance')).toBeVisible();
    await expect(finRow.getByText('2', { exact: true })).toBeVisible();
    await panel.getByRole('button', { name: 'Close' }).click();

    // Change an everyday setting in place.
    const reason = page.getByRole('listitem').filter({ has: page.getByText('Ask what changed when publishing a new version', { exact: true }) });
    await reason.getByRole('button', { name: 'Change Ask what changed when publishing a new version' }).click();
    await reason.getByRole('switch', { name: 'Ask what changed when publishing a new version' }).click();
    await reason.getByRole('button', { name: 'Save' }).click();
    await expect(reason.getByText('Off', { exact: true })).toBeVisible();
    await axe(page);
    const e = await (await owner.c.get('/api/v1/settings/effective?key=governance.reason_for_change.required', { headers: owner.headers })).json();
    expect([e.value, e.source]).toEqual([false, 'workspace']);

    // The index counts it as changed from the starting point.
    await page.getByRole('list', { name: 'Breadcrumb' }).getByRole('link', { name: 'Settings' }).click();
    await expect(cats.getByRole('listitem').filter({ has: page.getByRole('link', { name: 'Documents & reviews', exact: true }) }).getByText('1 changed')).toBeVisible();

    // Channels that don't exist yet aren't offered.
    await openSettings(page, 'Notifications');
    const channels = page.getByRole('listitem').filter({ has: page.getByText('Send notifications by', { exact: true }) });
    await channels.getByRole('button', { name: 'Change Send notifications by' }).click();
    await expect(channels.getByRole('button', { name: 'Email' })).toBeVisible();
    await expect(channels.getByRole('button', { name: 'In the app' })).toBeVisible();
    for (const unavailable of ['Push', 'Webhook', 'Chat']) await expect(channels.getByRole('button', { name: unavailable, exact: true })).toHaveCount(0);
    await channels.getByRole('button', { name: 'Cancel' }).click();

    // Links to the old single page still land in the right place.
    await page.goto(`${WEB}/admin/settings#set-files`);
    await expect(page).toHaveURL(/\/admin\/settings\/data$/);
    await expect(page.getByRole('heading', { level: 1, name: 'Data, retention & files' })).toBeVisible();
  });
});
