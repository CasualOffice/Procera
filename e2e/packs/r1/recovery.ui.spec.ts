import { type APIRequestContext, type Page, expect, test } from '@playwright/test';
import { PW, client, hasDatabase, uniqueEmail } from '../../fixtures/accounts';
import { axe, signIn, WEB } from '../../fixtures/ui';
import { type Session, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// Account recovery and admin account controls in the browser (docs/06 §3.6.1, OQ-15).
// E2E-R1-UI-48 — forgot password → email link → new password → sign in.
// E2E-R1-UI-49 — change password from Account › Security.
// E2E-R1-UI-50 — an admin requires a password change; the member chooses one at their next sign-in.

const NEW_PW = 'a brand new passphrase 2026';

/** A member (no admin role, so no second step) who joined the workspace by invitation. */
async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = uniqueEmail(name.toLowerCase());
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  const id = (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id;
  return { email, id };
}

/** The newest reset link in the system mail queue for `to`, as a path on the web origin. */
async function resetLink(request: APIRequestContext, to: string): Promise<string> {
  let path = '';
  await expect.poll(async () => {
    const mail = JSON.stringify(await (await request.get(`/__test/mail?to=${encodeURIComponent(to)}`)).json());
    path = [...mail.matchAll(/\/reset-password\?token=[A-Za-z0-9_-]+/g)].at(-1)?.[0] ?? '';
    return path;
  }, { message: `a reset email to ${to}` }).not.toBe('');
  return `${WEB}${path}`;
}

async function passwordSignIn(page: Page, email: string, password: string) {
  await page.goto(`${WEB}/sign-in`);
  await page.getByRole('textbox', { name: 'Work email' }).fill(email);
  await page.getByRole('textbox', { name: 'Password', exact: true }).fill(password);
  await page.getByRole('button', { name: 'Sign in' }).click();
}

const home = (page: Page) => expect(page.getByRole('heading', { level: 1, name: /^Good (morning|afternoon|evening)/ })).toBeVisible();

test.describe('account recovery @r1 @ui', () => {
  test.beforeEach(async ({ request }) => {
    test.slow();
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-UI-48 a member who forgot their password resets it from the emailed link and signs in', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const m = await member(request, owner, ws.id, 'Nina');
    await page.goto(`${WEB}/sign-in`);
    await page.getByRole('link', { name: 'Forgot password?' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Reset your password' })).toBeVisible();
    await axe(page);
    await page.getByRole('textbox', { name: 'Work email' }).fill(m.email);
    await page.getByRole('button', { name: 'Send reset link' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Check your email' })).toBeVisible();

    await page.goto(await resetLink(request, m.email));
    await expect(page.getByRole('heading', { level: 1, name: 'Choose a new password' })).toBeVisible();
    await expect(page).not.toHaveURL(/token=/); // the token leaves the address bar
    await axe(page);
    await page.getByRole('textbox', { name: 'New password', exact: true }).fill(NEW_PW);
    await page.getByRole('textbox', { name: 'Confirm new password' }).fill(NEW_PW);
    await page.getByRole('button', { name: 'Set new password' }).click();
    await expect(page.getByText('Your password is changed')).toBeVisible();

    await passwordSignIn(page, m.email, PW);
    await expect(page.getByRole('alert')).toBeVisible(); // the old password no longer works
    await passwordSignIn(page, m.email, NEW_PW);
    await home(page);
  });

  test('E2E-R1-UI-49 a member changes their password from Account › Security', async ({ page, request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const m = await member(request, owner, ws.id, 'Theo');
    await passwordSignIn(page, m.email, PW);
    await home(page);
    await page.goto(`${WEB}/account/security`);
    await page.getByRole('button', { name: 'Change password' }).click();
    const dialog = page.getByRole('dialog', { name: 'Change password' });
    await dialog.getByRole('textbox', { name: 'Current password' }).fill('not my password at all');
    await dialog.getByRole('textbox', { name: 'New password', exact: true }).fill(NEW_PW);
    await dialog.getByRole('textbox', { name: 'Confirm new password' }).fill(NEW_PW);
    await dialog.getByRole('button', { name: 'Update password' }).click();
    await expect(dialog.getByText('That isn’t your current password.')).toBeVisible();
    await dialog.getByRole('textbox', { name: 'Current password' }).fill(PW);
    await dialog.getByRole('button', { name: 'Update password' }).click();
    await expect(page.getByText(/Your password is changed/)).toBeVisible();
    await axe(page);
    await page.reload();
    await expect(page.getByRole('heading', { level: 1, name: 'Security' })).toBeVisible(); // still signed in here
  });

  test('E2E-R1-UI-50 an owner requires a password change and the member chooses one at their next sign-in', async ({ browser, page, request }) => {
    const { ws, ownerEmail, owner } = await workspaceWithOwner(request);
    const m = await member(request, owner, ws.id, 'Ravi');
    await signIn(page, ownerEmail, owner.recovery![0]!);
    await home(page);
    await page.goto(`${WEB}/members/${m.id}`);
    const actions = page.getByRole('list', { name: 'Account security actions' });
    await actions.getByRole('button', { name: 'Require password change' }).click();
    const dialog = page.getByRole('dialog', { name: 'Require Ravi to change their password?' });
    await dialog.getByRole('textbox', { name: 'Reason' }).fill('Shared on a sticky note');
    await dialog.getByRole('button', { name: 'Require password change' }).click();
    await expect(page.getByText('Ravi will choose a new password at their next sign-in.')).toBeVisible();
    await axe(page);

    const ravi = await (await browser.newContext()).newPage();
    await passwordSignIn(ravi, m.email, PW);
    await expect(ravi.getByRole('heading', { level: 1, name: 'Choose a new password' })).toBeVisible();
    await ravi.getByRole('textbox', { name: 'New password', exact: true }).fill(NEW_PW);
    await ravi.getByRole('textbox', { name: 'Confirm new password' }).fill(NEW_PW);
    await ravi.getByRole('button', { name: 'Save and continue' }).click();
    await home(ravi);
  });
});
