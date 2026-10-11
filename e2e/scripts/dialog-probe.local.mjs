import { chromium } from '@playwright/test';
const b = await chromium.launch();
const ctx = await b.newContext({ viewport: { width: 1280, height: 720 }, screen: { width: 1920, height: 1080 } });
const p = await ctx.newPage();
await p.goto('http://127.0.0.1:5173/sign-in');
await p.getByLabel('Work email').fill('olivia@acme.test');
await p.getByLabel('Password').fill('correct horse battery staple');
await p.getByRole('button', { name: 'Sign in' }).click();
await p.waitForURL((u) => !u.pathname.includes('sign-in'));
await p.goto('http://127.0.0.1:5173/admin/announcements?new=1');
await p.waitForTimeout(1500);
const info = await p.evaluate(() => {
  const d = document.querySelector('[role=dialog]');
  const btns = [...document.querySelectorAll('[role=dialog] button')].map((x) => [x.textContent?.trim(), Math.round(x.getBoundingClientRect().bottom)]);
  return { root: getComputedStyle(document.documentElement).fontSize, dialog: d ? d.getBoundingClientRect().toJSON() : null, vh: innerHeight, btns };
});
console.log(JSON.stringify(info));
await p.screenshot({ path: process.argv[2] });
await b.close();
