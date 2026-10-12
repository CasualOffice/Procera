import { ArrowLeft, Bell, Check, ChevronsUpDown, History, LogOut, Monitor, Moon, PauseCircle, Search, ShieldCheck, UserRound, Sun } from 'lucide-react';
import { useEffect, useState } from 'react';
import { Button as AriaButton, Header, Menu, MenuItem, MenuSection, MenuTrigger, Popover, Separator } from 'react-aria-components';
import { ApiError, api, blobUrl } from '../lib/api';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { setTerms, useTerms } from '../lib/terms';
import { Avatar } from './Avatar';
import { CommandPalette, shortcutLabel } from './CommandPalette';
import { type NavItem, type NavKey, adminNav, isAdminPage, workNav } from './nav';
import './shell.css';
import { ShellSkeleton } from './States';

export type { NavKey } from './nav';
type Theme = 'auto' | 'light' | 'dark';
/** A breadcrumb: a plain label (current page) or `[label, to]` for a link back. */
export type Crumb = string | [string, string];

function applyTheme(t: Theme) {
  if (t === 'auto') document.documentElement.removeAttribute('data-theme');
  else document.documentElement.setAttribute('data-theme', t);
  try {
    localStorage.setItem('procera.theme', t);
  } catch {
    /* storage unavailable: theme lasts for this page only */
  }
}

/**
 * Workspace app shell (docs/05 §2.1): workspace switcher, search (⌘K), permission-shaped
 * navigation in two groups, account menu, and linked breadcrumbs so every page has a way back.
 */
