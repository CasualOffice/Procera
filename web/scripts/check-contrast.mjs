// Verifies every declared colour pair in tokens.json meets its WCAG 2.x contrast minimum (docs/05 §7.1).
import { readFileSync } from 'node:fs';
const t = JSON.parse(readFileSync(new URL('../src/ui/tokens.json', import.meta.url), 'utf8'));
const lum = (hex) => {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((x) => (x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
};
const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
let failed = 0;
const rows = [];
for (const theme of ['light', 'dark']) {
  const pal = t.color[theme];
  const pairs = [...t.checks, ...(theme === 'light' ? t.checksLightOnly : t.checksDarkOnly)];
  for (const [fg, bg, min] of pairs) {
    const r = ratio(pal[fg], pal[bg]);
    const pass = r >= min;
    if (!pass) failed++;
    rows.push(`${pass ? 'ok  ' : 'FAIL'} ${theme.padEnd(5)} ${fg.padEnd(22)} on ${bg.padEnd(22)} ${r.toFixed(2).padStart(5)} (min ${min})`);
  }
}
console.log(rows.join('\n'));
if (failed) { console.error(`\n${failed} contrast pair(s) below WCAG minimum`); process.exit(1); }
console.log(`\nall ${rows.length} contrast pairs pass`);
