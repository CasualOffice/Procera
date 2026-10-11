import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { inviteToken, workspaceWithOwner } from '../../fixtures/workspace';
import { overdueCampaign } from '../../fixtures/library';

// E2E-R1-UI-23 — admin overview and people & access in the browser (BPMS-063).

test.describe('admin @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-23 an admin sees what needs attention and drills into people without two-step verification', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const email = await seedUser(request, 'uimember');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
    const token = await inviteToken(request, ws.id, email);
    const pw = await passwordSession(email);
    await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
    await overdueCampaign(request, owner, ws.id);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Overview');
    await expect(page.getByRole('heading', { level: 1, name: 'Overview' })).toBeVisible();
    await expect(page.getByRole('link', { name: '2 overdue acknowledgements', exact: true })).toBeVisible();
    const attention = page.getByRole('region', { name: 'Needs attention' });
    await expect(attention.getByRole('link', { name: /2 overdue acknowledgements for/ })).toBeVisible();
    await expect(page.getByRole('link', { name: /1\s*without two-step verification/ })).toBeVisible();
    await axe(page);

    await page.getByRole('link', { name: /without two-step verification/ }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'People & access' })).toBeVisible();
    await expect(page.getByRole('button', { name: /No two-step verification/ })).toHaveAttribute('aria-pressed', 'true');
    const table = page.getByRole('table', { name: 'People' });
    await expect(table.getByRole('row')).toHaveCount(2); // header + the member
    await expect(table.getByText(email)).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: /^Admins/ }).click();
    await expect(table.getByText(ownerEmail)).toBeVisible();
    await expect(table.getByText(email)).toHaveCount(0);
  });
});