export function AppShell({ children, current = 'home', crumbs = [], actions }: { children: React.ReactNode; current?: NavKey; crumbs?: Crumb[]; actions?: React.ReactNode }) {
  const { user, workspace, workspaces, can, selectWorkspace, signOut, reload } = useSession();
  useTerms(); // navigation labels use the workspace's terms
  const [unread, setUnread] = useState(0);
  const [logo, setLogo] = useState<string | null>(null);
  const [memberCount, setMemberCount] = useState<number | null>(null);
  const [palette, setPalette] = useState(false);
  const [theme, setTheme] = useState<Theme>(() => {
    try {
      return (localStorage.getItem('procera.theme') as Theme) || 'auto';
    } catch {
      return 'auto';
    }
  });
  useEffect(() => applyTheme(theme), [theme]);
  const [crumbOverride, setCrumbOverride] = useState<string | null>(null);
  const crumbKey = crumbs.map((c) => (typeof c === 'string' ? c : c[0])).join('/');
  useEffect(() => {
    const on = (e: Event) => setCrumbOverride((e as CustomEvent<string | null>).detail);
    window.addEventListener('procera:crumb', on);
    return () => window.removeEventListener('procera:crumb', on);
  }, []);
  useEffect(() => setCrumbOverride(null), [current, crumbKey]);

  // ⌘K / Ctrl K opens search from anywhere in the app.
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === 'k') { e.preventDefault(); setPalette((o) => !o); }
    };
    window.addEventListener('keydown', on);
    return () => window.removeEventListener('keydown', on);
  }, []);

  useEffect(() => {
    if (!workspace) return;
    api<{ unread_count: number }>('/api/v1/notifications?limit=1').then((r) => setUnread(r.unread_count)).catch((e: unknown) => {
      // Removed from this workspace (deactivated, or it was suspended away): re-read the session so the
      // chooser explains it, instead of every page failing on its own.
      if (e instanceof ApiError && e.status === 404) void reload();
    });
  }, [workspace, current, reload]);
  // Workspace accent (docs/05 §7): the server derives contrast-safe shades; paint them over the
  // product defaults so links, focus and active states carry the workspace's colour.
  useEffect(() => {
    if (!workspace) return;
    let live = true;
    api<{ light: Record<string, string>; dark: Record<string, string>; logo: string | null; terms?: Record<string, { singular: string; plural: string }> }>('/api/v1/branding').then((b) => {
      if (!live) return;
      setTerms(b.terms); // workspace terminology (CFG-008)
      if (b.logo) void blobUrl(b.logo).then((u) => { if (live) setLogo(u); else URL.revokeObjectURL(u); }).catch(() => undefined);
      else setLogo(null);
      const vars = (x: Record<string, string>) => `--color-accent-text:${x.text};--color-accent-strong:${x.strong};--color-accent-strong-hover:${x.strong_hover};--color-focus:${x.focus};`;
      let el = document.getElementById('ws-brand') as HTMLStyleElement | null;
      if (!el) { el = document.createElement('style'); el.id = 'ws-brand'; document.head.appendChild(el); }
      el.textContent = `:root{${vars(b.light)}}@media (prefers-color-scheme: dark){:root:not([data-theme='light']){${vars(b.dark)}}}:root[data-theme='dark']{${vars(b.dark)}}`;
    }).catch(() => undefined);
    return () => { live = false; document.getElementById('ws-brand')?.remove(); setTerms(null); };
  }, [workspace]);
  useEffect(() => () => { if (logo) URL.revokeObjectURL(logo); }, [logo]);
  useEffect(() => {
    if (!workspace || !can('member:view')) return;
    api<{ items: { status: string }[] }>('/api/v1/members').then((r) => setMemberCount(r.items.filter((m) => m.status === 'active').length)).catch(() => undefined);
  }, [workspace, can]);

  if (!workspace || !user) return <ShellSkeleton />;
  const adminGroups = adminNav(can);
  const adminMode = isAdminPage(current) && adminGroups.length > 0;
  const item = (i: NavItem) => (
    <li key={i.key}>
      <Link to={i.to} aria-current={i.key === current ? 'page' : undefined}>
        <i.icon aria-hidden="true" size="1rem" />
        <span>{i.label}</span>
      </Link>
    </li>
  );

  return (
    <div className="app">
      <a className="skip-link" href="#main">Skip to main content</a>
      <aside className="sidebar">
        <MenuTrigger>
          <AriaButton className="ws-switcher" aria-label={`Workspace: ${workspace.name}. Switch workspace`}>
            {logo ? <img className="ws-logo" src={logo} alt="" /> : <Avatar name={workspace.name} tone="brand" square size="1.875rem" />}
            <span className="ws-switcher-text">
              <span className="ws-switcher-name">{workspace.name}</span>
              <span className="ws-switcher-meta">{memberCount === null ? 'Workspace' : `${memberCount} ${memberCount === 1 ? 'member' : 'members'}`}</span>
            </span>
            <ChevronsUpDown aria-hidden="true" size="1rem" className="ws-switcher-chev" />
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
                    <Avatar name={w.name} tone="brand" square size="1.375rem" />
                    <span>{w.name}</span>
                    {w.workspace_id === workspace.workspace_id && (
                      <span className="menu-item-meta">
                        <Check aria-hidden="true" size="1rem" />
                        <span className="visually-hidden">Current</span>
                      </span>
                    )}
                  </MenuItem>
                ))}
              </MenuSection>
            </Menu>
          </Popover>
        </MenuTrigger>

        <nav className={adminMode ? 'nav is-admin' : 'nav'} aria-label="Main">
          <div className="nav-tools">
            <AriaButton className="search-btn" onPress={() => setPalette(true)} aria-keyshortcuts={shortcutLabel() === '⌘K' ? 'Meta+K' : 'Control+K'}>
              <Search aria-hidden="true" size="0.9375rem" />
              <span>Search</span>
              <kbd aria-hidden="true">{shortcutLabel()}</kbd>
            </AriaButton>
            <Link className="bell" to="/notifications" aria-current={current === 'inbox' ? 'page' : undefined} aria-label={unread ? `Notifications, ${unread} unread` : 'Notifications'}>
              <Bell aria-hidden="true" size="1rem" />
              {unread ? <span className="bell-count" aria-hidden="true">{unread > 99 ? '99+' : unread}</span> : null}
            </Link>
          </div>
          {adminMode ? (
            <>
              <Link className="nav-back" to="/"><ArrowLeft aria-hidden="true" size="1rem" /> Back to workspace</Link>
              <p className="nav-mode">Admin</p>
              {adminGroups.map((g) => (
                <div key={g.label}>
                  {!(g.items.length === 1 && g.items[0]!.label === g.label) && <p className="nav-group-label">{g.label}</p>}
                  <ul>{g.items.map(item)}</ul>
                </div>
              ))}
            </>
          ) : (
            <ul>{workNav(can).map(item)}</ul>
          )}
        </nav>

        <MenuTrigger>
          <AriaButton className="user-btn" aria-label={`Account: ${user.display_name}`}>
            <Avatar name={user.display_name} size="1.75rem" />
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
                } else if (key === 'security') navigate('/account/security');
                else if (key === 'profile') navigate('/account/profile');
                else if (key === 'activity') navigate('/admin/audit');
                else setTheme(String(key).replace('theme-', '') as Theme);
              }}
            >
              <MenuItem id="profile" className="menu-item" textValue="Profile">
                <UserRound aria-hidden="true" size="1rem" /> Profile
              </MenuItem>
              <MenuItem id="security" className="menu-item" textValue="Security">
                <ShieldCheck aria-hidden="true" size="1rem" /> Security
              </MenuItem>
              {can('audit:view_own') && !can('audit:view') && (
                <MenuItem id="activity" className="menu-item" textValue="My activity">
                  <History aria-hidden="true" size="1rem" /> My activity
                </MenuItem>
              )}
              <Separator className="menu-separator" />
              <MenuSection>
                <Header className="menu-header">Appearance</Header>
                {([['auto', 'System', Monitor], ['light', 'Light', Sun], ['dark', 'Dark', Moon]] as const).map(([k, label, Icon]) => (
                  <MenuItem key={k} id={`theme-${k}`} className="menu-item" textValue={`${label} theme`}>
                    <Icon aria-hidden="true" size="1rem" /> {label}
                    {theme === k && (
                      <span className="menu-item-meta"><Check aria-hidden="true" size="1rem" /><span className="visually-hidden">Selected</span></span>
                    )}
                  </MenuItem>
                ))}
              </MenuSection>
              <Separator className="menu-separator" />
              <MenuItem id="signout" className="menu-item" textValue="Sign out">
                <LogOut aria-hidden="true" size="1rem" /> Sign out
              </MenuItem>
            </Menu>
          </Popover>
        </MenuTrigger>
      </aside>

      <div className="panel">
        <header className="topbar">
          <ol className="crumbs" aria-label="Breadcrumb">
            {crumbs.map((c, i) => {
              const last = i === crumbs.length - 1;
              const label = typeof c === 'string' ? c : c[0];
              const text = last && crumbOverride ? crumbOverride : label;
              return <li key={label}>{typeof c !== 'string' && !last ? <Link to={c[1]}>{text}</Link> : <span aria-current={last ? 'page' : undefined}>{text}</span>}</li>;
            })}
          </ol>
          <div className="topbar-actions">{actions}</div>
        </header>
        {workspace.state === 'suspended_ro' && (
          <div className="ws-banner" role="status"><PauseCircle aria-hidden="true" size="1rem" /> <span><strong>This workspace is read-only.</strong> A system administrator paused changes. You can still read everything; nothing can be created, edited or acknowledged until it is reactivated.</span></div>
        )}
        <main id="main" className="page" tabIndex={-1}>{children}</main>
      </div>
      <CommandPalette isOpen={palette} onOpenChange={setPalette} />
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
          <img src="/procera-symbol.svg" alt="" width={28} height={28} />
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
