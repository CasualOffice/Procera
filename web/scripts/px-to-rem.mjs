// Converts px to rem across the web app (owner direction 2026-10-11: rem everywhere, scaling with the
// fluid root font-size in base.css). Idempotent: run it again after merging UI work.
//   node scripts/px-to-rem.mjs          rewrite files
//   node scripts/px-to-rem.mjs --check  fail if any convertible px remains
//
// Kept in px on purpose: hairlines (≤ 2px: borders, outlines, focus rings stay crisp), shadow offsets
// and blurs, the pill radius (999px), and absolute floors written as `max(Npx, …rem)` (minimum target
// sizes must not depend on the root size). Everything else is value / 16 rem.
import { readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = new URL('../src/', import.meta.url).pathname;
const check = process.argv.includes('--check');
const rem = (px) => `${+(px / 16).toFixed(4)}rem`;
const keepPx = (n) => n <= 2 || n === 999;

function walk(dir, out = []) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p, out);
    else out.push(p);
  }
  return out;
}

const problems = [];

function convertCss(src, file) {
  // Comments are left alone (they may describe sizes in px); only code segments are converted.
  let inComment = false;
  return src
    .split('\n')
    .map((line, i) => {
      // Shadows keep px (offsets/blur are optical, not layout).
      if (!inComment && /(^|[\s;{])(box-shadow|text-shadow|--shadow[\w-]*)\s*:/.test(line)) return line;
      return line
        .split(/(\/\*|\*\/)/)
        .map((seg) => {
          if (seg === '/*') { inComment = true; return seg; }
          if (seg === '*/') { inComment = false; return seg; }
          if (inComment) return seg;
          return seg.replace(/(?<!max\()(-?\d*\.?\d+)px\b/g, (m, n) => {
            const v = Math.abs(parseFloat(n));
            if (keepPx(v)) return m;
            if (check) problems.push(`${file}:${i + 1}: ${m}`);
            return rem(parseFloat(n));
          });
        })
        .join('');
    })
    .join('\n');
}

// Inline React styles: unitless numbers are px in React for these properties.
const LENGTH_KEYS = 'fontSize|margin|marginTop|marginBottom|marginLeft|marginRight|marginInline|marginBlock|padding|paddingTop|paddingBottom|paddingLeft|paddingRight|paddingInline|paddingBlock|gap|rowGap|columnGap|width|height|minWidth|minHeight|maxWidth|maxHeight|top|left|right|bottom|borderRadius|inset';
const styleNum = new RegExp(`\\b(${LENGTH_KEYS}):\\s*(-?\\d*\\.?\\d+)(?=\\s*[,}])`, 'g');

function convertTsx(src, file) {
  let out = src.replace(/style=\{\{[^}]*\}\}/g, (block) =>
    block.replace(styleNum, (m, key, n) => {
      const v = Math.abs(parseFloat(n));
      if (v === 0 || keepPx(v)) return m;
      if (check) problems.push(`${file}: ${m}`);
      return `${key}: '${rem(parseFloat(n))}'`;
    }),
  );
  // lucide icons: size={16} → size="1rem" (the outer <svg> takes CSS lengths).
  out = out.replace(/\bsize=\{(\d+(?:\.\d+)?)\}/g, (m, n) => {
    if (check) problems.push(`${file}: ${m}`);
    return `size="${rem(parseFloat(n))}"`;
  });
  return out;
}

for (const file of walk(ROOT)) {
  if (file.endsWith('tokens.css')) continue; // generated from tokens.json
  const isCss = file.endsWith('.css');
  const isTsx = file.endsWith('.tsx');
  if (!isCss && !isTsx) continue;
  const src = readFileSync(file, 'utf8');
  const out = isCss ? convertCss(src, file.replace(ROOT, 'src/')) : convertTsx(src, file.replace(ROOT, 'src/'));
  if (!check && out !== src) writeFileSync(file, out);
}

// tokens.json: every px token except hairlines becomes rem.
const tokensPath = join(ROOT, 'ui/tokens.json');
const tokens = readFileSync(tokensPath, 'utf8');
const tokensOut = tokens.replace(/"(-?\d*\.?\d+)px"/g, (m, n) => {
  if (keepPx(Math.abs(parseFloat(n)))) return m;
  if (check) problems.push(`src/ui/tokens.json: ${m}`);
  return `"${rem(parseFloat(n))}"`;
});
if (!check && tokensOut !== tokens) writeFileSync(tokensPath, tokensOut);

if (check) {
  if (problems.length) {
    console.error(`px found where rem is required (run \`node scripts/px-to-rem.mjs\`):\n  ${problems.slice(0, 30).join('\n  ')}${problems.length > 30 ? `\n  … ${problems.length - 30} more` : ''}`);
    process.exit(1);
  }
  console.log('units: rem everywhere (px only for hairlines, shadows and the pill radius)');
} else {
  console.log('converted px → rem');
}
