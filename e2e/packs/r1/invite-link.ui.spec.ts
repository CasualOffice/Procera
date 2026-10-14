import { type Browser, type Page, expect, test } from '@playwright/test';
import { PW, hasDatabase, mfaSession, seedUser, uniqueEmail } from '../../fixtures/accounts';
import { axe, nav, signIn, WEB } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// Invitation links for when email isn't set up (BPMS-023, BPMS-020; docs/08 "Members", "System API").
// E2E-R1-UI-71 — a system admin copies the owner's invitation link after creating a workspace and again
//                from its row; the newer link works and the earlier one no longer does.
// E2E-R1-UI-72 — a workspace admin copies the link of a pending invitation on Members; it opens the join page.

/** Reads the link out of the "Invitation link" dialog and closes it. */
async function readLink(page: Page, email: string) {
  const dialog = page.getByRole('dialog', { name: 'Invitation link' });
  await expect(dialog.getByText(`Anyone with this link can join as ${email}. It works once and expires in 7 days.`)).toBeVisible();
  const link = await dialog.getByRole('textbox', { name: 'Invitation link' }).inputValue();
  expect(link).toMatch(/\/invite\?token=/);
  await expect(dialog.getByRole('button', { name: 'Copy link' })).toBeVisible();
  await axe(page);
  await dialog.getByRole('button', { name: 'Done' }).click();
  await expect(dialog).toHaveCount(0);
  return link;
}

async function open(browser: Browser, link: string) {
  const p = await (await browser.newContext()).newPage();
  await p.goto(link);
  return p;
}

test.describe('invitation links @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-71 a system admin copies the owner invitation link after creating a workspace and from its row', async ({ page, browser, request }) => {
    const sysEmail = await seedUser(request, 'sysadmin');
    await request.post('/__test/system-roles', { data: { email: sysEmail, role: 'system.admin' } });
    const sys = await mfaSession(sysEmail);
    await page.goto(`${WEB}/system`);
    await page.getByLabel('Work email').fill(sysEmail);
    await page.getByLabel(/^Password/).fill(PW);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    await page.getByLabel('Code').fill(sys.recovery[0]!);
    await page.getByRole('button', { name: 'Verify' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Workspaces' })).toBeVisible();

    const name = `Linkfield ${Date.now().toString(36)}`;
    const owner = uniqueEmail('link-owner');
    await page.getByRole('button', { name: 'New workspace' }).first().click();
    const dlg = page.getByRole('dialog', { name: 'New workspace' });
    await dlg.getByRole('textbox', { name: 'Workspace name' }).fill(name);
    await dlg.getByRole('textbox', { name: 'Owner email' }).fill(owner);
    await dlg.getByRole('button', { name: 'Create workspace' }).click();
    await expect(page.getByText(`${name} was created`)).toBeVisible();

    await page.getByRole('button', { name: 'Copy owner invite link' }).click();
    const first = await readLink(page, owner);

    // Later, from the workspace row: a new link, and the first one stops working.
    await page.getByRole('button', { name: `${name}, open details` }).click();
    const drawer = page.getByRole('dialog', { name });
    await drawer.getByRole('button', { name: 'Copy owner invite link' }).click();
    const second = await readLink(page, owner);
    expect(second).not.toBe(first);

    const stale = await open(browser, first);
    await expect(stale.getByRole('heading', { level: 1, name: 'This invitation link doesn’t work' })).toBeVisible();
    const fresh = await open(browser, second);
    await expect(fresh.getByRole('heading', { level: 1, name: `Join ${name}` })).toBeVisible();
    await expect(fresh.getByText(owner, { exact: true })).toBeVisible();

    const ws = (await (await sys.c.get('/sysapi/v1/workspaces', { headers: sys.headers })).json()).items.find((w: { name: string }) => w.name === name);
    const history = await (await sys.c.get(`/sysapi/v1/system-audit/events?object_type=workspace&object_id=${ws.id}`, { headers: sys.headers })).json();
    expect(history.items.filter((e: { action: string }) => e.action === 'workspace.owner_invitation_link_copied')).toHaveLength(2);
  });

  test('E2E-R1-UI-72 a workspace admin copies the link of a pending invitation on Members', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const pending = uniqueEmail('pending-link');
    expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email: pending, roles: ['member'] } })).ok()).toBe(true);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Members');
    await expect(page.getByRole('heading', { level: 1, name: 'Members' })).toBeVisible();

    await page.getByRole('button', { name: `Copy invite link for ${pending}` }).click();
    const link = await readLink(page, pending);
    const join = await open(browser, link);
    await expect(join.getByRole('heading', { level: 1, name: `Join ${ws.name}` })).toBeVisible();
    await expect(join.getByText(pending, { exact: true })).toBeVisible();

    // The token never appears in the invitation list.
    const list = await (await owner.c.get('/api/v1/members/invitations', { headers: owner.headers })).text();
    expect(list).not.toContain(new URL(link).searchParams.get('token')!);
  });
});
