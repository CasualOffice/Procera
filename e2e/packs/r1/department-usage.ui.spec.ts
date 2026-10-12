import { expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { axe, nav, signIn } from '../../fixtures/ui';
import { inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-UI-75 — a department's people and what uses it (BPMS-023; docs/08 "Directory"): the selected
// department lists its members as links and, before Delete, a "Used by" section with the role grants
// scoped to it and the campaigns whose audience names it.

test.describe('department usage @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-75 a department shows its people and what uses it before it can be deleted', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const email = uniqueEmail('nina');
    expect((await owner.c.post('/api/v1/members/invitations', { headers: h, data: { email, roles: ['member'] } })).ok()).toBe(true);
    const token = await inviteToken(request, ws.id, email);
    expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: 'Nina', password: PW } })).status()).toBe(201);
    const nina = ((await (await owner.c.get('/api/v1/members', { headers: h })).json()).items as { id: string; email: string }[]).find((m) => m.email === email)!.id;

    const dept = await (await owner.c.post('/api/v1/departments', { headers: h, data: { name: 'Finance' } })).json();
    expect((await owner.c.patch(`/api/v1/members/${nina}`, { headers: h, data: { department_id: dept.id } })).ok()).toBe(true);
    const listed = (await (await owner.c.get('/api/v1/members', { headers: h })).json()).items.find((m: { id: string }) => m.id === nina);
    expect(listed.department_id).toBe(dept.id);
    const grant = await owner.c.post('/api/v1/role-grants', { headers: h, data: { membership_id: nina, role_key: 'reviewer', scope_kind: 'department', scope_id: dept.id, reason: 'finance reviews' } });
    expect(grant.ok(), await grant.text()).toBe(true);
    const { version } = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'expenses', title: 'Expenses', content: { blocks: [{ type: 'paragraph', text: 'Keep receipts.' }] } } })).json();
    await owner.c.post(`/api/v1/versions/${version.id}/submit`, { headers: h });
    expect((await owner.c.post(`/api/v1/versions/${version.id}/publish`, { headers: h, data: {} })).ok()).toBe(true);
    expect((await owner.c.post('/api/v1/campaigns', { headers: h, data: { version_id: version.id, title: 'Finance attestation', audience: { departments: [dept.id] } } })).ok()).toBe(true);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await nav(page, 'Departments');
    const finance = page.getByRole('region', { name: 'Finance' });
    await expect(finance.getByRole('list', { name: 'People in Finance' }).getByRole('link', { name: 'Nina' })).toBeVisible();
    const used = finance.getByRole('region', { name: 'Used by' });
    await expect(used.getByRole('list', { name: 'Role grants' })).toContainText('Reviewer · Nina');
    await expect(used.getByRole('list', { name: 'Campaigns' }).getByRole('link', { name: 'Finance attestation' })).toBeVisible();
    await axe(page);

    await finance.getByRole('button', { name: 'Delete department' }).click();
    await expect(page.getByRole('dialog', { name: 'Move things out of Finance first' })).toBeVisible();
  });
});
