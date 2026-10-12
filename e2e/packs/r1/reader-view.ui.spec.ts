import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { WEB, axe, signIn } from '../../fixtures/ui';
import { enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-73 — the reading view for plain readers (BPMS-040, docs/05 §3.21): a member sees the title,
// type, version and published date, owner, the acknowledgement callout and the text; governance rows and
// actions are gone, with Fingerprint and the review date under "Details". The owner keeps everything.

const doc = (...paras: string[]) => ({ blocks: paras.map((text) => ({ type: 'paragraph', text })) });

test.describe('reader view @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-73 a plain reader sees a clean document page; the owner still sees the governance rows', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key: 'conduct', title: 'Code of Conduct', content: doc('Be kind to each other.') } });
    const { artifact, version } = await r.json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: owner.headers });
    expect((await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: owner.headers, data: {} })).status()).toBe(200);
    expect((await owner.c.put(`/api/v1/artifacts/${artifact.id}/review-cadence`, { headers: owner.headers, data: { days: 365 } })).ok()).toBe(true);

    const email = await seedUser(request, 'reader');
    expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).status()).toBe(201);
    const token = await inviteToken(request, ws.id, email);
    const pw = await passwordSession(email);
    await pw.c.post('/api/v1/invitations/accept', { headers: { authorization: `Bearer ${pw.token}` }, data: { token } });
    const reader = await enter(email, ws.id);
    const camp = await (await owner.c.post('/api/v1/campaigns', { headers: owner.headers, data: { version_id: version.id, title: 'Annual attestation', audience: { all: true } } })).json();
    expect((await owner.c.post(`/api/v1/campaigns/${camp.id}/activate`, { headers: owner.headers })).status()).toBe(200);

    await signIn(page, email, reader.recovery[0]!);
    await expect(page.getByRole('heading', { name: 'Needs you' })).toBeVisible();
    await page.goto(`${WEB}/library/${artifact.id}`);
    await expect(page.getByRole('heading', { level: 1, name: 'Code of Conduct' })).toBeVisible();
    await expect(page.getByText('Version', { exact: true })).toBeVisible();
    const facts = page.getByRole('definition');
    await expect(facts.filter({ hasText: /^v1.*published/ })).toBeVisible();
    await expect(facts.filter({ hasText: 'Olivia' })).toBeVisible();
    await expect(page.getByRole('region', { name: 'Acknowledgement requested' })).toBeVisible();
    await expect(page.getByRole('article', { name: 'Document text' })).toContainText('Be kind to each other.');
    for (const label of ['Status', 'Collection', 'Review', 'Verified', 'Accountable']) {
      await expect(page.getByText(label, { exact: true })).toHaveCount(0);
    }
    await expect(page.getByText('Fingerprint')).toBeHidden();
    await expect(page.getByRole('button', { name: 'Change owner' })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Suggest a change' })).toBeVisible(); // readers can still suggest
    await page.getByText('Details', { exact: true }).click();
    await expect(page.getByText('Fingerprint')).toBeVisible();
    await expect(page.getByText('Next review')).toBeVisible();
    await axe(page);

    // The owner keeps the full page.
    const op = await (await browser.newContext()).newPage();
    await signIn(op, ownerEmail, owner.recovery![0]!);
    await expect(op.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await op.goto(`${WEB}/library/${artifact.id}`);
    await expect(op.getByText('Collection', { exact: true })).toBeVisible();
    await expect(op.getByText('Verified', { exact: true })).toBeVisible();
    await expect(op.getByText('Fingerprint')).toBeVisible();
    await expect(op.getByRole('button', { name: 'Change owner' })).toBeVisible();
  });
});
