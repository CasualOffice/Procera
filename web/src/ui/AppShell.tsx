import { Bell, ChevronDown, LogOut } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Button as AriaButton, Header, Menu, MenuItem, MenuSection, MenuTrigger, Popover, Separator } from 'react-aria-components';
import { api } from '../lib/api';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import './shell.css';

export type NavKey = 'home' | 'library' | 'admin' | 'none';

/** Workspace app shell (docs/05 §2.1): skip link, workspace switcher, permission-shaped navigation. */
export function AppShell({ children, current = 'home' }: { children: React.ReactNode; current?: NavKey }) {
  const { user, workspace, workspaces, can, selectWorkspace, signOut } = useSession();
  const [unread, setUnread] = useState(0);
  const [theme, setTheme] = useState<'auto' | 'light' | 'dark'>('auto');

  useEffect(() => {
    if (!workspace) return;
    api<{ unread_count: number }>('/api/v1/notifications?limit=1').then((r) => setUnread(r.unread_count)).catch(() => undefined);
  }, [workspace, current]);

  const cycle = () => {
    const next = theme === 'auto' ? 'light' : theme === 'light' ? 'dark' : 'auto';
    setTheme(next);
    if (next === 'auto') document.documentElement.removeAttribute('data-theme');
    else document.documentElement.setAttribute('data-theme', next);
  };

  // Hidden, not greyed out, when the member cannot use it (docs/05 §2.1).
  const nav: { key: NavKey; label: string; to: string; show: boolean }[] = [
    { key: 'home', label: 'Home', to: '/', show: true },
    { key: 'library', label: 'Library', to: '/library', show: can('policy:view') || can('sop:view') },
    { key: 'admin', label: 'Admin', to: '/admin/members', show: can('member:invite') || can('role:view') || can('workspace:settings.view') },
  ];

  return (
    <>
      <a className="skip-link" href="#main">Skip to main content</a>
      <header className="shell-header">
        <div className="shell-header-start">
          <Link className="shell-brand" to="/">
            <img src="/procera-symbol.png" alt="" width={24} height={24} />
            <span>Procera</span>
          </Link>
          {workspace && (
            <MenuTrigger>
              <AriaButton className="shell-switcher" aria-label={`Workspace: ${workspace.name}. Switch workspace`}>
                <span className="shell-switcher-name">{workspace.name}</span>
                <ChevronDown aria-hidden="true" size={16} />
              </AriaButton>
              <Popover className="menu-popover" placement="bottom start">
                <Menu
                  className="menu"
                  aria-label="Workspaces"
                  onAction={async (key) => {
                    if (key === 'signout') {
                      await signOut();
                      navigate('/sign-in', { replace: true });
                    } else if (key !== workspace.workspace_id) {
                      await selectWorkspace(String(key));
                      navigate('/', { replace: true });
                    }
                  }}
                >
                  <MenuSection>
                    <Header className="menu-header">Your workspaces</Header>
                    {workspaces.map((w) => (
                      <MenuItem key={w.workspace_id} id={w.workspace_id} className="menu-item" textValue={w.name}>
                        <span>{w.name}</span>
                        {w.workspace_id === workspace.workspace_id && <span className="menu-item-meta">Current</span>}
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
          )}
        </div>
        <div className="shell-header-end">
          {workspace && (
            <Link className="shell-icon-link" to="/notifications" aria-label={unread ? `Notifications, ${unread} unread` : 'Notifications'}>
              <Bell aria-hidden="true" size={20} />
              {unread > 0 && <span className="shell-badge" aria-hidden="true">{unread > 99 ? '99+' : unread}</span>}
            </Link>
          )}
          {user && <span className="shell-user">{user.display_name}</span>}
          <button type="button" className="shell-theme" onClick={cycle} aria-label={`Theme: ${theme}. Change theme`}>
            Theme: {theme}
          </button>
        </div>
      </header>
      <div className="shell-body">
        <nav className="shell-nav" aria-label="Main">
          <ul>
            {nav.filter((n) => n.show).map((n) => (
              <li key={n.key}>
                <Link to={n.to} aria-current={n.key === current ? 'page' : undefined}>{n.label}</Link>
              </li>
            ))}
          </ul>
        </nav>
        <main id="main" className="shell-main" tabIndex={-1}>{children}</main>
      </div>
    </>
  );
}

/** Pages outside the workspace (sign-in, chooser): brand header and a single readable column. */
export function PlainShell({ children }: { children: React.ReactNode }) {
  return (
    <>
      <a className="skip-link" href="#main">Skip to main content</a>
      <header className="shell-header">
        <span className="shell-brand">
          <img src="/procera-symbol.png" alt="" width={24} height={24} />
          <span>Procera</span>
        </span>
      </header>
      <main id="main" className="plain-main" tabIndex={-1}>{children}</main>
    </>
  );
}
