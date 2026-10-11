import { expect, test } from '@playwright/test';
import { hasDatabase, passwordSession, seedUser } from '../../fixtures/accounts';
import { inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-KEY-01 — workspace KEKs (BPMS-030): versions, rotation with a reason and step-up, background
// rewrap that keeps data readable, and isolation.

test.describe('encryption @r1 @security', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-KEY-01 an email sealed under v1 stays readable through rotation and rewrap to v2; the old key is destroyed; members cannot see keys', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    // An invitation email is envelope-encrypted under the workspace's key.
    const email = await seedUser(request, 'keyinvitee');
    await owner.c.post('/api/v1/members/invitations', { headers: h, data: { email, roles: ['member'] } });
    const mailText = async () => {
      const items = (await (await request.get(`/__test/mail?workspace=${ws.id}&to=${encodeURIComponent(email)}`)).json()).items as { kind: string; text: string }[];
      return items.find((m) => m.kind === 'invitation')?.text ?? '';
    };
    await expect.poll(async () => { await request.post('/__test/drain'); return mailText(); }).toContain('/invite?token=');

    const before = await (await owner.c.get('/api/v1/security/encryption', { headers: h })).json();
    expect([before.algorithm, before.active_version, before.pending_rewrap]).toEqual(['AES-256-GCM', 1, 0]);
    expect(before.versions[0].values).toBeGreaterThanOrEqual(1);

    expect((await owner.c.post('/api/v1/security/encryption/rotate', { headers: h, data: { reason: ' ' } })).status()).toBe(422);
    const rotated = await owner.c.post('/api/v1/security/encryption/rotate', { headers: h, data: { reason: 'Annual rotation' } });
    expect(rotated.status()).toBe(200);
    expect((await rotated.json()).active_version).toBe(2);
    // Readable straight after rotation, whether or not the background rewrap has run yet.
    expect(await mailText()).toContain('/invite?token=');

    await expect.poll(async () => {
      await request.post('/__test/keys/rewrap');
      return (await (await owner.c.get('/api/v1/security/encryption', { headers: h })).json()).pending_rewrap;
    }).toBe(0);
    const after = await (await owner.c.get('/api/v1/security/encryption', { headers: h })).json();
    expect(after.versions.map((v: { version: number; state: string }) => [v.version, v.state])).toEqual([[2, 'active'], [1, 'destroyed']]);
    expect(await mailText()).toContain('/invite?token='); // readable under the new key

    const audit = await (await owner.c.get('/api/v1/audit/events?action=key.', { headers: h })).json();
    expect(audit.items.map((e: { action: string }) => e.action)).toContain('key.rotated');

    // A member cannot see or rotate keys.
    const token = await inviteToken(request, ws.id, email);
    const pw = await passwordSession(email);
    const bearer = { authorization: `Bearer ${pw.token}` };
    await pw.c.post('/api/v1/invitations/accept', { headers: bearer, data: { token } });
    const sw = await (await pw.c.post('/api/v1/session/workspace', { headers: bearer, data: { workspace_id: ws.id } })).json();
    const mh = { authorization: `Bearer ${sw.access_token}` };
    expect((await pw.c.get('/api/v1/security/encryption', { headers: mh })).status()).toBe(403);
    expect((await pw.c.post('/api/v1/security/encryption/rotate', { headers: mh, data: { reason: 'x' } })).status()).toBe(403);
  });
});
