/** Initials avatar. `tone="brand"` for workspaces (white on deep red, 5.2:1), neutral for people. */
export function Avatar({ name, size = 28, tone = 'neutral', square = false }: { name: string; size?: number; tone?: 'neutral' | 'brand'; square?: boolean }) {
  const initials = name
    .split(/[\s@._-]+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase() ?? '')
    .join('');
  return (
    <span
      aria-hidden="true"
      className={`avatar avatar-${tone}${square ? ' avatar-square' : ''}`}
      style={{ width: size, height: size, fontSize: Math.round(size * 0.4) }}
    >
      {initials || '?'}
    </span>
  );
}
