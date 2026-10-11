/**
 * WCAG contrast and the accent shade rule (mirror of crates/settings/src/branding.rs): the closest
 * shade of a colour, moved toward black, that reads at 4.5:1 on the light surfaces. Used for the live
 * preview in Settings; the server's derivation is what the app paints.
 */
const rgb = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) || 0) as [number, number, number];
const lum = (h: string) => {
  const [r, g, b] = rgb(h).map((c) => { const v = c / 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; });
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
};
export const contrast = (a: string, b: string) => { const [x, y] = [lum(a), lum(b)]; return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05); };
const hex = (c: number[]) => `#${c.map((v) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, '0')).join('').toUpperCase()}`;
const LIGHT = ['#FFFFFF', '#F7F7F5', '#F1F1EF'];

/** The text-safe shade of `accent` on light surfaces, and whether it differs from the accent. */
export function textShade(accent: string): { shade: string; adjusted: boolean; ratio: number } {
  if (!/^#[0-9a-f]{6}$/i.test(accent)) return { shade: accent, adjusted: false, ratio: 0 };
  const [r, g, b] = rgb(accent);
  for (let step = 0; step <= 100; step++) {
    const t = 1 - step / 100;
    const s = hex([r * t, g * t, b * t]);
    if (LIGHT.every((bg) => contrast(s, bg) >= 4.5)) return { shade: s, adjusted: step > 0, ratio: contrast(s, '#FFFFFF') };
  }
  return { shade: '#000000', adjusted: true, ratio: 21 };
}
