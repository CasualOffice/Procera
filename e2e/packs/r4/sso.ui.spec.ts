import { expect, test } from '@playwright/test';
import { PW, client, hasDatabase } from '../../fixtures/accounts';
import { oidcProvider } from '../../fixtures/oidc';
import { WEB, axe, nav, signIn } from '../../fixtures/ui';
import { inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R4-UI-08 — single sign-on in the browser (BPMS-140): the owner sets the provider in Admin ›
// Single sign-on (copies the redirect URL, the provider is checked on save); a member chooses
// "Sign in with SSO" on the sign-in page, enters their email, goes through the provider and lands
// in the workspace; a refusal (unverified email) comes back to the sign-in page with the reason; axe.

test.describe('single sign-on @r4 @ui @sso', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R4-UI-08 an owner sets up SSO and a member signs in through the provider', async ({ page, browser, request }) => {
    const { ws, owner, ownerEmail } = await workspaceWithOwner(request);
    const idp = await oidcProvider({ clientId: 'procera', clientSecret: 's3cret' });
    const dom = `sso-ui-${Date.now().toString(36)}.test`;
    try {
      await signIn(page, ownerEmail, owner.recovery![0]!);
      await nav(page, 'Admin');
      await page.getByRole('link', { name: 'Single sign-on' }).first().click();
      await expect(page.getByRole('heading', { level: 1, name: 'Single sign-on' })).toBeVisible();
      await expect(page.getByLabel('Redirect URL')).toHaveText(/\/api\/v1\/auth\/sso\/callback$/);
      await axe(page);
      await page.getByRole('textbox', { name: /^Issuer URL/ }).fill(idp.issuer);
      await page.getByRole('textbox', { name: /^Client ID/ }).fill('procera');
      await page.getByLabel(/^Client secret/).fill('s3cret');
      await page.getByRole('textbox', { name: /^Trusted email domains/ }).fill(dom);
      await page.getByRole('button', { name: 'Save' }).click();
      await expect(page.getByRole('status').filter({ hasText: 'Saved' })).toBeVisible();
      await expect(page.getByLabel(/^Client secret/)).toHaveAttribute('placeholder', /Saved/);

      const email = `ann@${dom}`;
      expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
      const token = await inviteToken(request, ws.id, email);
      expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: 'Ann Asker', password: PW } })).status()).toBe(201);

      // A refusal comes back to the sign-in page with the reason.
      const ctx = await browser.newContext();
      const p = await ctx.newPage();
      idp.as({ sub: 'kc-ann', email, email_verified: false });
      await p.goto(`${WEB}/`);
      await p.getByRole('button', { name: 'Sign in with SSO' }).click();
      await p.getByLabel('Work email').fill(email);
      await p.getByRole('button', { name: 'Continue with SSO' }).click();
      await expect(p.getByText('Single sign-on didn’t work')).toBeVisible();
      await expect(p.getByText(/verified email/)).toBeVisible();
      await axe(p);

      // Through the provider: lands in the workspace, signed in.
      idp.as({ sub: 'kc-ann', email, email_verified: true });
      await p.getByRole('button', { name: 'Sign in with SSO' }).click();
      await p.getByLabel('Work email').fill(email);
      await p.getByRole('button', { name: 'Continue with SSO' }).click();
      await expect(p.getByRole('navigation', { name: 'Main' })).toBeVisible();
      await expect(p).not.toHaveURL(/sign-in/);
      await ctx.close();
    } finally { await idp.close(); }
  });
});
