/** Initials avatar. Workspaces: white on deep red (5.2:1). People: a stable pastel from the name. */
/** `size` is a CSS length (rem, scales with the root size); numbers are treated as px and converted. */
export function Avatar({ name, size = '1.75rem', tone = 'person', square = false }: { name: string; size?: string | number; tone?: 'person' | 'brand' | 'neutral'; square?: boolean }) {
  const len = typeof size === 'number' ? `${size / 16}rem` : size;
  const initials = name
    .split(/[\s@._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase() ?? '')
    .join('');
  let h = 0;
  for (const c of name) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const cls = tone === 'brand' ? 'avatar-brand' : tone === 'neutral' ? 'avatar-neutral' : `avatar-c${h % 6}`;
  return (
    <span aria-hidden="true" className={`avatar ${cls}${square ? ' avatar-square' : ''}`} style={{ width: len, height: len, fontSize: `max(0.625rem, calc(${len} * 0.4))` }}>
      {initials || '?'}
    </span>
  );
}
