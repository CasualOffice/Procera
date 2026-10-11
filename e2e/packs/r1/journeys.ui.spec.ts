import { expect, test } from '@playwright/test';
import { PW, hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { totp } from '../../fixtures/totp';
import { WEB } from '../../fixtures/ui';
import { inviteToken } from '../../fixtures/workspace';
import { axe, nav, signIn } from '../../fixtures/ui';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-07..11 — end-to-end journeys in the browser: author and publish a policy, acknowledge a
// campaign, invite a colleague, read and acknowledge an announcement (BPMS-031/040/046/047/023).

const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });

test.describe('journeys @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // each journey provisions a workspace and signs in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-07 author a policy: create, write, submit and publish from the library', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Library');
    await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
    // The page header always offers it; an empty library repeats it in the empty state.
    await page.getByRole('button', { name: 'New document' }).first().click();
    const dialog = page.getByRole('dialog', { name: 'New document' });
    await dialog.getByLabel('Title').fill('Remote work');
    await axe(page);
    await dialog.getByRole('button', { name: 'Create draft' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Remote work' })).toBeVisible();
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    // The block editor (BPMS-042): markdown shortcuts as you type ("## " heading, "- " list).
    await page.getByRole('textbox', { name: 'Content' }).click();
    await page.keyboard.type('## Eligibility');
    await page.keyboard.press('Enter');
    await page.keyboard.type('Everyone may work remotely two days a week.');
    await page.keyboard.press('Enter');
    await page.keyboard.type('- Agree days with your manager');
    await page.keyboard.press('Enter');
    await page.keyboard.type('Keep core hours 10–15');
    await page.getByRole('button', { name: 'Save draft' }).click();
    await expect(page.getByRole('heading', { name: 'Eligibility' })).toBeVisible();
    await expect(page.getByRole('listitem').filter({ hasText: 'Keep core hours' })).toBeVisible();
    await page.getByRole('button', { name: 'Submit for review' }).click();
    await expect(page.getByText('Approved', { exact: true })).toBeVisible(); // a standard policy needs no review in a Standard workspace
    await page.getByRole('button', { name: 'Publish' }).click();
    await page.getByRole('dialog', { name: 'Publish v1' }).getByRole('button', { name: 'Publish now' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Published.' })).toBeVisible();
    await expect(page.getByText('Fingerprint')).toBeVisible();
    await axe(page);
    await nav(page, 'Library');
    const row = page.getByRole('row', { name: /Remote work/ });
    await expect(row.getByText('Published')).toBeVisible();
  });

  test('E2E-R1-UI-08 acknowledge a policy from Home: the task leads to the document and the acknowledgement is recorded', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const { version } = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key: 'conduct', title: 'Code of Conduct', content: doc('Be kind.') } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: owner.headers });
    await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: owner.headers, data: {} });
    const camp = await (await owner.c.post('/api/v1/campaigns', { headers: owner.headers, data: { version_id: version.id, title: 'Annual attestation', audience: { all: true } } })).json();
    expect((await owner.c.post(`/api/v1/campaigns/${camp.id}/activate`, { headers: owner.headers })).status()).toBe(200);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByText('Acknowledge “Code of Conduct”')).toBeVisible();
    await page.getByRole('button', { name: 'Read and acknowledge' }).click();
    const callout = page.getByRole('region', { name: 'Acknowledgement requested' });
    await expect(callout).toBeVisible();
    await axe(page);
    await callout.getByRole('button', { name: 'I acknowledge' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Acknowledged' })).toBeVisible();
    await expect(callout).toHaveCount(0);
    const progress = await (await owner.c.get(`/api/v1/campaigns/${camp.id}`, { headers: owner.headers })).json();
    expect(progress.progress.acknowledged).toBe(1);
  });

  test('E2E-R1-UI-09 invite a colleague from Members; the invitation appears in the table', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Members');
    await expect(page.getByRole('heading', { level: 1, name: 'Members' })).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Invite people' }).click();
    const dialog = page.getByRole('dialog', { name: 'Invite people' });
    const email = `newhire-${Date.now()}@acme.test`;
    await dialog.getByLabel('Email address').fill(email);
    await dialog.getByRole('button', { name: /Reviewer/ }).click();
    await dialog.getByRole('button', { name: 'Send invitation' }).click();
    const row = page.getByRole('row', { name: new RegExp(email) });
    await expect(row.getByText('Invited')).toBeVisible();
    await expect(row.getByText('Reviewer')).toBeVisible();
  });

  test('E2E-R1-UI-10 read and acknowledge an announcement in the feed', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    await seedUser(request, 'feed-reader');
    const a = await (await owner.c.post('/api/v1/announcements', { headers: owner.headers, data: { title: 'Fire drill on Tuesday', content: doc('Leave by the nearest exit when the alarm sounds.'), audience: { roles: ['workspace.owner'] }, require_ack: true, priority: 'critical', pinned: true } })).json();
    expect((await owner.c.post(`/api/v1/announcements/${a.id}/publish`, { headers: owner.headers })).status()).toBe(200);
    await request.post('/__test/drain');

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Announcements');
    const post = page.getByRole('listitem').filter({ hasText: 'Fire drill on Tuesday' });
    await expect(post.getByText('Needs acknowledgement')).toBeVisible();
    await post.getByRole('button', { name: /Fire drill on Tuesday/ }).click();
    await expect(post.getByText('Leave by the nearest exit')).toBeVisible();
    await axe(page);
    await post.getByRole('button', { name: 'I’ve read this' }).click();
    await expect(post.getByText('Acknowledged')).toBeVisible();
  });

  test('E2E-R1-UI-11 an admin without two-step verification is guided through setup and then gets in', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const email = await seedUser(request, 'new-admin');
    await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['workspace.admin'] } });
    const token = await inviteToken(request, ws.id, email);
    const pw = await passwordSession(email);
    await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });

    await page.goto(`${WEB}/sign-in`);
    await page.getByLabel('Work email').fill(email);
    await page.getByLabel('Password').fill(PW);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Turn on two-step verification' })).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Get started' }).click();
    await expect(page.getByRole('img', { name: 'QR code for your authenticator app' })).toBeVisible();
    const secret = (await page.locator('.mfa-secret code').innerText()).replace(/\s+/g, '');
    await page.getByLabel('6-digit code').fill(totp(secret, await serverNow(request)));
    await page.getByRole('button', { name: 'Verify and turn on' }).click();
    const codes = page.getByRole('list', { name: 'Recovery codes' }).getByRole('listitem');
    await expect(codes).toHaveCount(10);
    const recovery = await codes.first().innerText();
    await axe(page);
    // The confirmed code counts as the second step: straight in, no second sign-in.
    await page.getByRole('button', { name: 'I’ve saved them, continue' }).click();
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await page.reload(); // the refreshed session keeps two-step
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    // The next sign-in asks for the second step.
    await page.getByRole('button', { name: /^Account:/ }).click();
    await page.getByRole('menuitem', { name: 'Sign out' }).click();
    await page.getByLabel('Work email').fill(email);
    await page.getByLabel('Password').fill(PW);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await page.getByLabel('Code').fill(recovery);
    await page.getByRole('button', { name: 'Verify' }).click();
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await expect(page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Admin', exact: true })).toBeVisible();
  });
});
