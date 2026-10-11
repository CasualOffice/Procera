import { type APIRequestContext, type Browser, type Page, expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { WEB, axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-18..22 — library governance and the policy portal through the web UI (BPMS-043/044/045):
// verify and recertify, suggest a change and resolve it, compare versions word by word, the
// Policies list with categories and "what changed", and restricted policies hidden from outsiders.

const doc = (...paras: string[]) => ({ blocks: paras.map((text) => ({ type: 'paragraph', text })) });

/** Creates and publishes a policy (v1); returns its ids. */
async function publishPolicy(owner: Session, key: string, title: string, content = doc('Be kind to each other.')) {
  const r = await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key, title, content } });
  expect(r.status()).toBe(201);
  const { artifact, version } = await r.json();
  await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: owner.headers });
  expect((await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: owner.headers, data: {} })).status()).toBe(200);
  return { id: artifact.id as string, v1: version.id as string };
}

/** Publishes a new version of `id` with `content` and a change reason. */
async function publishNewVersion(owner: Session, id: string, content: unknown, reason: string) {
  const nv = await (await owner.c.post(`/api/v1/artifacts/${id}/versions`, { headers: owner.headers })).json();
  const vid = nv.version?.id ?? nv.id;
  const cur = await (await owner.c.get(`/api/v1/versions/${vid}`, { headers: owner.headers })).json();
  expect((await owner.c.patch(`/api/v1/versions/${vid}`, { headers: { ...owner.headers, 'if-match': `"${cur.revision}"` }, data: { content } })).ok()).toBeTruthy();
  await owner.c.post(`/api/v1/versions/${vid}/submit`, { headers: owner.headers });
  expect((await owner.c.post(`/api/v1/versions/${vid}/publish`, { headers: owner.headers, data: { change_reason: reason } })).status()).toBe(200);
  return vid as string;
}

/** Invites a person with `role` and returns their email and recovery codes for a browser sign-in. */
async function addMember(request: APIRequestContext, ws: { id: string }, owner: Session, tag: string, role = 'member') {
  const email = await seedUser(request, tag);
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: [role] } })).status()).toBe(201);
  const token = await inviteToken(request, ws.id, email);
  const pw = await passwordSession(email);
  await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
  const s = await enter(email, ws.id);
  return { email, recovery: s.recovery };
}

async function secondPage(browser: Browser): Promise<Page> {
  return (await browser.newContext()).newPage();
}

