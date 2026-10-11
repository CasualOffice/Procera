import { Link } from '../lib/router';
import { useSession } from '../lib/session';
import './directory.css';

export type PeopleTab = 'members' | 'departments' | 'groups' | 'roles';

/**
 * The People area's tab strip (Admin → People, docs/design/ia-competitive-analysis.md §5.4). Links,
 * not ARIA tabs: each tab is its own page with its own address. Tabs the member can't use are hidden.
 */
export function PeopleTabs({ current }: { current: PeopleTab }) {
  const { can } = useSession();
  const tabs: [PeopleTab, string, string, boolean][] = [
    ['members', 'Members', '/admin/members', can('member:invite') || can('role:view')],
    ['departments', 'Departments', '/admin/departments', can('directory:manage')],
    ['groups', 'Groups', '/admin/groups', can('directory:manage')],
    ['roles', 'Roles & access', '/admin/roles', can('role:view')],
  ];
  const shown = tabs.filter((t) => t[3] || t[0] === current);
  if (shown.length < 2) return null;
  return (
    <nav className="people-tabs" aria-label="People">
      {shown.map(([id, label, to]) => <Link key={id} to={to} aria-current={id === current ? 'page' : undefined}>{label}</Link>)}
    </nav>
  );
}
