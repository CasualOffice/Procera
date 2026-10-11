import { type APIRequestContext, type Locator, type Page, expect, test } from '@playwright/test';
import { PW, hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { WEB, axe, nav, signIn } from '../../fixtures/ui';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1 release gate (docs/23 §R1, BPMS-032): the scenarios whose features have shipped, run in
// the browser. R1-16 deactivation cuts access at once; R1-19 keyboard-only publish and acknowledge.

/** A member (Member role) who joined `ws` through an invitation; signs in with a password only. */
async function member(request: APIRequestContext, owner: Session, ws: string, tag: string): Promise<{ email: string; id: string }> {
  const email = await seedUser(request, tag);
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).status()).toBe(201);
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const acc = await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
  expect(acc.status()).toBe(200);
  return { email, id: (await acc.json()).membership_id };
}

async function signInPassword(page: Page, email: string) {
  await page.goto(`${WEB}/sign-in`);
  await page.getByLabel('Work email').fill(email);
  await page.getByLabel('Password').fill(PW);
  await page.getByRole('button', { name: 'Sign in' }).click();
  await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
}

/** Moves focus with Tab (and only Tab) until `target` has it. */
async function tabTo(page: Page, target: Locator, max = 120) {
  for (let i = 0; i < max; i++) {
    if (await target.evaluate((el) => el === document.activeElement || el.contains(document.activeElement)).catch(() => false)) return;
    await page.keyboard.press('Tab');
  }
  throw new Error(`could not reach ${target} with the keyboard`);
}

test.describe('R1 gate @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-16 a deactivated member loses access at once: their open session stops working and they cannot get back in', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const m = await member(request, owner, ws.id, 'leaver');
    const theirs = await (await browser.newContext()).newPage();
    await signInPassword(theirs, m.email);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await page.goto(`${WEB}/members/${m.id}`);
    await page.getByRole('button', { name: 'Deactivate' }).click();
    const dlg = page.getByRole('dialog', { name: /^Deactivate/ });
    await axe(page);
    await dlg.getByRole('button', { name: 'Deactivate' }).click();
    await expect(page.getByText('can no longer sign in to this workspace')).toBeVisible();
    await expect(page.getByText('Deactivated')).toBeVisible();

    // Their open tab: the next page they open sends them out of the workspace.
    await nav(theirs, 'Library');
    await expect(theirs.getByRole('heading', { level: 1, name: 'Choose a workspace' })).toBeVisible();
    await expect(theirs.getByText('You’re not in a workspace yet')).toBeVisible();
    // Signing in again does not bring the workspace back.
    const fresh = await (await browser.newContext()).newPage();
    await fresh.goto(`${WEB}/sign-in`);
    await fresh.getByLabel('Work email').fill(m.email);
    await fresh.getByLabel('Password').fill(PW);
    await fresh.getByRole('button', { name: 'Sign in' }).click();
    await expect(fresh.getByRole('heading', { level: 1, name: 'Choose a workspace' })).toBeVisible();
    await expect(fresh.getByRole('button', { name: /Acme/ })).toHaveCount(0);
    // And the API agrees.
    const pw = await passwordSession(m.email);
    expect((await pw.c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${pw.token}` }, data: { workspace_id: ws.id } })).status()).toBe(404);
  });

  test('E2E-R1-19 keyboard only: an author publishes a policy and a colleague acknowledges it without a mouse', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const m = await member(request, owner, ws.id, 'keyboard');
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();

    // Author, keyboard only from here.
    await tabTo(page, page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: /^Library/ }));
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
    await tabTo(page, page.getByRole('button', { name: 'New document' }).first());
    await page.keyboard.press('Enter');
    const create = page.getByRole('dialog', { name: 'New document' });
    await tabTo(page, create.getByLabel('Title'));
    await page.keyboard.type('Clean desk');
    await tabTo(page, create.getByRole('button', { name: 'Create draft' }));
    await page.keyboard.press('Enter');
    await expect(page.getByRole('heading', { level: 1, name: 'Clean desk' })).toBeVisible();
    await tabTo(page, page.getByRole('button', { name: 'Edit', exact: true }));
    await page.keyboard.press('Enter');
    await tabTo(page, page.getByRole('textbox', { name: 'Content' }));
    await page.keyboard.type('Clear your desk at the end of each day.');
    await tabTo(page, page.getByRole('button', { name: 'Save draft' }));
    await page.keyboard.press('Enter');
    await expect(page.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible();
    await tabTo(page, page.getByRole('button', { name: 'Submit for review' }));
    await page.keyboard.press('Enter');
    await tabTo(page, page.getByRole('button', { name: 'Publish' }));
    await page.keyboard.press('Enter');
    const publish = page.getByRole('dialog', { name: 'Publish v1' });
    await tabTo(page, publish.getByRole('button', { name: 'Publish now' }));
    await page.keyboard.press('Enter');
    await expect(page.getByRole('status').filter({ hasText: 'Published.' })).toBeVisible();

    // Setup outside the scenario: an acknowledgement campaign for everyone.
    const art = (await (await owner.c.get('/api/v1/artifacts', { headers: owner.headers })).json()).items.find((a: { title: string }) => a.title === 'Clean desk');
    const camp = await (await owner.c.post('/api/v1/campaigns', { headers: owner.headers, data: { version_id: art.current_published_version_id, title: 'Clean desk', audience: { all: true } } })).json();
    expect((await owner.c.post(`/api/v1/campaigns/${camp.id}/activate`, { headers: owner.headers })).status()).toBe(200);

    // Colleague, keyboard only: from Home to the acknowledgement.
    const them = await (await browser.newContext()).newPage();
    await them.goto(`${WEB}/sign-in`);
    await tabTo(them, them.getByLabel('Work email'));
    await them.keyboard.type(m.email);
    await tabTo(them, them.getByLabel('Password'));
    await them.keyboard.type(PW);
    await them.keyboard.press('Enter');
    await expect(them.getByText('Acknowledge “Clean desk”')).toBeVisible();
    await tabTo(them, them.getByRole('button', { name: 'Read and acknowledge' }));
    await them.keyboard.press('Enter');
    const callout = them.getByRole('region', { name: 'Acknowledgement requested' });
    await tabTo(them, callout.getByRole('button', { name: 'I acknowledge' }));
    await them.keyboard.press('Enter');
    await expect(them.getByRole('status').filter({ hasText: 'Acknowledged' })).toBeVisible();
    const progress = await (await owner.c.get(`/api/v1/campaigns/${camp.id}`, { headers: owner.headers })).json();
    expect(progress.progress.acknowledged).toBe(1);
  });
});
