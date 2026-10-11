import { Link } from '../lib/router';

export interface SectionTab { label: string; to: string; current: boolean }

/**
 * Views of one area as links (docs/05 §2.1): Library (All documents · Applies to me · My documents)
 * and Announcements (Feed · Manage). Each view keeps its own URL, so links and history work.
 */
export function SectionTabs({ label, tabs }: { label: string; tabs: (SectionTab | false)[] }) {
  const shown = tabs.filter(Boolean) as SectionTab[];
  if (shown.length < 2) return null;
  return (
    <nav className="section-tabs" aria-label={label}>
      <ul>
        {shown.map((t) => (
          <li key={t.to}><Link to={t.to} aria-current={t.current ? 'page' : undefined}>{t.label}</Link></li>
        ))}
      </ul>
    </nav>
  );
}
