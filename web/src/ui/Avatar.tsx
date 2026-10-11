/** Initials avatar. Workspaces: white on deep red (5.2:1). People: a stable pastel from the name. */
export function Avatar({ name, size = 28, tone = 'person', square = false }: { name: string; size?: number; tone?: 'person' | 'brand' | 'neutral'; square?: boolean }) {
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
    <span aria-hidden="true" className={`avatar ${cls}${square ? ' avatar-square' : ''}`} style={{ width: size, height: size, fontSize: Math.max(10, Math.round(size * 0.4)) }}>
      {initials || '?'}
    </span>
  );
}
