import { chromium } from '@playwright/test';
const b = await chromium.launch();
for (const [sw, vw] of [[1440, 1440], [1512, 1512], [1728, 1728], [1920, 1920], [2560, 2560], [2560, 1280], [3840, 1920]]) {
  const ctx = await b.newContext({ viewport: { width: vw, height: 900 }, screen: { width: sw, height: 1440 } });
  const p = await ctx.newPage();
  await p.goto('http://127.0.0.1:5173/sign-in');
  await p.waitForTimeout(500);
  const r = await p.evaluate(() => ({ html: getComputedStyle(document.documentElement).fontSize, card: Math.round(document.querySelector('.auth-card')?.getBoundingClientRect().width ?? 0) }));
  console.log(`screen ${sw} window ${vw}`, JSON.stringify(r));
  await ctx.close();
}
await b.close();
