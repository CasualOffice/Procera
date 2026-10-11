import { type APIRequestContext, type Locator, type Page, expect, test } from '@playwright/test';
import { PW, hasDatabase, passwordSession, seedUser, serverNow } from '../../fixtures/accounts';
import { WEB, axe, nav, openSettings, signIn } from '../../fixtures/ui';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';
import { totp } from '../../fixtures/totp';
import { inviteFromMembers, joinAsNewPerson } from '../../fixtures/journey';
import { EICAR, PNG_1PX, upload } from '../../fixtures/files';
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
    await openSettings(page, 'General & branding');
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

    // Terminology: the workspace calls SOPs "Work Instructions"; labels follow, API values do not.
    // Terminology is rarely changed, so it sits under the page's Advanced disclosure.
    await openSettings(page, 'General & branding');
    await page.getByRole('button', { name: /^Advanced/ }).click();
    const terms =page.getByRole('listitem').filter({ has: page.getByText('Terminology', { exact: true }) });
    await terms.getByRole('button', { name: 'Change Terminology' }).click();
    await terms.getByLabel('Singular for SOP').fill('Work Instruction');
    await terms.getByLabel('Plural for SOP').fill('Work Instructions');
    await terms.getByRole('button', { name: 'Save' }).click();
    await expect(terms.getByText('SOP → Work Instruction')).toBeVisible();
    await nav(page, 'Home');
    await expect(page.getByRole('link', { name: 'New Work Instruction' })).toBeVisible();
    await them.reload();
    await nav(them, 'Library');
    await expect(them.getByText('Work Instructions').first()).toBeVisible();
    const t = await (await owner.c.get('/api/v1/branding', { headers: owner.headers })).json();
    expect(t.terms.sop).toEqual({ singular: 'Work Instruction', plural: 'Work Instructions' });
    expect((await owner.c.get('/api/v1/branding/logo', { headers: owner.headers })).headers()['content-type']).toBe('image/png');
  });

  test('E2E-R1-06 controlled policy: self-approval is blocked, the reviewer approves with step-up, v2 is scheduled, goes live at its time, and v1 is marked superseded', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const rvMember = await member(request, owner, ws.id, 'rev', 'reviewer');
    // The reviewer signs in with a password (the role doesn't require two-step verification).
    const rvs = await passwordSession(rvMember.email);
    const rvsw = await rvs.c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${rvs.token}` }, data: { workspace_id: ws.id } });
    const rv = { c: rvs.c, headers: { authorization: `Bearer ${(await rvsw.json()).access_token}` } };
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
    await signInPassword(rp, rvMember.email);
    await request.post('/__test/session/age', { data: { email: rvMember.email, seconds: 3600 } });
    await rp.goto(`${WEB}/library/${r.artifact.id}`);
    await rp.getByRole('button', { name: 'Approve' }).click();
    const confirm = rp.getByRole('dialog', { name: 'Confirm it’s you' });
    await expect(confirm).toBeVisible();
    await confirm.getByLabel('Password').fill(PW);
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
    await expect(page.getByRole('status').filter({ hasText: /^Scheduled for / })).toBeVisible();
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

  test('E2E-R1-05 clone Process Designer into HR Designer scoped to the HR collection: preview counts, the scope holds in the UI and the API, and missing permissions are refused', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const nina = await member(request, owner, ws.id, 'nina');
    const hr = await (await owner.c.post('/api/v1/collections', { headers: h, data: { name: 'HR' } })).json();
    const fin = await (await owner.c.post('/api/v1/collections', { headers: h, data: { name: 'Finance' } })).json();
    const draft = async (key: string, title: string, col: string) => {
      const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key, title, content: { blocks: [{ type: 'paragraph', text: title }] } } })).json();
      expect((await owner.c.put(`/api/v1/artifacts/${r.artifact.id}/collection`, { headers: h, data: { collection_id: col } })).status()).toBe(200);
      return r;
    };
    const onboarding = await draft('hr.onboarding', 'Onboarding', hr.id);
    const payroll = await draft('fin.payroll', 'Payroll run', fin.id);

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await nav(page, 'Roles & access');
    await expect(page.getByRole('heading', { level: 1, name: 'Roles & access' })).toBeVisible();
    await page.getByRole('button', { name: 'Process Designer', exact: true }).click();
    const pd = page.getByRole('dialog', { name: 'Process Designer' });
    await expect(pd.getByText('Built-in roles can’t be changed.')).toBeVisible();
    await pd.getByRole('button', { name: 'Duplicate' }).click();
    const dup = page.getByRole('dialog', { name: 'Duplicate Process Designer' });
    await dup.getByLabel('Name').fill('HR Designer');
    await dup.getByRole('button', { name: 'Duplicate role' }).click();
    await expect(page.getByText('Created HR Designer.')).toBeVisible();
    const hd = page.getByRole('dialog', { name: 'HR Designer' });
    await axe(page);

    // Grant it to Nina for the HR collection only.
    await hd.getByRole('button', { name: 'Grant role' }).click();
    const grant = page.getByRole('dialog', { name: 'Grant a role' });
    await grant.getByRole('button', { name: /Person$/ }).click();
    await page.getByRole('option', { name: `nina (${nina.email})`, exact: true }).click();
    await grant.getByText('A collection', { exact: true }).click();
    await grant.getByRole('button', { name: /Collection$/ }).click();
    await page.getByRole('option', { name: 'HR', exact: true }).click();
    await grant.getByRole('button', { name: 'Grant role' }).click();
    await expect(page.getByText('Role granted.')).toBeVisible();

    // A permission change previews who is affected, with the right count.
    const hd2 = page.getByRole('dialog', { name: 'HR Designer' }); // still open after granting
    await hd2.getByRole('button', { name: /^All/ }).click();
    const review1 = hd2.getByRole('checkbox', { name: /sop:review$/ });
    await hd2.locator('label.checkbox').filter({ hasText: /sop:review$/ }).click(); // a custom checkbox: press its label
    await expect(review1).toBeChecked();
    await hd2.getByRole('button', { name: 'Review changes' }).click();
    const review = page.getByRole('dialog', { name: 'Review changes to HR Designer' });
    await expect(review.getByText(/1 person gains 1 permission/)).toBeVisible();
    await review.getByRole('button', { name: 'Save changes' }).click();
    await expect(page.getByText('Role saved.')).toBeVisible();

    // The scope holds: Nina edits the HR draft, but not the Finance one, in the UI and the API.
    const np = await (await browser.newContext()).newPage();
    await signInPassword(np, nina.email);
    await np.goto(`${WEB}/library/${onboarding.artifact.id}`);
    await expect(np.getByRole('heading', { level: 1, name: 'Onboarding' })).toBeVisible();
    await expect(np.getByRole('button', { name: 'Edit', exact: true })).toBeVisible();
    await np.goto(`${WEB}/library/${payroll.artifact.id}`);
    await expect(np.getByRole('button', { name: 'Edit', exact: true })).toHaveCount(0);
    const ns = await passwordSession(nina.email);
    const sw = await ns.c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${ns.token}` }, data: { workspace_id: ws.id } });
    const nh = { authorization: `Bearer ${(await sw.json()).access_token}` };
    expect((await ns.c.patch(`/api/v1/versions/${onboarding.version.id}`, { headers: nh, data: { content: { blocks: [{ type: 'paragraph', text: 'Updated by HR' }] } } })).status()).toBe(200);
    expect([403, 404]).toContain((await ns.c.patch(`/api/v1/versions/${payroll.version.id}`, { headers: nh, data: { content: { blocks: [{ type: 'paragraph', text: 'Not mine' }] } } })).status());

    // Giving a permission the editor does not hold is refused: a role manager who can edit roles
    // but cannot export audit data tries to put audit export into HR Designer.
    const rm = await member(request, owner, ws.id, 'rolemgr');
    expect((await owner.c.post('/api/v1/roles', { headers: h, data: { key: 'custom.role_manager', name: 'Role manager', permissions: ['role:view', 'role:manage', 'sop:view'] } })).status()).toBe(201);
    expect((await owner.c.post('/api/v1/role-grants', { headers: h, data: { membership_id: rm.id, role_key: 'custom.role_manager', scope_kind: 'workspace' } })).status()).toBe(201);
    const rs = await passwordSession(rm.email);
    const rsw = await rs.c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${rs.token}` }, data: { workspace_id: ws.id } });
    const rh = { authorization: `Bearer ${(await rsw.json()).access_token}` };
    const hrRole = (await (await rs.c.get('/api/v1/roles', { headers: rh })).json()).items.find((r: { name: string }) => r.name === 'HR Designer');
    const esc = await rs.c.patch(`/api/v1/roles/${hrRole.id}`, { headers: rh, data: { permissions: [...hrRole.permissions, 'audit:export'] } });
    expect(esc.status(), await esc.text()).toBe(403);
    expect((await esc.json()).code).toBe('escalation');
  });

  test('E2E-R1-02 a system admin creates a Starter and an Enterprise workspace; the owner accepts from the email and turns on two-step verification', async ({ page, browser, request }) => {
    const sysEmail = await seedUser(request, 'sysadmin');
    await request.post('/__test/system-roles', { data: { email: sysEmail, role: 'system.admin' } });
    const sys = await (await import('../../fixtures/accounts')).mfaSession(sysEmail);
    await page.goto(`${WEB}/system`);
    await page.getByLabel('Work email').fill(sysEmail);
    await page.getByLabel('Password').fill(PW);
    await page.getByRole('button', { name: 'Sign in' }).click();
    await page.getByLabel('Code').fill(sys.recovery[0]!);
    await page.getByRole('button', { name: 'Verify' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Workspaces' })).toBeVisible();

    const suffix = Date.now().toString(36);
    const ownerA = uniqueEmail('owner-a');
    for (const [name, profile, owner] of [[`Northwind ${suffix}`, 'Starter', ownerA], [`Contoso ${suffix}`, 'Enterprise', uniqueEmail('owner-b')]] as const) {
      await page.getByRole('button', { name: 'New workspace' }).first().click();
      const dlg = page.getByRole('dialog', { name: 'New workspace' });
      await dlg.getByLabel('Workspace name').fill(name);
      await expect(dlg.getByLabel('Slug')).not.toHaveValue('');
      await dlg.getByText(profile, { exact: true }).click(); // the card (radio) is pressed through its label
      await expect(dlg.getByRole('radio', { name: new RegExp(profile) })).toBeChecked();
      await dlg.getByLabel('Owner email').fill(owner);
      await axe(page);
      await dlg.getByRole('button', { name: 'Create workspace' }).click();
      await expect(page.getByText(`${name} was created`)).toBeVisible();
      await expect(page.getByRole('table', { name: 'Workspaces' }).getByRole('button', { name: `${name}, open details` })).toBeVisible();
    }

    // Owner A accepts from the email, enrols two-step verification and lands in the workspace.
    const list = await (await sys.c.get('/sysapi/v1/workspaces', { headers: sys.headers })).json();
    const wsA = (list.items as { id: string; name: string }[]).find((w) => w.name === `Northwind ${suffix}`)!;
    const { emailLink } = await import('../../fixtures/journey');
    const link = await emailLink(request, wsA.id, ownerA, 'invitation', /\/invite\?token=/);
    const op = await (await browser.newContext()).newPage();
    await op.goto(link);
    await expect(op.getByRole('heading', { level: 1, name: `Join Northwind ${suffix}` })).toBeVisible();
    await op.getByLabel('Your name').fill('Owen Owner');
    await op.getByLabel(/^Password/).fill(PW);
    await op.getByLabel('Confirm password').fill(PW);
    await op.getByRole('button', { name: 'Create account and join' }).click();
    await expect(op.getByRole('heading', { level: 1, name: 'Turn on two-step verification' })).toBeVisible();
    await op.getByRole('button', { name: 'Get started' }).click();
    const secret = (await op.locator('.mfa-secret code').innerText()).replace(/\s+/g, '');
    await op.getByLabel('6-digit code').fill(totp(secret, await serverNow(request)));
    await op.getByRole('button', { name: 'Verify and turn on' }).click();
    await expect(op.getByRole('list', { name: 'Recovery codes' }).getByRole('listitem')).toHaveCount(10);
    await op.getByRole('button', { name: 'I’ve saved them, continue' }).click();
    await expect(op.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening), Owen/ })).toBeVisible();
  });

  test('E2E-R1-01 first-run setup page: a bad token is refused, and an already set-up platform says so (no default credentials)', async ({ page, request }) => {
    await page.goto(`${WEB}/setup`);
    await expect(page.getByRole('heading', { level: 1, name: /Set up Procera|Procera is already set up/ })).toBeVisible();
    if (await page.getByRole('heading', { level: 1, name: 'Set up Procera' }).isVisible()) {
      await axe(page);
      await page.getByLabel('Setup token').fill('not-a-real-token');
      await page.getByLabel('Work email').fill(uniqueEmail('root'));
      await page.getByLabel('Your name').fill('Root');
      await page.getByLabel(/^Password/).fill(PW);
      await page.getByLabel('Confirm password').fill(PW);
      await page.getByRole('button', { name: 'Create system owner' }).click();
      await expect(page.getByText(/This setup token doesn’t work|Procera is already set up/).first()).toBeVisible();
    }
    // No default account signs in.
    for (const [email, pw] of [['admin@procera.local', 'admin'], ['admin', 'admin'], ['root@localhost', 'changeme']]) {
      const r = await request.post('/api/v1/auth/login', { data: { email, password: pw } });
      expect([401, 422, 429]).toContain(r.status());
    }
  });

  test('E2E-R1-07 a major version asks everyone to re-acknowledge; 50 people do (10 on phones); the evidence lists the version hash and how each signed in', async ({ page, browser, request }) => {
    test.setTimeout(300_000);
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const people: { email: string; id: string }[] = [];
    for (let i = 0; i < 50; i += 10) {
      people.push(...(await Promise.all(Array.from({ length: 10 }, (_, k) => member(request, owner, ws.id, `emp${i + k}`)))));
    }
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'policy', key: 'conduct', title: 'Code of conduct', content: { blocks: [{ type: 'paragraph', text: 'Be kind.' }] } } })).json();
    await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} });
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${r.artifact.id}/versions`, { headers: h })).json();
    await owner.c.patch(`/api/v1/versions/${v2.id}`, { headers: h, data: { content: { blocks: [{ type: 'paragraph', text: 'Be kind. Report harassment.' }] } } });
    await owner.c.post(`/api/v1/versions/${v2.id}/submit`, { headers: h });

    // The owner publishes v2 as a major change in the browser.
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await page.goto(`${WEB}/library/${r.artifact.id}`);
    await page.getByRole('button', { name: 'Publish', exact: true }).click();
    const dlg = page.getByRole('dialog', { name: 'Publish v2' });
    await dlg.getByLabel('What changed?').fill('Harassment reporting');
    await expect(dlg.getByText(/^Publishing v2\.0 asks 51 people to re-acknowledge/)).toBeVisible();
    await dlg.getByRole('button', { name: 'Publish now' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Published v2.0. 51 people asked to re-acknowledge.' })).toBeVisible();
    await request.post('/__test/drain');

    // Ten acknowledge on a phone-sized screen, from Home.
    for (const p of people.slice(0, 10)) {
      const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
      const m = await ctx.newPage();
      await signInPassword(m, p.email);
      await m.getByRole('button', { name: 'Read and acknowledge' }).first().click();
      await m.getByRole('region', { name: 'Acknowledgement requested' }).getByRole('button', { name: 'I acknowledge' }).click();
      await expect(m.getByRole('status').filter({ hasText: 'Acknowledged' })).toBeVisible();
      await ctx.close();
    }
    // Forty more through the API (same endpoint the page uses).
    await Promise.all(people.slice(10).map(async (p) => {
      const s = await passwordSession(p.email);
      const sw = await s.c.post('/api/v1/session/workspace', { headers: { authorization: `Bearer ${s.token}` }, data: { workspace_id: ws.id } });
      const hh = { authorization: `Bearer ${(await sw.json()).access_token}` };
      const mine = (await (await s.c.get('/api/v1/me/acknowledgements?state=pending', { headers: hh })).json()).items;
      expect(mine).toHaveLength(1);
      const done = await s.c.post(`/api/v1/acknowledgements/${mine[0].id}/complete`, { headers: hh, data: { content_hash: mine[0].subject.content_hash } });
      expect(done.status()).toBe(200);
    }));

    // The evidence: 50 done, each with the v2 hash and the sign-in method.
    await page.reload();
    await page.getByRole('link', { name: /of 51 people/ }).click();
    await expect(page.getByText('50 of 51')).toBeVisible();
    const download = page.waitForEvent('download');
    await page.getByRole('button', { name: 'Evidence CSV' }).click();
    const csv: string = (await import('node:fs')).readFileSync((await (await download).path())!, 'utf8');
    const [head, ...rows] = csv.trim().split(/\r?\n/);
    const cols = head!.split(',');
    expect(cols).toEqual(expect.arrayContaining(['version', 'content_hash', 'auth_method']));
    const done = rows.map((l) => l.split(',')).filter((c) => c[cols.indexOf('state')] === 'completed');
    expect(done).toHaveLength(50);
    const hash = (await (await owner.c.get(`/api/v1/versions/${v2.id}`, { headers: h })).json()).content_hash;
    expect(done.every((c) => c[cols.indexOf('content_hash')] === hash && c[cols.indexOf('version')] === '2')).toBe(true);
    expect(done.every((c) => (c[cols.indexOf('auth_method')] ?? '').includes('pwd'))).toBe(true);
  });

  test('E2E-R1-11 a 50 MB attachment survives a dropped chunk and resumes; EICAR is quarantined and never downloadable; links are short-lived, bound and useless from another workspace', async ({ page, request }) => {
    test.setTimeout(240_000);
    const a = await workspaceWithOwner(request);
    const b = await workspaceWithOwner(request);
    const r = await (await a.owner.c.post('/api/v1/artifacts', { headers: a.owner.headers, data: { kind: 'sop', key: 'ops.big', title: 'Site survey', content: { blocks: [{ type: 'paragraph', text: 'See the survey.' }] } } })).json();
    await signIn(page, a.ownerEmail, a.owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await page.goto(`${WEB}/library/${r.artifact.id}`);

    // 50 MB; the third chunk is dropped once on the way, and the upload resumes from what arrived.
    const big = Buffer.alloc(50_000_000); // 50 MB (decimal), under the browser file-buffer limit
    for (let i = 0; i < big.length; i += 4096) big.writeUInt32LE((i * 2654435761) >>> 0, i);
    let dropped = false;
    await page.route(/\/api\/v1\/files\/[^/]+\/content\?offset=(\d+)/, async (route) => {
      const offset = Number(new URL(route.request().url()).searchParams.get('offset'));
      if (!dropped && offset >= 4 * 1024 * 1024) { dropped = true; await route.abort('connectionreset'); return; }
      await route.continue();
    });
    await page.locator('#att-input').setInputFiles({ name: 'survey.bin', mimeType: 'application/octet-stream', buffer: big });
    await expect(page.getByRole('progressbar', { name: 'Uploading' })).toBeVisible();
    await expect(page.getByText('survey.bin attached.')).toBeVisible({ timeout: 120_000 });
    expect(dropped).toBe(true);
    const list = page.getByRole('region', { name: 'Attachments' });
    await expect(list.getByRole('listitem').filter({ hasText: 'survey.bin' }).getByText('Clean')).toBeVisible();
    const att = (await (await a.owner.c.get(`/api/v1/versions/${r.version.id}/attachments`, { headers: a.owner.headers })).json()).items.find((f: { name: string }) => f.name === 'survey.bin');
    const { createHash } = await import('node:crypto');
    expect(att.sha256).toBe(createHash('sha256').update(big).digest('hex'));
    expect(att.size).toBe(big.length);

    // EICAR: quarantined, shown so, and no link is ever issued for it.
    await page.locator('#att-input').setInputFiles({ name: 'invoice.txt', mimeType: 'text/plain', buffer: Buffer.from(EICAR) });
    await expect(page.getByText(/invoice\.txt was quarantined/)).toBeVisible();
    const files = (await (await a.owner.c.get('/api/v1/files', { headers: a.owner.headers })).json()).items as { file: { id: string; name: string; state: string } }[];
    const eicar = files.find((f) => f.file.name === 'invoice.txt')!;
    expect(eicar.file.state).toBe('quarantined');
    expect((await a.owner.c.get(`/api/v1/files/${eicar.file.id}/download-url`, { headers: a.owner.headers })).status()).not.toBe(200);

    // Signed links: short-lived, tamper-proof, and workspace B can neither get one nor see the file.
    const link = await (await a.owner.c.get(`/api/v1/files/${att.id}/download-url`, { headers: a.owner.headers })).json();
    expect(Date.parse(link.expires_at) - (await serverNow(request))).toBeLessThanOrEqual(5 * 60_000);
    expect((await request.get(link.url)).status()).toBe(200);
    expect((await request.get(link.url.replace(/.$/, (c: string) => (c === 'A' ? 'B' : 'A')))).status()).toBe(403);
    expect((await b.owner.c.get(`/api/v1/files/${att.id}/download-url`, { headers: b.owner.headers })).status()).toBe(404);
    expect((await b.owner.c.get(`/api/v1/files/${att.id}`, { headers: b.owner.headers })).status()).toBe(404);
  });

  test('E2E-R1-12 Files: filter by scan state and type, change the retention class of several files at once, and a held file cannot be deleted', async ({ page, request }) => {
    const { ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'ops.keys', title: 'Keys', content: { blocks: [{ type: 'paragraph', text: 'Keys.' }] } } })).json();
    const held = await upload(owner, 'held.png', PNG_1PX);
    await owner.c.post(`/api/v1/files/${held.id}/links`, { headers: h, data: { object_type: 'artifact_version', object_id: r.version.id } });
    await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} });
    const a = await upload(owner, 'notes-a.txt', Buffer.from('a'));
    const b = await upload(owner, 'notes-b.txt', Buffer.from('b'));
    await upload(owner, 'eicar.txt', Buffer.from(EICAR));

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await nav(page, 'Files');
    const grid = page.getByRole('table', { name: 'Files' });
    // Scan state.
    await page.getByRole('button', { name: /^Quarantined/ }).click();
    await expect(grid.getByRole('row', { name: /eicar\.txt/ })).toBeVisible();
    await expect(grid.getByRole('row', { name: /notes-a/ })).toHaveCount(0);
    await page.getByRole('button', { name: /^All/ }).first().click();
    // Type, through the grid's filter builder.
    await page.getByRole('button', { name: /^Filter/ }).click();
    const builder = page.getByRole('form', { name: 'Add a filter' });
    await builder.getByLabel('Field').selectOption({ label: 'Name' });
    await builder.getByLabel('Condition').selectOption({ label: 'contains' });
    await builder.getByLabel('Value').fill('.txt');
    await builder.getByRole('button', { name: 'Add filter' }).click();
    await expect(grid.getByRole('row', { name: /held\.png/ })).toHaveCount(0);

    // Bulk retention change with a reason.
    for (const f of [a, b]) await grid.getByRole('checkbox', { name: `Select row ${f.id}` }).click();
    await page.getByRole('button', { name: 'Change retention class' }).click();
    const dlg = page.getByRole('dialog', { name: 'Change retention class of 2 files' });
    await dlg.getByLabel('Retention class').selectOption({ label: 'Operational' });
    await dlg.getByLabel(/^Reason/).fill('Working notes');
    await axe(page);
    await dlg.getByRole('button', { name: 'Change class' }).click();
    await expect(page.getByText('Retention class changed.')).toBeVisible();
    await expect(grid.getByRole('row', { name: /notes-a/ })).toContainText('Operational (set by an admin)');
    await expect(grid.getByRole('row', { name: /notes-b/ })).toContainText('Operational (set by an admin)');

    // A file held by a published version is refused.
    await page.getByRole('button', { name: /^Remove filter/ }).first().click();
    await grid.getByRole('row', { name: /held\.png/ }).getByRole('button', { name: 'Delete held.png' }).click();
    await expect(page.getByText(/a published or approved version uses this file/)).toBeVisible();
    await expect(grid.getByRole('row', { name: /held\.png/ })).toBeVisible();
    const audit = await (await owner.c.get('/api/v1/audit/events?action=file.retention_changed', { headers: h })).json();
    expect(audit.items).toHaveLength(2);
  });

  test('E2E-R1-13 DataGrid: multi-sort, typed and grouped (AND/OR) filters, a view shared with a role, deep link restores it, CSV and XLSX export what is shown', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const rev = await member(request, owner, ws.id, 'rita', 'reviewer');
    const docs: [string, string, string, boolean][] = [['policy', 'pol.alpha', 'Alpha policy', true], ['policy', 'pol.bravo', 'Bravo policy', false], ['sop', 'sop.charlie', 'Charlie SOP', true], ['sop', 'sop.delta', 'Delta SOP', false]];
    for (const [kind, key, title, publish] of docs) {
      const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind, key, title, content: { blocks: [{ type: 'paragraph', text: title }] } } })).json();
      if (publish) { await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h }); await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} }); }
    }
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await nav(page, 'Library');
    const table = page.getByRole('table', { name: 'Documents' });
    await expect(table.getByRole('row')).toHaveCount(5);

    // Status is Published AND (Name contains "Alpha" OR Name contains "Charlie").
    await page.getByRole('button', { name: /^Filter/ }).click();
    let builder = page.getByRole('form', { name: 'Add a filter' });
    await builder.getByLabel('Field').selectOption({ label: 'Status' });
    await builder.getByLabel('Published').check();
    await builder.getByRole('button', { name: 'Add filter' }).click();
    for (const word of ['Alpha', 'Charlie']) {
      await page.getByRole('button', { name: /^Filter/ }).click();
      builder = page.getByRole('form', { name: 'Add a filter' });
      await builder.getByLabel('Add to').selectOption({ label: word === 'Alpha' ? 'New group: match any (or)' : 'Group 1 (match any)' });
      await builder.getByLabel('Field').selectOption({ label: 'Name' });
      await builder.getByLabel('Condition').selectOption({ label: 'contains' });
      await builder.getByLabel('Value').fill(word);
      await builder.getByRole('button', { name: 'Add filter' }).click();
    }
    await expect(page.getByRole('group', { name: 'Filter group 1: match any' })).toBeVisible();
    await expect(page.getByText(/and \(Name contains “Alpha” or Name contains “Charlie”\)/)).toBeVisible();
    await expect(table.getByRole('row')).toHaveCount(3);
    // Multi-sort: by type, then by name (shift-click adds a second key).
    await table.getByRole('columnheader', { name: /Type/ }).getByRole('button').click();
    await table.getByRole('columnheader', { name: /Name/ }).getByRole('button').click({ modifiers: ['Shift'] });
    await expect(table.getByLabel('sort 2')).toBeVisible();
    await axe(page);

    // Save the view for Reviewers; the deep link brings it back for them.
    await page.getByRole('button', { name: 'Save view' }).click();
    const save = page.getByRole('dialog', { name: 'Save view' });
    await save.getByLabel('Name').fill('Published A or C');
    await save.getByText('A role…', { exact: true }).click();
    await save.getByRole('combobox', { name: 'Role' }).selectOption({ label: 'Reviewer' });
    await save.getByRole('button', { name: 'Save view' }).click();
    await expect(page).toHaveURL(/[?&]view=/);
    const deep = page.url();
    const rp = await (await browser.newContext()).newPage();
    await signInPassword(rp, rev.email);
    await rp.goto(deep);
    await expect(rp.getByRole('table', { name: 'Documents' }).getByRole('row')).toHaveCount(3);
    await expect(rp.getByRole('group', { name: 'Filter group 1: match any' })).toBeVisible();

    // Exports match the view: same rows, same columns.
    const csvDl = page.waitForEvent('download');
    await page.getByRole('button', { name: 'Export', exact: true }).click();
    const csv: string = (await import('node:fs')).readFileSync((await (await csvDl).path())!, 'utf8');
    const lines = csv.trim().split(/\r?\n/);
    expect(lines).toHaveLength(3);
    expect(csv).toContain('Alpha policy');
    expect(csv).toContain('Charlie SOP');
    expect(csv).not.toContain('Bravo');
    const xDl = page.waitForEvent('download');
    await page.getByRole('button', { name: 'Export XLSX' }).click();
    const x = await xDl;
    expect(x.suggestedFilename()).toMatch(/\.xlsx$/);
    const head = (await import('node:fs')).readFileSync((await x.path())!).subarray(0, 2).toString('latin1');
    expect(head).toBe('PK');
  });

  test('E2E-R1-18 startup day one (J12): a Starter workspace installs the handbook pack, publishes it, launches the acknowledgement, and 20 people acknowledge on phones', async ({ page, browser, request }) => {
    test.setTimeout(300_000);
    const started = Date.now();
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request, 'starter');
    const people: { email: string; id: string }[] = [];
    for (let i = 0; i < 20; i += 10) people.push(...(await Promise.all(Array.from({ length: 10 }, (_, k) => member(request, owner, ws.id, `starter${i + k}`)))));

    await signIn(page, ownerEmail, owner.recovery![0]!);
    await expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    // A new workspace's owner starts from the setup checklist; templates sit in Admin › Content.
    await expect(page.getByRole('region', { name: /^Set up / })).toBeVisible();
    await nav(page, 'Templates');
    await expect(page.getByRole('heading', { level: 1, name: 'Templates' })).toBeVisible();
    await axe(page);
    await page.getByRole('button', { name: 'Install Employee handbook & acknowledgement' }).click();
    const wiz = page.getByRole('dialog', { name: 'Install Employee handbook & acknowledgement' });
    await expect(wiz.getByRole('group', { name: 'Documents' }).getByRole('checkbox')).toHaveCount(5);
    await wiz.getByRole('button', { name: 'Continue' }).click();
    await wiz.getByRole('button', { name: 'Install 5 documents' }).click();
    const done = page.getByRole('dialog', { name: 'Template installed' });
    await expect(done.getByRole('list', { name: 'Installed documents' }).getByRole('link')).toHaveCount(5);
    await done.getByRole('link', { name: 'Employee handbook' }).click();

    // Publish the handbook (a standard policy needs no review on Starter).
    await expect(page.getByRole('heading', { level: 1, name: 'Employee handbook' })).toBeVisible();
    await page.getByRole('button', { name: 'Submit for review' }).click();
    await page.getByRole('button', { name: 'Publish', exact: true }).click();
    await page.getByRole('dialog', { name: 'Publish v1' }).getByRole('button', { name: 'Publish now' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Published.' })).toBeVisible();

    // Launch the acknowledgement for everyone.
    await nav(page, 'Campaigns');
    await page.getByRole('button', { name: 'New campaign' }).first().click();
    const dlg = page.getByRole('dialog', { name: 'New campaign' });
    await dlg.getByRole('button', { name: /Employee handbook/ }).click();
    await dlg.getByRole('button', { name: 'Create draft' }).click();
    await page.getByRole('button', { name: 'Launch' }).click();
    await expect(page.getByRole('table', { name: 'People in this campaign' })).toBeVisible();
    const campaignUrl = page.url();
    await request.post('/__test/drain');

    // 20 employees acknowledge on phone-sized screens.
    for (const p of people) {
      const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
      const m = await ctx.newPage();
      await signInPassword(m, p.email);
      await m.getByRole('button', { name: 'Read and acknowledge' }).first().click();
      await m.getByRole('region', { name: 'Acknowledgement requested' }).getByRole('button', { name: 'I acknowledge' }).click();
      await expect(m.getByRole('status').filter({ hasText: 'Acknowledged' })).toBeVisible();
      await ctx.close();
    }
    await page.goto(campaignUrl);
    await expect(page.getByText('20 of 21')).toBeVisible();
    expect(Date.now() - started).toBeLessThan(15 * 60_000);
  });
});
