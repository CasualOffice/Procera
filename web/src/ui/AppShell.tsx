import { Bell, BookOpen, Check, ChevronsUpDown, Gauge, House, LogOut, Megaphone, Monitor, Moon, ShieldCheck, Sun, Users } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Button as AriaButton, Header, Menu, MenuItem, MenuSection, MenuTrigger, Popover, Separator } from 'react-aria-components';
import { api } from '../lib/api';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from './Avatar';
import './shell.css';

export type NavKey = 'home' | 'inbox' | 'library' | 'announcements' | 'members' | 'overview' | 'people' | 'none';
type Theme = 'auto' | 'light' | 'dark';

function applyTheme(t: Theme) {
  if (t === 'auto') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.setAttribute('data-theme', t);
  try {
    localStorage.setItem('procera.theme', t);
  } catch {
    /* storage unavailable: theme lasts for this page only */
  }
}

/** Workspace app shell (docs/05 §2.1): workspace switcher, permission-shaped navigation, account menu. */
export function AppShell({
  children,
  current = 'home',
  crumbs = [],
  actions,
}: {
  children: React.ReactNode;
  current?: NavKey;
  crumbs?: string[];
  actions?: React.ReactNode;
}) {
  const { user, workspace, workspaces, can, selectWorkspace, signOut } = useSession();
  const [unread, setUnread] = useState(0);
  const [memberCount, setMemberCount] = useState<number | null>(null);
  const [theme, setTheme] = useState<Theme>(() => {
    try {
      return (localStorage.getItem('procera.theme') as Theme) || 'auto';
    } catch {
      return 'auto';
    }
  });
  useEffect(() => applyTheme(theme), [theme]);
  const [crumbOverride, setCrumbOverride] = useState<string | null>(null);
  useEffect(() => {
    const on = (e: Event) => setCrumbOverride((e as CustomEvent<string | null>).detail);
    window.addEventListener('procera:crumb', on);
    return () => window.removeEventListener('procera:crumb', on);
  }, []);
  useEffect(() => setCrumbOverride(null), [current, crumbs.join('/')]);

  useEffect(() => {
    if (!workspace) return;
    api<{ unread_count: number }>('/api/v1/notifications?limit=1').then((r) => setUnread(r.unread_count)).catch(() => undefined);
  }, [workspace, current]);
  useEffect(() => {
    if (!workspace || !can('member:view')) return;
    api<{ items: { status: string }[] }>('/api/v1/members').then((r) => setMemberCount(r.items.filter((m) => m.status === 'active').length)).catch(() => undefined);
  }, [workspace, can]);

  if (!workspace || !user) return null;

  const work = [
    { key: 'home' as const, label: 'Home', to: '/', icon: House, show: true },
    { key: 'inbox' as const, label: 'Notifications', to: '/notifications', icon: Bell, show: true, count: unread },
    { key: 'library' as const, label: 'Library', to: '/library', icon: BookOpen, show: can('policy:view') || can('sop:view') },
    { key: 'announcements' as const, label: 'Announcements', to: '/announcements', icon: Megaphone, show: can('announcement:view') },
  ];
  const admin = [
    { key: 'overview' as const, label: 'Overview', to: '/admin/overview', icon: Gauge, show: can('monitoring:view_dashboards') },
    { key: 'members' as const, label: 'Members', to: '/admin/members', icon: Users, show: can('member:invite') || can('role:view') },
    { key: 'people' as const, label: 'People & access', to: '/admin/people', icon: ShieldCheck, show: can('monitoring:view_dashboards') && can('member:view') },
  ];
  const groups = [
    { label: 'Workspace', items: work.filter((i) => i.show) },
    { label: 'Administration', items: admin.filter((i) => i.show) },
  ].filter((g) => g.items.length);

  return (
    <div className="app">
      <a className="skip-link" href="#main">Skip to main content</a>
      <aside className="sidebar">
        <MenuTrigger>
          <AriaButton className="ws-switcher" aria-label={`Workspace: ${workspace.name}. Switch workspace`}>
            <Avatar name={workspace.name} tone="brand" square size={30} />
            <span className="ws-switcher-text">
              <span className="ws-switcher-name">{workspace.name}</span>
              <span className="ws-switcher-meta">{memberCount === null ? 'Workspace' : `${memberCount} ${memberCount === 1 ? 'member' : 'members'}`}</span>
            </span>
            <ChevronsUpDown aria-hidden="true" size={16} className="ws-switcher-chev" />
          </AriaButton>
          <Popover className="menu-popover" placement="bottom start">
            <Menu
              className="menu"
              aria-label="Workspaces"
              onAction={async (key) => {
                if (key !== workspace.workspace_id) {
                  await selectWorkspace(String(key));
                  navigate('/', { replace: true });
                }
              }}
            >
              <MenuSection>
                <Header className="menu-header">Workspaces</Header>
                {workspaces.map((w) => (
                  <MenuItem key={w.workspace_id} id={w.workspace_id} className="menu-item" textValue={w.name}>
                    <Avatar name={w.name} tone="brand" square size={22} />
                    <span>{w.name}</span>
                    {w.workspace_id === workspace.workspace_id && (
                      <span className="menu-item-meta">
                        <Check aria-hidden="true" size={16} />
                        <span className="visually-hidden">Current</span>
                      </span>
                    )}
                  </MenuItem>
                ))}
              </MenuSection>
            </Menu>
          </Popover>
        </MenuTrigger>

        <nav className="nav" aria-label="Main">
          {groups.map((g) => (
            <div key={g.label}>
              <p className="nav-group-label">{g.label}</p>
              <ul>
                {g.items.map((i) => (
                  <li key={i.key}>
                    <Link to={i.to} aria-current={i.key === current ? 'page' : undefined} aria-label={'count' in i && i.count ? `${i.label}, ${i.count} unread` : undefined}>
                      <i.icon aria-hidden="true" size={16} />
                      <span>{i.label}</span>
                      {'count' in i && i.count ? <span className="nav-count" aria-hidden="true">{i.count > 99 ? '99+' : i.count}</span> : null}
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </nav>

        <MenuTrigger>
          <AriaButton className="user-btn" aria-label={`Account: ${user.display_name}`}>
            <Avatar name={user.display_name} size={28} />
            <span className="user-btn-text">
              <span className="user-btn-name">{user.display_name}</span>
              <span className="user-btn-email">{user.email}</span>
            </span>
          </AriaButton>
          <Popover className="menu-popover" placement="top start">
            <Menu
              className="menu"
              aria-label="Account"
              onAction={async (key) => {
                if (key === 'signout') {
                  await signOut();
                  navigate('/sign-in', { replace: true });
                } else setTheme(String(key).replace('theme-', '') as Theme);
              }}
            >
              <MenuSection>
                <Header className="menu-header">Appearance</Header>
                {([['auto', 'System', Monitor], ['light', 'Light', Sun], ['dark', 'Dark', Moon]] as const).map(([k, label, Icon]) => (
                  <MenuItem key={k} id={`theme-${k}`} className="menu-item" textValue={`${label} theme`}>
                    <Icon aria-hidden="true" size={16} /> {label}
                    {theme === k && (
                      <span className="menu-item-meta"><Check aria-hidden="true" size={16} /><span className="visually-hidden">Selected</span></span>
                    )}
                  </MenuItem>
                ))}
              </MenuSection>
              <Separator className="menu-separator" />
              <MenuItem id="signout" className="menu-item" textValue="Sign out">
                <LogOut aria-hidden="true" size={16} /> Sign out
              </MenuItem>
            </Menu>
          </Popover>
        </MenuTrigger>
      </aside>

      <div className="panel">
        <header className="topbar">
          <ol className="crumbs" aria-label="Breadcrumb">
            <li>{workspace.name}</li>
            {crumbs.map((c, i) => <li key={c}>{i === crumbs.length - 1 && crumbOverride ? crumbOverride : c}</li>)}
          </ol>
          <div className="topbar-actions">{actions}</div>
        </header>
        <main id="main" className="page" tabIndex={-1}>{children}</main>
      </div>
    </div>
  );
}

/** Pages outside a workspace (sign-in, chooser): one focused, centred card on the paper background. */
export function PlainShell({ children }: { children: React.ReactNode }) {
  return (
    <div className="auth">
      <a className="skip-link" href="#main">Skip to main content</a>
      <div className="auth-column">
        <div className="auth-brand">
          <img src="/procera-symbol.png" alt="" width={28} height={28} />
          <span>Procera</span>
        </div>
        <main id="main" className="auth-card" tabIndex={-1}>{children}</main>
        <p className="auth-legal">Policies, procedures and approvals, with a tamper-evident record of every change.</p>
      </div>
    </div>
  );
}

/** Lets a page name the last breadcrumb (e.g. a document title). */
export function setCrumb(title: string | null) {
  window.dispatchEvent(new CustomEvent('procera:crumb', { detail: title }));
}
