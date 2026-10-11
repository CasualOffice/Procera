import { type APIRequestContext, type Locator, type Page, expect, test } from '@playwright/test';
import { PW, hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { WEB, axe, nav, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';
import { totp } from '../../fixtures/totp';
import { inviteFromMembers, joinAsNewPerson } from '../../fixtures/journey';
import { PNG_1PX } from '../../fixtures/files';
import { uniqueEmail } from '../../fixtures/accounts';

// E2E-R1 release gate (docs/23 §R1, BPMS-032): the scenarios whose features have shipped, run in
// the browser. R1-16 deactivation cuts access at once; R1-19 keyboard-only publish and acknowledge.

/** A member (Member role) who joined `ws` through an invitation; signs in with a password only. */
async function member(request: APIRequestContext, owner: Session, ws: string, tag: string, role = 'member'): Promise<{ email: string; id: string }> {
  const email = await seedUser(request, tag);
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: [role] } })).status()).toBe(201);
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

  test('E2E-R1-04 directory CSV import: check shows each row, a manager cycle and an unknown person are refused with clear reasons, the rest applies', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const ana = await member(request, owner, ws.id, 'ana');
    const raj = await member(request, owner, ws.id, 'raj');
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await nav(page, 'Members');
    await page.getByRole('button', { name: 'Import directory' }).click();
    const dlg = page.getByRole('dialog', { name: 'Import directory' });
    const csv = [
      'email,department,title,manager_email,location',
      `${ana.email},Finance,Accountant,${ownerEmail},Bangalore`,
      `${raj.email},Finance,Analyst,${ana.email},Bangalore`,
      `${ownerEmail},,,${raj.email},`, // owner → raj → ana → owner: a reporting cycle
      'nobody@acme.test,Finance,,,',
    ].join('\n');
    await dlg.locator('#dir-csv').setInputFiles({ name: 'directory.csv', mimeType: 'text/csv', buffer: Buffer.from(csv) });
    await dlg.getByRole('button', { name: 'Check file' }).click();
    const rows = dlg.getByRole('table', { name: 'Rows' });
    await expect(rows.getByRole('row', { name: new RegExp(ana.email) }).getByText('Update')).toBeVisible();
    await expect(rows.getByRole('row', { name: new RegExp(ownerEmail) })).toContainText('cycle');
    await expect(rows.getByRole('row', { name: /nobody@acme\.test/ }).getByText('Error')).toBeVisible();
    await expect(dlg.getByText('Rows with errors are skipped')).toBeVisible();
    await axe(page);
    // Nothing changed yet.
    const before = await (await owner.c.get(`/api/v1/members/${ana.id}/directory`, { headers: owner.headers })).json();
    expect(before.title).toBeNull();
    await dlg.getByRole('button', { name: 'Apply 2 rows' }).click();
    await expect(page.getByText('Directory updated.')).toBeVisible();

    await page.goto(`${WEB}/members/${raj.id}`);
    await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
    const props = page.locator('dl.props');
    await expect(props).toContainText('Finance');
    await expect(props).toContainText('Bangalore');
    await props.getByRole('link', { name: 'ana', exact: true }).click(); // their manager
    await expect(page.getByRole('heading', { level: 1, name: 'ana' })).toBeVisible();
    await expect(page.locator('dl.props')).toContainText('Olivia');
  });

  test('E2E-R1-17 a suspended (read-only) workspace keeps reading, refuses changes with a banner explaining why, and works again once restored', async ({ page, request }) => {
    const { sys, ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: owner.headers, data: { kind: 'policy', key: 'travel', title: 'Travel', content: { blocks: [{ type: 'paragraph', text: 'Book early.' }] } } })).json();
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();

    const ro = await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/suspend`, { headers: sys.headers, data: { mode: 'read_only', reason: 'billing review' } });
    expect(ro.status(), await ro.text()).toBe(200);

    // Reading still works; the first change is refused and the banner explains it.
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await expect(page.getByText('Book early.')).toBeVisible();
    await page.getByRole('button', { name: 'Submit for review' }).click();
    await expect(page.getByText('This workspace is read-only', { exact: false }).first()).toBeVisible();
    const banner = page.getByRole('status').filter({ hasText: 'This workspace is read-only.' });
    await expect(banner).toBeVisible();
    await axe(page);
    // The banner stays on every page while suspended.
    await nav(page, 'Library');
    await expect(banner).toBeVisible();

    const back = await sys.c.post(`/sysapi/v1/workspaces/${ws.id}/restore`, { headers: { ...sys.headers, 'if-match': ro.headers()['etag']! } });
    expect(back.status()).toBe(200);
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
    await expect(banner).toHaveCount(0);
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await page.getByRole('button', { name: 'Submit for review' }).click();
    await expect(page.getByText('Approved', { exact: true })).toBeVisible();
  });

  test('E2E-R1-14 audit trail: ten actions by two people, filtered by person in the Audit log, a version timeline in order, the chain verified', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    // Ten actions: a policy written over three edits, submitted and published; a collection; an
    // invitation; an announcement draft; a member who joins.
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'expenses', title: 'Expenses', content: { blocks: [{ type: 'paragraph', text: 'v0' }] } } })).json();
    let rev = r.version.revision;
    for (const t of ['One', 'Two', 'Three']) {
      const e = await owner.c.patch(`/api/v1/versions/${r.version.id}`, { headers: { ...h, 'if-match': `"${rev}"` }, data: { content: { blocks: [{ type: 'paragraph', text: t }] } } });
      expect(e.status()).toBe(200);
      rev = (await e.json()).revision;
    }
    await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} });
    await owner.c.post('/api/v1/collections', { headers: h, data: { name: 'Finance' } });
    await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Quarter close', content: { blocks: [{ type: 'paragraph', text: 'Soon.' }] }, audience: { all: true } } });
    const joiner = await member(request, owner, ws.id, 'auditee');

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await nav(page, 'Audit log');
    await expect(page.getByText('Chain verified')).toBeVisible();
    const table = page.getByRole('table', { name: 'Audit events' });
    // Only the joiner's own actions when filtered to them.
    await page.getByLabel('Person').selectOption({ label: 'auditee' });
    await expect(table.getByRole('row').nth(1)).toBeVisible();
    for (const row of await table.locator('tbody tr').allTextContents()) expect(row).toContain('auditee');
    // Olivia's actions include the publication, newest first.
    await page.getByLabel('Person').selectOption({ label: 'Olivia' });
    await expect(table.getByText('Version published').first()).toBeVisible();
    await axe(page);
    // The version's own history, oldest first: three edits, then submitted, then published.
    await table.getByRole('row').filter({ hasText: 'Version submitted' }).first().getByRole('button', { name: /artifact version/ }).click();
    const tl = page.getByRole('dialog', { name: /history/ });
    await expect(tl.locator('.tl-title').filter({ hasText: 'Version published' })).toBeVisible();
    const titles = await tl.locator('.tl-title').allInnerTexts();
    const at = (t: string) => titles.findIndex((x) => x.startsWith(t));
    expect(titles.filter((t) => t.startsWith('Version edited'))).toHaveLength(3);
    expect(at('Version edited')).toBeLessThan(at('Version submitted'));
    expect(at('Version submitted')).toBeLessThan(at('Version published'));
    const chain = await (await request.get(`/__test/audit/verify?workspace=${ws.id}`)).json();
    expect(chain.ok).toBe(true);
    expect(joiner.id).toBeTruthy();
  });

  test('E2E-R1-08 a new joiner automatically receives the open acknowledgement campaigns that apply to them', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'conduct', title: 'Code of conduct', content: { blocks: [{ type: 'paragraph', text: 'Be kind.' }] } } })).json();
    await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} });
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();

    // The campaign asks people who join later too (the default), and says so before launch.
    await nav(page, 'Campaigns');
    await page.getByRole('button', { name: 'New campaign' }).first().click();
    const dlg = page.getByRole('dialog', { name: 'New campaign' });
    await dlg.getByRole('button', { name: /Code of conduct/ }).click();
    await expect(dlg.getByRole('checkbox', { name: /Also ask people who join later/ })).toBeChecked();
    await dlg.getByRole('button', { name: 'Create draft' }).click();
    await expect(page.getByText('People who join the audience later are asked too')).toBeVisible();
    await page.getByRole('button', { name: 'Launch' }).click();
    await expect(page.getByRole('table', { name: 'People in this campaign' })).toBeVisible();
    const campaignUrl = page.url();

    // A new hire joins after launch and finds the request waiting on Home.
    const email = uniqueEmail('joiner');
    await inviteFromMembers(page, email);
    const hire = await joinAsNewPerson(browser, request, ws.id, email, 'Jo Joiner');
    await request.post('/__test/drain');
    await hire.reload();
    await expect(hire.getByText('Acknowledge “Code of conduct”')).toBeVisible();
    await hire.getByRole('button', { name: 'Read and acknowledge' }).click();
    await hire.getByRole('region', { name: 'Acknowledgement requested' }).getByRole('button', { name: 'I acknowledge' }).click();
    await expect(hire.getByRole('status').filter({ hasText: 'Acknowledged' })).toBeVisible();

    await page.goto(campaignUrl);
    await expect(page.getByRole('row', { name: /Jo Joiner/ }).getByText('Done')).toBeVisible();
  });

  test('E2E-R1-09 J14: an announcement to Bangalore with a confirm-by date reaches only people there, runs on the campaign engine, escalates once to the manager of whoever did not confirm, and closes on archive', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const ana = await member(request, owner, ws.id, 'ana');
    const kim = await member(request, owner, ws.id, 'kim');
    const raj = await member(request, owner, ws.id, 'raj');
    const csv = `email,department,location,manager_email\n${ana.email},Finance,Bangalore,${ownerEmail}\n${kim.email},Finance,Bangalore,${ownerEmail}\n${raj.email},Operations,Pune,`;
    const imp = await owner.c.post('/api/v1/members/import?mode=apply', { headers: { ...owner.headers, 'content-type': 'text/csv' }, data: csv });
    expect(imp.status(), await imp.text()).toBe(200);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await nav(page, 'Manage announcements');
    await page.getByRole('button', { name: 'New announcement' }).first().click();
    const dlg = page.getByRole('dialog', { name: 'New announcement' });
    await dlg.getByLabel('Title').fill('Bangalore office move');
    await dlg.getByLabel('Message').fill('We move to the new office on Monday.');
    await dlg.getByRole('button', { name: 'Specific people' }).click();
    const places = dlg.getByRole('group', { name: 'Locations' });
    await expect(places.getByRole('button', { name: /Pune/ })).toBeVisible();
    await places.getByRole('button', { name: /Bangalore/ }).click();
    await expect(dlg.getByText('Sends to:')).toContainText('Bangalore');
    await dlg.getByText('Ask people to confirm they’ve read it').click();
    const due = new Date((await serverNow(request)) + 3 * 86_400_000).toISOString().slice(0, 10); // the test server keeps its own clock
    await dlg.getByLabel(/^Confirm by/).fill(due);
    await axe(page);
    await dlg.getByRole('button', { name: 'Save draft' }).click();
    await page.getByRole('button', { name: 'Publish now' }).click(); // a targeted post needs no approval
    await expect(page.getByText(/\(0 of 2\)/).first()).toBeVisible();
    await expect(page.getByText('Confirm by')).toBeVisible();
    await request.post('/__test/drain');

    // Ana confirms; Raj (Pune) never sees it.
    const anaPage = await (await browser.newContext()).newPage();
    await signInPassword(anaPage, ana.email);
    await nav(anaPage, 'Announcements');
    const post = anaPage.getByRole('listitem').filter({ hasText: 'Bangalore office move' });
    await post.getByRole('button', { name: /Bangalore office move/ }).click();
    await post.getByRole('button', { name: 'I’ve read this' }).click();
    await expect(post.getByText('Acknowledged')).toBeVisible();
    const rajPage = await (await browser.newContext()).newPage();
    await signInPassword(rajPage, raj.email);
    await nav(rajPage, 'Announcements');
    await expect(rajPage.getByText('Bangalore office move')).toHaveCount(0);

    // The campaign behind it counts Ana's confirmation.
    await page.getByRole('link', { name: 'Reminders and evidence' }).click();
    await expect(page.getByText('1 of 2')).toBeVisible();
    await expect(page.getByRole('row', { name: /ana/ }).getByText('Done')).toBeVisible();
    const campaignUrl = page.url();
    const campaignId = campaignUrl.split('/').pop()!;

    // Past the deadline, Kim's manager is told once.
    expect((await request.post('/__test/campaigns/backdate', { data: { workspace: ws.id, campaign: campaignId, seconds: 15 * 86_400 } })).status()).toBe(204);
    const kinds = async () => (await (await owner.c.get('/api/v1/notifications', { headers: owner.headers })).json()).items.map((n: { kind: string }) => n.kind) as string[];
    await expect.poll(async () => { await request.post('/__test/drain'); return (await kinds()).filter((k) => k === 'announcement_ack_escalated').length; }).toBe(1);
    await request.post('/__test/drain');
    expect((await kinds()).filter((k) => k === 'announcement_ack_escalated')).toHaveLength(1);
    await nav(page, 'Notifications');
    await expect(page.getByText(/kim has not acknowledged Bangalore office move/)).toBeVisible();

    // Archiving the announcement closes its campaign.
    await nav(page, 'Manage announcements');
    await page.getByRole('table', { name: 'Announcements' }).getByRole('button', { name: 'Bangalore office move' }).click();
    await page.getByRole('button', { name: 'Archive' }).click();
    const archive = page.getByRole('dialog', { name: 'Archive this announcement?' });
    await archive.getByLabel(/^Reason/).fill('Move done');
    await archive.getByRole('button', { name: 'Archive', exact: true }).click();
    await expect(archive).toHaveCount(0);
    await page.goto(campaignUrl);
    await expect(page.getByText('the announcement was archived')).toBeVisible();
  });

  test('E2E-R1-03 branding: a too-light accent is adjusted for text with a live explanation, and every member sees the workspace colour', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const m = await member(request, owner, ws.id, 'viewer');
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await nav(page, 'Settings');
    const row = page.getByRole('listitem').filter({ has: page.getByText('Accent colour', { exact: true }) });
    await row.scrollIntoViewIfNeeded();
    await row.getByRole('button', { name: 'Change Accent colour' }).click();
    await row.getByLabel('Hex colour').fill('#FFE14D');
    const note = row.getByRole('status');
    await expect(note).toContainText('Too light for text on its own');
    const shade = (await note.locator('code').innerText()).trim();
    expect(shade).toMatch(/^#[0-9A-F]{6}$/);
    await axe(page);
    await expect(row.getByLabel('Reason for the change')).toHaveCount(0); // branding needs no reason
    await row.getByRole('button', { name: 'Save' }).click();
    await expect(row.getByText('#FFE14D')).toBeVisible();

    // The server's derived text shade reads at 4.5:1 and is what the app paints, for everyone.
    const b = await (await owner.c.get('/api/v1/branding', { headers: owner.headers })).json();
    expect(b).toMatchObject({ accent: '#FFE14D', adjusted: true });
    expect(b.light.text).toBe(shade);
    // A logo: uploaded in Settings, shown in everyone's sidebar.
    const logoRow = page.getByRole('listitem').filter({ has: page.getByText('Logo', { exact: true }) });
    await logoRow.getByRole('button', { name: 'Change Logo' }).click();
    await logoRow.locator('#logo-input').setInputFiles({ name: 'acme.png', mimeType: 'image/png', buffer: PNG_1PX });
    await expect(logoRow.getByText('acme.png')).toBeVisible();
    await logoRow.getByRole('button', { name: 'Save' }).click();
    await expect(logoRow.getByText('Uploaded image')).toBeVisible();
    const them = await (await browser.newContext({ colorScheme: 'light' })).newPage();
    await signInPassword(them, m.email);
    await expect.poll(() => them.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--color-accent-text').trim().toUpperCase())).toBe(shade);
    await expect.poll(() => them.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--color-focus').trim().toUpperCase())).toBe(b.light.focus);
    await expect(them.locator('img.ws-logo')).toBeVisible();
    expect((await owner.c.get('/api/v1/branding/logo', { headers: owner.headers })).headers()['content-type']).toBe('image/png');
  });

  test('E2E-R1-06 controlled policy: self-approval is blocked, the reviewer approves with step-up, v2 is scheduled, goes live at its time, and v1 is marked superseded', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const rvMember = await member(request, owner, ws.id, 'rev', 'reviewer');
    const rv = await enter(rvMember.email, ws.id); // the reviewer turns on two-step verification
    const h = owner.headers;
    // v1: a controlled policy, reviewed and published.
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'cash', title: 'Cash handling', criticality: 'controlled', content: { blocks: [{ type: 'paragraph', text: 'Count cash twice.' }] } } })).json();
    await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h });
    expect((await rv.c.post(`/api/v1/versions/${r.version.id}/reviews`, { headers: rv.headers, data: { decision: 'approve' } })).status()).toBe(200);
    expect((await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} })).status()).toBe(200);

    // The author writes v2 and cannot approve it themselves.
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await page.getByRole('button', { name: 'New version' }).click();
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    await page.getByRole('textbox', { name: 'Content' }).click();
    await page.keyboard.press('End');
    await page.keyboard.type(' Two people sign the count sheet.');
    await page.getByRole('button', { name: 'Save draft' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Saved.' })).toBeVisible();
    await page.getByRole('button', { name: 'Submit for review' }).click();
    await expect(page.getByText('In review', { exact: true }).first()).toBeVisible();
    await page.getByRole('button', { name: 'Approve' }).click();
    await expect(page.getByText(/cannot review their own/i)).toBeVisible();

    // The reviewer approves; their sign-in is not recent, so they confirm it's them.
    const rp = await (await browser.newContext()).newPage();
    await signIn(rp, rvMember.email, rv.recovery[0]!);
    await expect(rp.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await request.post('/__test/session/age', { data: { email: rvMember.email, seconds: 3600 } });
    await rp.goto(`${WEB}/library/${r.artifact.id}`);
    await rp.getByRole('button', { name: 'Approve' }).click();
    const confirm = rp.getByRole('dialog', { name: 'Confirm it’s you' });
    await expect(confirm).toBeVisible();
    await confirm.getByRole("textbox").fill(totp(rv.secret, await serverNow(request), 1)); // the current step was used when the reviewer enrolled
    await confirm.getByRole('button', { name: /Verify|Confirm/ }).click();
    await expect(rp.getByRole('status').filter({ hasText: 'Approved.' })).toBeVisible();

    // The author schedules it; v1 stays current until then.
    await page.reload();
    await page.getByRole('button', { name: 'Publish' }).click();
    const pub = page.getByRole('dialog', { name: 'Publish v2' });
    await pub.getByLabel('What changed?').fill('Second signature');
    const later = new Date((await serverNow(request)) + 2 * 3_600_000);
    const local = new Date(later.getTime() - later.getTimezoneOffset() * 60_000).toISOString().slice(0, 16);
    await pub.getByLabel(/^Goes live/).fill(local);
    await pub.getByRole('button', { name: 'Schedule' }).click();
    await expect(page.getByText(/^Scheduled: goes live/)).toBeVisible();
    const versions = (await (await owner.c.get(`/api/v1/artifacts/${r.artifact.id}`, { headers: h })).json()).versions as { id: string; version_no: number; lifecycle: string }[];
    const v2 = versions.find((v) => v.version_no === 2)!;
    expect(v2.lifecycle).toBe('scheduled');
    expect(versions.find((v) => v.version_no === 1)!.lifecycle).toBe('published');

    // At its time it goes live and v1 says it is superseded, on the page and in the printable export.
    expect((await request.post('/__test/library/due', { data: { workspace: ws.id, version: v2.id } })).status()).toBe(204);
    await expect.poll(async () => { await request.post('/__test/drain'); return (await (await owner.c.get(`/api/v1/versions/${v2.id}`, { headers: h })).json()).lifecycle; }).toBe('published');
    const v1html = await (await owner.c.get(`/api/v1/versions/${r.version.id}/export?format=html`, { headers: h })).text();
    expect(v1html).toContain('<strong>Superseded.</strong> This is version 1, not the current version.');
    await page.reload();
    await expect(page.getByText('Two people sign the count sheet.')).toBeVisible();
  });

  test('E2E-R1-15 workspace overview: each activity tile drills into a pre-filtered list with the same count', async ({ page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    await member(request, owner, ws.id, 'one');
    await member(request, owner, ws.id, 'two');
    const published: string[] = [];
    for (const key of ['hr.leave', 'it.access']) {
      const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key, title: key, content: { blocks: [{ type: 'paragraph', text: key }] } } })).json();
      await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h });
      await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} });
      published.push(r.version.id);
    }
    const camp = await (await owner.c.post('/api/v1/campaigns', { headers: h, data: { version_id: published[0], title: 'Leave policy', audience: { all: true } } })).json();
    await owner.c.post(`/api/v1/campaigns/${camp.id}/activate`, { headers: h });
    const ann = await (await owner.c.post('/api/v1/announcements', { headers: h, data: { title: 'Welcome', content: { blocks: [{ type: 'paragraph', text: 'Hi' }] }, audience: { roles: ['member'] } } })).json();
    await owner.c.post(`/api/v1/announcements/${ann.id}/publish`, { headers: h });

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await nav(page, 'Overview');
    await expect(page.getByRole('heading', { level: 2, name: 'Activity' })).toBeVisible();
    await axe(page);

    const tile = (name: RegExp) => page.getByRole('link', { name });
    // Open acknowledgements → active campaigns, same number still to acknowledge.
    const open = (await tile(/open acknowledgements: see active campaigns/).getAttribute('aria-label'))!.split(' ')[0];
    await tile(/open acknowledgements: see active campaigns/).click();
    await expect(page.getByRole('button', { name: /^Active/ })).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByText(`${open} still to acknowledge`)).toBeVisible();
    // Versions published → the audit log filtered to publications in the range.
    await nav(page, 'Overview');
    const versions = (await tile(/versions published: see them/).getAttribute('aria-label'))!.split(' ')[0];
    expect(versions).toBe('2');
    await tile(/versions published: see them/).click();
    await expect(page.getByText(`${versions} events`, { exact: true })).toBeVisible();
    await expect(page.getByRole('table', { name: 'Audit events' }).getByText('Version published')).toHaveCount(2);
    // Announcements published → the published tab, same count.
    await nav(page, 'Overview');
    const anns = (await tile(/announcements published: see them/).getAttribute('aria-label'))!.split(' ')[0];
    await tile(/announcements published: see them/).click();
    await expect(page.getByRole('button', { name: /^Published/ })).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByRole('button', { name: /^Published/ }).locator('.seg-count')).toHaveText(anns);
    // Security strip → People & access filtered.
    await nav(page, 'Overview');
    const noMfa = await page.locator('.strip-item').filter({ hasText: 'without two-step verification' }).locator('.strip-value').innerText();
    await page.locator('.strip-item').filter({ hasText: 'without two-step verification' }).click();
    await expect(page.getByRole('button', { name: /^No two-step verification/ })).toHaveAttribute('aria-pressed', 'true');
    await expect(page.getByRole('button', { name: /^No two-step verification/ }).locator('.filter-chip-count')).toHaveText(noMfa);
  });

  test('E2E-R1-10 announcement approval workflow: draft → approver → published to everyone; at expiry it leaves the feed and shows as expired', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const adm = await member(request, owner, ws.id, 'approver', 'workspace.admin');
    const approver = await enter(adm.email, ws.id);
    const reader = await member(request, owner, ws.id, 'reader');

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await nav(page, 'Manage announcements');
    await page.getByRole('button', { name: 'New announcement' }).first().click();
    const dlg = page.getByRole('dialog', { name: 'New announcement' });
    await dlg.getByLabel('Title').fill('Holiday calendar 2027');
    await dlg.getByLabel('Message').fill('The holiday calendar for next year is out.');
    await dlg.getByRole('button', { name: 'Save draft' }).click();
    // Everyone-audience posts need someone else's approval: the drawer says so up front.
    const drawer = page.getByRole('dialog', { name: 'Holiday calendar 2027' });
    await expect(drawer.getByText('This one needs approval')).toBeVisible();
    await drawer.getByRole('button', { name: 'Send for approval' }).click();
    await expect(drawer.getByText('Waiting for approval', { exact: true })).toBeVisible();

    // The approver (a second admin) approves it.
    const list = await (await approver.c.get('/api/v1/announcements', { headers: approver.headers })).json();
    const a = list.items.find((x: { title: string }) => x.title === 'Holiday calendar 2027');
    expect((await approver.c.post(`/api/v1/announcements/${a.id}/approve`, { headers: approver.headers })).status()).toBe(200);
    await request.post('/__test/drain');

    const rp = await (await browser.newContext()).newPage();
    await signInPassword(rp, reader.email);
    await nav(rp, 'Announcements');
    await expect(rp.getByRole('listitem').filter({ hasText: 'Holiday calendar 2027' })).toBeVisible();

    // At its expiry it leaves the feed; admins see it as expired.
    expect((await request.post('/__test/announcements/expire', { data: { workspace: ws.id, announcement: a.id } })).status()).toBe(204);
    await rp.reload();
    await expect(rp.getByRole('heading', { level: 1, name: 'Announcements' })).toBeVisible();
    await expect(rp.getByText('Holiday calendar 2027')).toHaveCount(0);
    await page.reload();
    await page.getByRole('table', { name: 'Announcements' }).getByRole('button', { name: 'Holiday calendar 2027' }).click();
    await expect(page.getByText('Expired: it no longer shows in people’s feeds.')).toBeVisible();
  });
});
