// Renders site/assets/og.png (1200×630) from the brand banner SVG. Run it again when the
// banner changes; the PNG is committed so the site build itself needs no browser.
//
//   PLAYWRIGHT=e2e/node_modules/playwright node site/scripts/og-image.mjs
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '..', '..');
const require = createRequire(import.meta.url);
const { chromium } = require(path.resolve(process.env.PLAYWRIGHT || path.join(root, 'e2e', 'node_modules', 'playwright')));

const banner = readFileSync(path.join(root, 'assets/brand/vector/procera-banner.svg'), 'utf8');
const font = readFileSync(path.join(here, '..', 'assets/fonts/geist-latin-wght-normal.woff2')).toString('base64');
const html = `<!doctype html><html><head><style>
@font-face { font-family: Geist; src: url(data:font/woff2;base64,${font}) format('woff2'); font-weight: 100 900; }
html, body { margin: 0; }
body { width: 1200px; height: 630px; background: #0F172A; display: grid; grid-template-rows: 1fr auto; font-family: Geist, sans-serif; color: #F8FAFC; }
.banner { display: grid; place-items: center; padding: 0 60px; }
.banner svg { width: 1080px; height: auto; }
.foot { display: flex; justify-content: space-between; padding: 0 64px 48px; font-size: 26px; color: #CBD5E1; letter-spacing: -0.01em; }
.foot b { color: #F8FAFC; font-weight: 600; }
</style></head><body><div class="banner">${banner}</div>
<div class="foot"><span><b>Open source</b> · Apache-2.0 · Self-hosted</span><span>Policies · SOPs · Acknowledgements · Audit</span></div></body></html>`;

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1200, height: 630 } });
await page.setContent(html, { waitUntil: 'load' });
await page.screenshot({ path: path.join(here, '..', 'assets', 'og.png') });
await browser.close();
console.log('og-image: site/assets/og.png');
