import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-POL-04..05 — re-acknowledgement on publish (BPMS-045; FR-PL03, FR-G03): versions are numbered
// major.minor; publishing a version that requires it (governance.reack.policy) starts a campaign for the
// policy's applicable audience, due after governance.reack.window, bound to the exact version hash.

type Req = Parameters<typeof seedUser>[0];
const DAY = 86_400_000;

/** A plain member with a workspace session and their membership id. */
async function reader(request: Req, owner: Session, ws: string, tag: string) {
  const email = await seedUser(request, tag);
  await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } });
  const token = await inviteToken(request, ws, email);
  const pw = await passwordSession(email);
  const bearer = { authorization: `Bearer ${pw.token}` };
  const { membership_id } = await (await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } })).json();
  const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws } })).json();
  return { c: pw.c, headers: { authorization: `Bearer ${sw.access_token}` }, id: membership_id as string };
}

const doc = (text: string) => ({ blocks: [{ type: 'paragraph', text }] });

/** Starts a new version of `artifact` with `text` and gets it approved; returns its id. */
async function approvedVersion(owner: Session, artifact: string, text: string) {
  const v = await (await owner.c.post(`/api/v1/artifacts/${artifact}/versions`, { headers: owner.headers })).json();
  await owner.c.patch(`/api/v1/versions/${v.id}`, { headers: owner.headers, data: { content: doc(text) } });
  expect((await (await owner.c.post(`/api/v1/versions/${v.id}/submit`, { headers: owner.headers })).json()).lifecycle).toBe('approved');
  return v.id as string;
}

async function publish(owner: Session, id: string, data: Record<string, unknown>) {
  const r = await owner.c.post(`/api/v1/versions/${id}/publish`, { headers: owner.headers, data });
  expect(r.status(), await r.text()).toBe(200);
  return r.json();
}

async function reackCampaigns(owner: Session) {
  const all = (await (await owner.c.get('/api/v1/campaigns', { headers: owner.headers })).json()).items;
  return all.filter((c: { origin: string }) => c.origin === 'reack');
}

const near = (iso: string, at: number) => Math.abs(Date.parse(iso) - at) < 120_000;

