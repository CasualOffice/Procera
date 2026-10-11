import { type Page, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { axe, signIn, WEB } from '../../fixtures/ui';
import { inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-62 — persona navigation (docs/05 §2.1, approved 2026-10-12): an employee sees three
// everyday items; an owner gets one Admin door into grouped admin pages and back; area tabs.
// E2E-R1-UI-63 — workspace onboarding: the owner's setup checklist and the four-step /welcome.

const sidebarLinks = (page: Page) => page.getByRole('navigation', { name: 'Main' }).getByRole('listitem').getByRole('link');

test.describe('navigation and onboarding @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-62 an employee sees Home, Library and Announcements; an owner reaches grouped admin pages through Admin and back', async ({ browser, page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const email = uniqueEmail('finn');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
    const token = await inviteToken(request, ws.id, email);
    expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: 'Finn', password: PW } })).status()).toBe(201);

    // Employee: three everyday items and the bell, nothing administrative.
    const finn = await (await browser.newContext()).newPage();
    await finn.goto(`${WEB}/sign-in`);
    await finn.getByRole('textbox', { name: 'Work email' }).fill(email);
    await finn.getByRole('textbox', { name: 'Password', exact: true }).fill(PW);
    await finn.getByRole('button', { name: 'Sign in' }).click();
    await expect(finn.getByRole('heading', { name: 'Needs you' })).toBeVisible();
    await expect(sidebarLinks(finn)).toHaveText(['Home', 'Library', 'Announcements']);
    await expect(finn.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: /^Notifications/ })).toBeVisible();
    await axe(finn);

    // Owner: everyday items plus Admin; the hub groups admin pages and explains each one.
    await signIn(page, ownerEmail, owner.recovery![0]!);
    const main = page.getByRole('navigation', { name: 'Main' });
    await expect(sidebarLinks(page)).toHaveText(['Home', 'Library', 'Announcements', 'My documents', 'Campaigns', 'Admin']);
    await main.getByRole('link', { name: 'Admin', exact: true }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Admin' })).toBeVisible();
    for (const group of ['People', 'Data & security', 'Workspace']) await expect(page.getByRole('heading', { level: 2, name: group })).toBeVisible();
    await expect(main.getByText('Admin', { exact: true })).toBeVisible(); // admin mode label
    await axe(page);
    await main.getByRole('link', { name: 'Members' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Members' })).toBeVisible();
    await expect(main.getByRole('link', { name: 'Members' })).toHaveAttribute('aria-current', 'page');
    await main.getByRole('link', { name: 'Back to workspace' }).click();
    await expect(page.getByRole('heading', { name: 'Needs you' })).toBeVisible();

    // Area tabs: Library views and Announcements views keep their own URLs.
    await main.getByRole('link', { name: 'Library' }).click();
    const views = page.getByRole('navigation', { name: 'Library views' });
    await views.getByRole('link', { name: 'Applies to me' }).click();
    await expect(page).toHaveURL(/\/policies$/);
    await expect(main.getByRole('link', { name: 'Library' })).toHaveAttribute('aria-current', 'page');
    await main.getByRole('link', { name: 'My documents' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'My documents' })).toBeVisible();
    await main.getByRole('link', { name: 'Announcements' }).click();
    await page.getByRole('navigation', { name: 'Announcement views' }).getByRole('link', { name: 'Manage' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Manage announcements' })).toBeVisible();
    await axe(page);
  });

  test('E2E-R1-UI-63 an owner follows the setup checklist through the guided setup and sees it tick', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    const setup = page.getByRole('region', { name: 'Set up Acme' });
    await expect(setup).toBeVisible();
    await expect(setup.getByRole('progressbar', { name: 'Setup progress' })).toBeVisible();
    await axe(page);
    await setup.getByRole('link', { name: /^(Start|Continue) setup$/ }).click();

    // 1 Your workspace.
    await expect(page.getByRole('heading', { level: 1, name: 'Set up Acme' })).toBeVisible();
    await page.getByRole('textbox', { name: 'Workspace name' }).fill('Acme Labs');
    await page.getByLabel('Accent colour').fill('#1f4e79');
    await page.getByRole('button', { name: 'Save and continue' }).click();
    // 2 Starting point.
    await expect(page.getByRole('heading', { level: 2, name: 'Starting point' })).toBeVisible();
    await page.getByRole('button', { name: 'Install and continue' }).click();
    // 3 Departments.
    await expect(page.getByRole('heading', { level: 2, name: 'Departments' })).toBeVisible();
    await page.getByLabel('Departments, one per line').fill('People\nFinance\n');
    await page.getByRole('button', { name: 'Add 2 and continue' }).click();
    // 4 Invite people.
    await expect(page.getByRole('heading', { level: 2, name: 'Invite people' })).toBeVisible();
    await axe(page);
    await page.getByRole('textbox', { name: 'Email 1' }).fill(uniqueEmail('rev'));
    await page.getByRole('button', { name: /Role 1/ }).click();
    await page.getByRole('option', { name: 'Reviewer' }).click();
    await page.getByRole('button', { name: 'Invite 1 and finish' }).click();

    // Back on Home: the renamed workspace and three steps ticked from real data.
    await expect(page.getByRole('region', { name: 'Set up Acme Labs' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Set up Acme Labs' }).getByText(/^[3-9] of 7 done/)).toBeVisible();
    const depts = await (await owner.c.get('/api/v1/departments', { headers: owner.headers })).json();
    expect(depts.items.map((d: { name: string }) => d.name).sort()).toEqual(['Finance', 'People']);
    const docs = await (await owner.c.get('/api/v1/artifacts', { headers: owner.headers })).json();
    expect(docs.items.length).toBeGreaterThan(0); // the starter packs installed as drafts
  });
});
