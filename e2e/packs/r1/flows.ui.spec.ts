import { expect, test } from '@playwright/test';
import { PW, hasDatabase, seedUser, serverNow, uniqueEmail } from '../../fixtures/accounts';
import { totp } from '../../fixtures/totp';
import { WEB, axe, nav, signIn } from '../../fixtures/ui';
import { emailLink, inviteFromMembers, joinAsNewPerson, personPage, signInForm } from '../../fixtures/journey';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-FLOW-01..05: complete user flows, start to finish, in the browser with every person in
// their own browser. Links are taken from the emails the product sent, the way people arrive.

test.describe('complete flows @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow(); // several people, each in their own browser
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-FLOW-01 onboarding: invited from Members, the email link creates the account and lands in the workspace', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    const email = uniqueEmail('newhire');
    await inviteFromMembers(page, email);

    const link = await emailLink(request, ws.id, email, 'invitation', /\/invite\?token=/);
    const hire = await personPage(browser);
    await hire.goto(link);
    await expect(hire.getByRole('heading', { level: 1, name: 'Join Acme' })).toBeVisible();
    await expect(hire.getByText(/Olivia invited/)).toBeVisible();
    await axe(hire);
    await hire.getByLabel('Your name').fill('Nina Hire');
    await hire.getByLabel(/^Password/).fill(PW);
    await hire.getByLabel('Confirm password').fill(`${PW}x`);
    await expect(hire.getByText('The passwords don’t match.')).toBeVisible();
    await expect(hire.getByRole('button', { name: 'Create account and join' })).toBeDisabled();
    await hire.getByLabel('Confirm password').fill(PW);
    await hire.getByRole('button', { name: 'Create account and join' }).click();
    await expect(hire.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening), Nina/ })).toBeVisible();
    await expect(hire.getByRole('button', { name: /Workspace: Acme/ })).toBeVisible();

    // The link worked once.
    await hire.goto(link);
    await expect(hire.getByRole('heading', { level: 1, name: 'This invitation link doesn’t work' })).toBeVisible();

    // The owner sees the new member as active.
    await page.reload();
    const row = page.getByRole('row', { name: /Nina Hire/ });
    await expect(row.getByText('Active')).toBeVisible();
    // Their person page (where manager escalations link) shows how to reach them.
    await row.getByRole('link', { name: 'Nina Hire' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Nina Hire' })).toBeVisible();
    await expect(page.getByRole('link', { name: email })).toHaveAttribute('href', `mailto:${email}`);
    await axe(page);
  });

  test('E2E-R1-FLOW-02 someone with an account signs in from the invitation and joins with one click', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const email = await seedUser(request, 'existing');
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await inviteFromMembers(page, email);

    const link = await emailLink(request, ws.id, email, 'invitation', /\/invite\?token=/);
    const them = await personPage(browser);
    await them.goto(link);
    await expect(them.getByText('You already have a Procera account. Sign in to accept.')).toBeVisible();
    await them.getByRole('button', { name: 'Sign in to accept' }).click();
    await signInForm(them, email);
    // Back on the invitation, now signed in.
    await expect(them).toHaveURL(/\/invite\?token=/);
    await them.getByRole('button', { name: 'Join Acme' }).click();
    await expect(them.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();
    await expect(them.getByRole('button', { name: /Workspace: Acme/ })).toBeVisible();

    // A link that never existed explains itself.
    await them.goto(`${WEB}/invite?token=not-a-real-token`);
    await expect(them.getByRole('heading', { level: 1, name: 'This invitation link doesn’t work' })).toBeVisible();
  });

  test('E2E-R1-FLOW-03 policy to evidence: write and publish, launch a campaign, a colleague acknowledges from the email, the owner sees it done', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    const email = uniqueEmail('colleague');
    await inviteFromMembers(page, email);
    const colleague = await joinAsNewPerson(browser, request, ws.id, email, 'Cara Colleague');

    // Owner: author and publish.
    await nav(page, 'Library');
    await page.getByRole('button', { name: 'New document' }).first().click();
    const create = page.getByRole('dialog', { name: 'New document' });
    await create.getByLabel('Title').fill('Travel policy');
    await create.getByRole('button', { name: 'Create draft' }).click();
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    await page.getByRole('textbox', { name: 'Content' }).click();
    await page.keyboard.type('Book travel through the portal at least two weeks ahead.');
    await page.getByRole('button', { name: 'Save draft' }).click();
    await page.getByRole('button', { name: 'Submit for review' }).click();
    await page.getByRole('button', { name: 'Publish' }).click();
    await page.getByRole('dialog', { name: 'Publish v1' }).getByRole('button', { name: 'Publish now' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Published.' })).toBeVisible();

    // Owner: campaign for everyone, launched.
    await nav(page, 'Campaigns');
    await page.getByRole('button', { name: 'New campaign' }).first().click();
    const dlg = page.getByRole('dialog', { name: 'New campaign' });
    await dlg.getByRole('button', { name: /Travel policy/ }).click();
    await dlg.getByRole('button', { name: 'Create draft' }).click();
    await expect(page.getByText(/people will be asked to acknowledge/)).toBeVisible();
    await page.getByRole('button', { name: 'Launch' }).click();
    await expect(page.getByRole('table', { name: 'People in this campaign' })).toBeVisible();
    const campaignUrl = page.url();

    // Colleague: the task is on Home; the email link opens the document with the request.
    await colleague.reload();
    await expect(colleague.getByText('Acknowledge “Travel policy”')).toBeVisible();
    const ackLink = await emailLink(request, ws.id, email, 'campaign_assigned', /\/acknowledgements\//);
    await colleague.goto(ackLink);
    await expect(colleague.getByRole('heading', { level: 1, name: 'Travel policy' })).toBeVisible();
    const callout = colleague.getByRole('region', { name: 'Acknowledgement requested' });
    await expect(callout).toBeVisible();
    await callout.getByRole('button', { name: 'I acknowledge' }).click();
    await expect(colleague.getByRole('status').filter({ hasText: 'Acknowledged' })).toBeVisible();
    await colleague.goto(`${WEB}/`);
    await expect(colleague.getByText('Acknowledge “Travel policy”')).toHaveCount(0);

    // Owner: the colleague is done; the evidence downloads.
    await page.goto(campaignUrl);
    const row = page.getByRole('row', { name: /Cara Colleague/ });
    await expect(row.getByText('Done')).toBeVisible();
    await expect(page.getByText(/1 of 2/)).toBeVisible();
    const download = page.waitForEvent('download');
    await page.getByRole('button', { name: 'Evidence CSV' }).click();
    expect((await download).suggestedFilename()).toMatch(/evidence\.csv$/);
    await axe(page);
  });

  test('E2E-R1-FLOW-04 announcement: compose and publish, a colleague opens it from the email signed out, reads it, the owner sees the receipt', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    await request.post('/__test/settings', { data: { workspace: ws.id, key: 'announcements.approval_required', value: 'off' } });
    await signIn(page, ownerEmail, owner.recovery![0]!);
    const email = uniqueEmail('reader');
    await inviteFromMembers(page, email);
    const reader = await joinAsNewPerson(browser, request, ws.id, email, 'Rae Reader');
    await reader.close(); // they come back later from the email, signed out

    await nav(page, 'Manage announcements');
    await page.getByRole('button', { name: 'New announcement' }).first().click();
    const dlg = page.getByRole('dialog', { name: 'New announcement' });
    await dlg.getByLabel('Title').fill('Office closed Friday');
    await dlg.getByLabel('Message').fill('The office is closed for maintenance. Work from home.');
    await dlg.getByText('Ask people to confirm they’ve read it').click();
    await dlg.getByText('Critical', { exact: true }).click();
    await dlg.getByRole('button', { name: 'Save draft' }).click();
    await page.getByRole('button', { name: 'Publish now' }).click();
    await expect(page.getByRole('heading', { name: 'Receipts' })).toBeVisible();

    const link = await emailLink(request, ws.id, email, 'announcement', /\/announcements\//);
    const back = await personPage(browser);
    await back.goto(link);
    await signInForm(back, email);
    const post = back.getByRole('listitem').filter({ hasText: 'Office closed Friday' });
    await expect(post.getByText('The office is closed for maintenance.')).toBeVisible();
    await post.getByRole('button', { name: 'I’ve read this' }).click();
    await expect(post.getByText('Acknowledged')).toBeVisible();

    await page.reload();
    await page.getByRole('table', { name: 'Announcements' }).getByRole('button', { name: 'Office closed Friday' }).click();
    await expect(page.getByText(/\(1 of 2\)/).first()).toBeVisible();
  });

  test('E2E-R1-FLOW-05 review loop: a controlled draft is submitted, the reviewer is emailed and asks for changes, the author fixes it, the reviewer approves, the author publishes', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    const email = uniqueEmail('reviewer');
    await inviteFromMembers(page, email, 'Reviewer');
    const reviewer = await joinAsNewPerson(browser, request, ws.id, email, 'Rob Reviewer');

    // Author: a controlled policy always needs an independent review.
    await nav(page, 'Library');
    await page.getByRole('button', { name: 'New document' }).first().click();
    const create = page.getByRole('dialog', { name: 'New document' });
    await create.getByLabel('Title').fill('Data retention');
    await create.getByText('Controlled', { exact: true }).click();
    await create.getByRole('button', { name: 'Create draft' }).click();
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    await page.getByRole('textbox', { name: 'Content' }).click();
    await page.keyboard.type('Keep customer records for five years.');
    await page.getByRole('button', { name: 'Save draft' }).click();
    await page.getByRole('button', { name: 'Submit for review' }).click();
    await expect(page.getByText('In review', { exact: true }).first()).toBeVisible();
    const docUrl = page.url();

    // Reviewer: the email asks for a review; they send it back with a comment.
    const reviewLink = await emailLink(request, ws.id, email, 'review_requested', /\/library\//);
    await reviewer.goto(reviewLink);
    await expect(reviewer.getByRole('heading', { level: 1, name: 'Data retention' })).toBeVisible();
    await reviewer.getByRole('button', { name: 'Request changes' }).click();
    const changes = reviewer.getByRole('dialog', { name: 'Request changes' });
    await changes.getByLabel('What should change?').fill('Say which records and who deletes them.');
    await changes.getByRole('button', { name: 'Send' }).click();
    await expect(reviewer.getByRole('status').filter({ hasText: 'Sent back to the author.' })).toBeVisible();

    // Author: the notification explains what to change and leads back to the draft.
    await request.post('/__test/drain');
    await nav(page, 'Notifications');
    const note = page.getByRole('listitem').filter({ hasText: 'Changes requested on Data retention' });
    await expect(note.getByText('Say which records and who deletes them.')).toBeVisible();
    await note.getByRole('link', { name: 'Changes requested on Data retention' }).click();
    await expect(page).toHaveURL(docUrl);
    await page.getByRole('button', { name: 'Edit', exact: true }).click();
    await page.getByRole('textbox', { name: 'Content' }).click();
    await page.keyboard.press('End');
    await page.keyboard.type(' The records team deletes them after that.');
    await page.getByRole('button', { name: 'Save draft' }).click();
    await page.getByRole('button', { name: 'Submit for review' }).click();
    await expect(page.getByText('In review', { exact: true }).first()).toBeVisible();

    // Reviewer approves from Home this time.
    await reviewer.goto(`${WEB}/`);
    await reviewer.getByRole('listitem').filter({ hasText: 'Review “Data retention”' }).getByRole('button', { name: 'Review' }).click();
    await reviewer.getByRole('button', { name: 'Approve' }).click();
    await expect(reviewer.getByRole('status').filter({ hasText: 'Approved.' })).toBeVisible();

    // Author: told it is approved, publishes.
    await request.post('/__test/drain');
    await nav(page, 'Notifications');
    await page.getByRole('link', { name: 'Data retention is approved' }).click();
    await page.getByRole('button', { name: 'Publish' }).click();
    await page.getByRole('dialog', { name: 'Publish v1' }).getByRole('button', { name: 'Publish now' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Published.' })).toBeVisible();
    await axe(page);
  });

  test('E2E-R1-FLOW-06 announcement approval: a second admin joins and turns on two-step verification, gets the approval email, sends the post back; the author fixes it; the admin approves and it reaches people', async ({ page, browser, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    await signIn(page, ownerEmail, owner.recovery![0]!);
    const email = uniqueEmail('approver');
    await inviteFromMembers(page, email, 'Admin');

    // The new admin creates an account; their role needs two-step verification, set up right away.
    const link = await emailLink(request, ws.id, email, 'invitation', /\/invite\?token=/);
    const admin = await personPage(browser);
    await admin.goto(link);
    await admin.getByLabel('Your name').fill('Ada Approver');
    await admin.getByLabel(/^Password/).fill(PW);
    await admin.getByLabel('Confirm password').fill(PW);
    await admin.getByRole('button', { name: 'Create account and join' }).click();
    await expect(admin.getByRole('heading', { level: 1, name: 'Turn on two-step verification' })).toBeVisible();
    await admin.getByRole('button', { name: 'Get started' }).click();
    const secret = (await admin.locator('.mfa-secret code').innerText()).replace(/\s+/g, '');
    await admin.getByLabel('6-digit code').fill(totp(secret, await serverNow(request)));
    await admin.getByRole('button', { name: 'Verify and turn on' }).click();
    const recovery = await admin.getByRole('list', { name: 'Recovery codes' }).getByRole('listitem').first().innerText();
    await admin.getByRole('button', { name: /sign in again/ }).click();
    await signInForm(admin, email);
    await admin.getByLabel('Code').fill(recovery);
    await admin.getByRole('button', { name: 'Verify' }).click();
    await expect(admin.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening), Ada/ })).toBeVisible();

    // Owner writes a workspace-wide post: it needs someone else's approval.
    await nav(page, 'Manage announcements');
    await page.getByRole('button', { name: 'New announcement' }).first().click();
    const dlg = page.getByRole('dialog', { name: 'New announcement' });
    await dlg.getByLabel('Title').fill('New expense tool');
    await dlg.getByLabel('Message').fill('Submit expenses in the new tool from Monday.');
    await dlg.getByRole('button', { name: 'Save draft' }).click();
    await page.getByRole('button', { name: 'Send for approval' }).click();
    await expect(page.getByText('Waiting for a publisher to approve it.', { exact: false })).toBeVisible();

    // Admin: the approval email opens the post; they send it back with a note.
    const approve = await emailLink(request, ws.id, email, 'announcement_approval', /\/admin\/announcements\?id=/);
    await admin.goto(approve);
    await expect(admin.getByText('Submit expenses in the new tool from Monday.')).toBeVisible();
    await admin.getByRole('button', { name: 'Request changes' }).click();
    const back = admin.getByRole('dialog', { name: 'Request changes' });
    await back.getByLabel('What should change?').fill('Add the link to the tool.');
    await back.getByRole('button', { name: 'Send back' }).click();
    await expect(admin.getByText('Sent back with changes requested. Edit it, then send it again.')).toBeVisible();

    // Owner: told it came back, with the note; edits and resubmits from the notification.
    await page.getByRole('dialog', { name: 'New expense tool' }).getByRole('button', { name: 'Close' }).click(); // the drawer stays open after sending
    await request.post('/__test/drain');
    await nav(page, 'Notifications');
    const note = page.getByRole('listitem').filter({ hasText: '“New expense tool” was sent back' });
    await expect(note.getByText('Add the link to the tool.')).toBeVisible();
    await note.getByRole('link', { name: '“New expense tool” was sent back' }).click();
    await page.getByRole('button', { name: 'Edit' }).first().click();
    const edit = page.getByRole('dialog', { name: 'Edit announcement' });
    await edit.getByLabel('Message').fill('Submit expenses in the new tool from Monday: expenses.acme.test');
    await edit.getByRole('button', { name: 'Save draft' }).click();
    await page.getByRole('button', { name: 'Send for approval' }).click();
    await expect(page.getByText('Waiting for a publisher to approve it.', { exact: false })).toBeVisible();

    // Admin approves; the owner hears it; the post is in everyone's feed.
    await admin.reload();
    await admin.getByRole('button', { name: 'Approve and publish' }).click();
    await expect(admin.getByRole('heading', { name: 'Receipts' })).toBeVisible();
    // Focus stayed in the owner's drawer after its button went away, so Esc still closes it.
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: 'New expense tool' })).toHaveCount(0);
    await request.post('/__test/drain');
    await nav(page, 'Notifications');
    await expect(page.getByRole('link', { name: '“New expense tool” is approved' })).toBeVisible();
    await nav(page, 'Announcements');
    await expect(page.getByRole('listitem').filter({ hasText: 'New expense tool' })).toBeVisible();
    await axe(admin);
  });
});