test.describe('library governance @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // provisions a workspace and signs in through the browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-18 an owner sets a review schedule, recertifies when it falls due, and verifies a policy', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const { id } = await publishPolicy(owner, 'conduct', 'Code of Conduct');

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { name: 'Needs you' })).toBeVisible();
    await page.goto(`${WEB}/library/${id}`);
    await expect(page.getByRole('heading', { level: 1, name: 'Code of Conduct' })).toBeVisible();

    await page.getByRole('button', { name: 'Change schedule' }).click();
    const sched = page.getByRole('dialog', { name: 'Review schedule' });
    await sched.getByLabel('Review every (days)').fill('20');
    await sched.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('Review due soon')).toBeVisible();
    const callout = page.getByRole('region', { name: 'Review due' });
    await expect(callout).toBeVisible();
    await axe(page);
    await callout.getByRole('button', { name: 'Recertify now' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Recertified.' })).toBeVisible();

    await page.getByRole('button', { name: 'Verify', exact: true }).click();
    await expect(page.getByText(/Verified until/)).toBeVisible();
    await page.getByRole('button', { name: 'Remove verification' }).click();
    await page.getByRole('dialog', { name: 'Remove verification?' }).getByRole('button', { name: 'Remove' }).click();
    await expect(page.getByText('Not verified')).toBeVisible();
  });

  test('E2E-R1-UI-19 a member suggests a change; the owner accepts it and marks it implemented in the new version', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const { id } = await publishPolicy(owner, 'travel', 'Travel and expenses', doc('Book economy for flights under six hours.'));
    const member = await addMember(request, ws, owner, 'suggester');

    await signIn(page, member.email, member.recovery[0]!);
    await expect(page.getByRole('heading', { name: 'Needs you' })).toBeVisible();
    await page.goto(`${WEB}/library/${id}`);
    await page.getByRole('button', { name: 'Suggest a change' }).click();
    const dialog = page.getByRole('dialog', { name: 'Suggest a change' });
    await dialog.getByLabel('What should change?').fill('Allow premium economy over six hours');
    await dialog.getByLabel('Details').fill('Long flights are tiring; most peers allow it.');
    await axe(page);
    await dialog.getByRole('button', { name: 'Send suggestion' }).click();
    const item = page.getByRole('listitem').filter({ hasText: 'Allow premium economy over six hours' });
    await expect(item.getByText('Open')).toBeVisible();
    await expect(item.getByRole('button', { name: 'Accept' })).toHaveCount(0); // members can't resolve

    const v2 = await publishNewVersion(owner, id, doc('Book economy for flights under six hours. Premium economy is allowed for longer flights.'), 'Premium economy for long flights');
    expect(v2).toBeTruthy();
    const op = await secondPage(browser);
    await signIn(op, ownerEmail, owner.recovery![0]!);
    await expect(op.getByRole('heading', { name: 'Needs you' })).toBeVisible();
    await op.goto(`${WEB}/library/${id}`);
    const ownerItem = op.getByRole('listitem').filter({ hasText: 'Allow premium economy over six hours' });
    await ownerItem.getByRole('button', { name: 'Accept' }).click();
    await expect(ownerItem.getByText('Accepted')).toBeVisible();
    await ownerItem.getByRole('button', { name: 'Mark implemented' }).click();
    const impl = op.getByRole('dialog', { name: 'Mark as implemented' });
    await impl.getByLabel('Version').selectOption({ label: 'v2 (published)' });
    await axe(op);
    await impl.getByRole('button', { name: 'Mark implemented' }).click();
    await expect(ownerItem.getByText('Implemented')).toBeVisible();
    await expect(ownerItem.getByText('in v2')).toBeVisible();
  });

  test('E2E-R1-UI-20 compare v1 and v2 from the version history shows the word-level changes', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const { id } = await publishPolicy(owner, 'remote', 'Remote work', doc('Everyone may work remotely two days a week.', 'Core hours are 10 to 15.'));
    await publishNewVersion(owner, id, doc('Everyone may work remotely three days a week.', 'Core hours are 10 to 15.', 'Tell your manager which days.'), 'Three remote days');

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { name: 'Needs you' })).toBeVisible();
    await page.goto(`${WEB}/library/${id}`);
    await page.getByRole('link', { name: 'Compare with v1' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'What changed' })).toBeVisible();
    await expect(page.getByRole('status').filter({ hasText: '1 section changed, 1 added.' })).toBeVisible();
    await expect(page.locator('del', { hasText: 'two' })).toBeVisible();
    await expect(page.locator('ins', { hasText: 'three' })).toBeVisible();
    await expect(page.getByText('Tell your manager which days.')).toBeVisible();
    await expect(page.getByText('Three remote days')).toBeVisible();
    await axe(page);
  });

  test('E2E-R1-UI-21 a member opens Policies, filters by category and follows "what changed" to the comparison', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const hr = await publishPolicy(owner, 'leave', 'Annual leave', doc('Everyone gets 25 days of leave.'));
    const it = await publishPolicy(owner, 'devices', 'Device security', doc('Lock your screen when you step away.'));
    await owner.c.put(`/api/v1/artifacts/${hr.id}/applicability`, { headers: owner.headers, data: { category: 'HR', applicability: null, restricted: false } });
    await owner.c.put(`/api/v1/artifacts/${it.id}/applicability`, { headers: owner.headers, data: { category: 'IT', applicability: null, restricted: false } });
    await publishNewVersion(owner, it.id, doc('Lock your screen when you step away. Use a password manager.'), 'Password managers');
    const member = await addMember(request, ws, owner, 'reader');

    await signIn(page, member.email, member.recovery[0]!);
    await nav(page, 'Policies');
    await expect(page.getByRole('heading', { level: 1, name: 'Policies' })).toBeVisible();
    await expect(page.getByRole('link', { name: 'Annual leave' })).toBeVisible();
    await axe(page);
    await page.getByRole('group', { name: 'Category' }).getByRole('button', { name: 'IT' }).click();
    await expect(page.getByRole('link', { name: 'Annual leave' })).toHaveCount(0);
    await expect(page.getByRole('link', { name: 'Device security' })).toBeVisible();
    await page.getByRole('link', { name: 'What changed in v2' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'What changed' })).toBeVisible();
    await expect(page.locator('ins', { hasText: 'password' })).toBeVisible();
  });

  test('E2E-R1-UI-22 a publisher restricts a policy to a role; a member outside it can’t find or open it', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const { id } = await publishPolicy(owner, 'board', 'Board conduct', doc('Board papers are confidential.'));
    const member = await addMember(request, ws, owner, 'outsider');

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { name: 'Needs you' })).toBeVisible();
    await page.goto(`${WEB}/library/${id}`);
    await page.getByRole('button', { name: 'Change', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Who this policy applies to' });
    await dialog.getByLabel('Category').fill('Governance');
    await dialog.getByRole('button', { name: 'Specific people' }).click();
    await dialog.getByRole('group', { name: 'Roles' }).getByRole('button', { name: 'Workspace Owner' }).click();
    await dialog.getByText('Only these people can see it').click();
    await axe(page);
    await dialog.getByRole('button', { name: 'Save' }).click();
    await expect(page.getByText('Only they can see it')).toBeVisible();

    const mp = await secondPage(browser);
    await signIn(mp, member.email, member.recovery[0]!);
    await nav(mp, 'Policies');
    await expect(mp.getByRole('heading', { level: 1, name: 'Policies' })).toBeVisible();
    await expect(mp.getByRole('link', { name: 'Board conduct' })).toHaveCount(0);
    await mp.goto(`${WEB}/library/${id}`);
    await expect(mp.getByRole('heading', { level: 1, name: 'Document not found' })).toBeVisible();
  });
});