test.describe('re-acknowledgement on publish @r1 @library', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-POL-04 a major version asks the applicable audience to re-acknowledge that exact version; a minor one does not (major_only)', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const inside = await reader(request, owner, ws.id, 'warden');
    const outside = await reader(request, owner, ws.id, 'visitor');
    const g = await (await owner.c.post('/api/v1/groups', { headers: h, data: { name: 'Fire wardens' } })).json();
    await owner.c.post(`/api/v1/groups/${g.id}/members`, { headers: h, data: { membership_id: inside.id } });

    const { artifact, version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'safety.fire', title: 'Fire safety', content: doc('Know your exits.') } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
    const v1 = await publish(owner, version.id, { change: 'minor' });
    expect([v1.semver, v1.reack.required, v1.reack.campaign_id]).toEqual(['1.0', false, null]);
    await owner.c.put(`/api/v1/artifacts/${artifact.id}/applicability`, { headers: h, data: { category: 'Safety', applicability: { groups: [g.id] } } });

    // The dialog preview: numbers for each choice, who is asked and by when.
    const v2id = await approvedVersion(owner, artifact.id, 'Know your exits. Drill monthly.');
    expect((await inside.c.get(`/api/v1/versions/${v2id}/publish-preview`, { headers: inside.headers })).status()).toBe(403);
    const now = await serverNow(owner.c);
    const pre = await (await owner.c.get(`/api/v1/versions/${v2id}/publish-preview`, { headers: h })).json();
    expect(pre).toMatchObject({ version_no: 2, next_semver_major: '2.0', next_semver_minor: '1.1', reack_policy: 'major_only', reack_required_if_major: true, reack_required_if_minor: false, audience_size: 1, audience: { groups: [g.id] } });
    expect(near(pre.due_at, now + 14 * DAY)).toBe(true);
    expect((await owner.c.post(`/api/v1/versions/${v2id}/publish`, { headers: h, data: { change: 'huge', change_reason: 'x' } })).status()).toBe(422);

    // Publishing v2 as a major starts the campaign in the same call.
    const v2 = await publish(owner, v2id, { change: 'major', change_reason: 'Monthly drills' });
    expect([v2.lifecycle, v2.semver, v2.reack_required, v2.reack.required, v2.reack.audience_size]).toEqual(['published', '2.0', true, true, 1]);
    expect(near(v2.reack.due_at, now + 14 * DAY)).toBe(true);
    const cmp = await (await owner.c.get(`/api/v1/campaigns/${v2.reack.campaign_id}`, { headers: h })).json();
    expect(cmp).toMatchObject({ origin: 'reack', state: 'active', title: 'Re-acknowledge Fire safety v2.0', audience: { groups: [g.id] }, subject: { version_id: v2id, version_no: 2, semver: '2.0', content_hash: v2.content_hash }, progress: { assigned: 1 } });

    // Only the applicable member is asked; the evidence carries the version hash.
    expect((await (await outside.c.get('/api/v1/me/acknowledgements?state=pending', { headers: outside.headers })).json()).items).toEqual([]);
    const mine = (await (await inside.c.get('/api/v1/me/acknowledgements?state=pending', { headers: inside.headers })).json()).items;
    expect(mine.map((a: { subject: { version_id: string } }) => a.subject.version_id)).toEqual([v2id]);
    const done = await (await inside.c.post(`/api/v1/acknowledgements/${mine[0].id}/complete`, { headers: inside.headers, data: { content_hash: v2.content_hash } })).json();
    expect([done.acknowledged_version_id, done.acknowledged_content_hash]).toEqual([v2id, v2.content_hash]);

    // A minor change under major_only: numbered 2.1, nobody is asked again.
    const v3id = await approvedVersion(owner, artifact.id, 'Know your exits. Drill monthly. Typo fixed.');
    const v3 = await publish(owner, v3id, { change: 'minor', change_reason: 'Typo' });
    expect([v3.semver, v3.reack_required, v3.reack.required]).toEqual(['2.1', false, false]);
    // The published-event consumer finds the campaign already started and adds none.
    await request.post('/__test/drain');
    expect((await reackCampaigns(owner)).map((c: { subject: { version_id: string } }) => c.subject.version_id)).toEqual([v2id]);
  });

  test('E2E-R1-POL-05 every_publish asks on minors, none never asks, and a scheduled major asks when it goes live', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const setPolicy = async (value: string) => expect((await owner.c.put('/api/v1/settings/governance.reack.policy', { headers: h, data: { value } })).ok()).toBe(true);
    const { artifact, version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'ops.keys', title: 'Key handover', content: doc('Sign the log.') } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
    await publish(owner, version.id, {});

    await setPolicy('every_publish');
    const v2 = await publish(owner, await approvedVersion(owner, artifact.id, 'Sign the log twice.'), { change: 'minor', change_reason: 'Twice' });
    expect([v2.semver, v2.reack.required, v2.reack.audience_size]).toEqual(['1.1', true, 1]); // everyone: just the owner
    expect(v2.reack.campaign_id).toBeTruthy();

    await setPolicy('none');
    const v3 = await publish(owner, await approvedVersion(owner, artifact.id, 'Use the key box.'), { change_reason: 'Key box' });
    expect([v3.semver, v3.reack.required, v3.reack.campaign_id]).toEqual(['2.0', false, null]);

    await setPolicy('major_only');
    const v4id = await approvedVersion(owner, artifact.id, 'Use the smart lock.');
    const at = (await serverNow(owner.c)) + 30_000;
    const v4 = await publish(owner, v4id, { change: 'major', change_reason: 'Smart lock', effective_at: new Date(at).toISOString() });
    expect([v4.lifecycle, v4.semver, v4.reack.required, v4.reack.campaign_id]).toEqual(['scheduled', '3.0', true, null]);
    expect(near(v4.reack.due_at, at + 14 * DAY)).toBe(true);
    expect((await reackCampaigns(owner)).map((c: { subject: { version_id: string } }) => c.subject.version_id)).toEqual([v2.id]);

    await request.post('/__test/clock', { data: { advance: 'PT1M' } });
    // Drain is global (other tests publish too), so poll until the consumer has run for this version.
    await expect.poll(async () => {
      await request.post('/__test/drain');
      return (await reackCampaigns(owner)).filter((c: { subject: { version_id: string } }) => c.subject.version_id === v4id).length;
    }, { timeout: 15_000 }).toBe(1);
    const live = (await reackCampaigns(owner)).find((c: { subject: { version_id: string } }) => c.subject.version_id === v4id);
    expect(live).toMatchObject({ state: 'active', title: 'Re-acknowledge Key handover v3.0', audience: { all: true }, subject: { semver: '3.0' } });
    expect((await (await owner.c.get(`/api/v1/versions/${v4id}`, { headers: h })).json()).lifecycle).toBe('published');
  });
});
