import { Activity, ArrowLeftRight, Building2, Gauge, History, LogOut, Mail, ShieldAlert } from 'lucide-react';
import { createContext, useContext, useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { type SystemMe, sysCan } from '../lib/system';
import { MfaSetup } from '../pages/MfaSetup';
import { Button } from './components/Button';
import { Notice } from './components/Notice';
import { Skeleton } from './States';
import './shell.css';
import './system.css';

export type SystemNavKey = 'workspaces' | 'quotas' | 'health' | 'email' | 'audit' | 'setup';

const NAV: { key: SystemNavKey; label: string; to: string; icon: typeof Building2; audit?: boolean; platform?: boolean }[] = [
  { key: 'workspaces', label: 'Workspaces', to: '/system/workspaces', icon: Building2 },
  { key: 'quotas', label: 'Quotas & usage', to: '/system/quotas', icon: Gauge },
  { key: 'health', label: 'Platform health', to: '/system/health', icon: Activity },
  { key: 'email', label: 'Email & platform', to: '/system/email', icon: Mail, platform: true },
  { key: 'audit', label: 'System audit', to: '/system/audit', icon: History, audit: true },
];

const SystemCtx = createContext<SystemMe | null>(null);

/** The signed-in system user's roles (inside the system console only). */
export function useSystem(): SystemMe {
  const me = useContext(SystemCtx);
  if (!me) throw new Error('useSystem outside SystemShell');
  return me;
}

/**
 * System Admin console shell (docs/05 §2.3): a separate realm with dark chrome and a "System" label.
 * It has no navigation into workspace content. Only system users get past the gate; everyone else
 * sees "not found", as the API answers them (the system plane is invisible to them).
 */
export function SystemShell({ current, title, children }: { current: SystemNavKey; title: string; children: React.ReactNode }) {
  const { user, workspaces, signOut } = useSession();
  const [me, setMe] = useState<SystemMe | null>(null);
  const [gate, setGate] = useState<'loading' | 'ok' | 'hidden' | 'mfa' | 'error'>('loading');
  useEffect(() => {
    api<SystemMe>('/sysapi/v1/me')
      .then((m) => { setMe(m); setGate('ok'); })
      .catch((e: unknown) => setGate(e instanceof ApiError && e.status === 404 ? 'hidden' : e instanceof ApiError && e.code === 'mfa_required' ? 'mfa' : 'error'));
  }, []);
  useEffect(() => { document.title = `${title} · System · Procera`; }, [title]);

  // A system user without two-step verification enrols right here (docs/06 §3.6), then signs in again.
  if (gate === 'mfa' && !user?.mfa_enabled) {
    return (
      <div className="auth">
        <main id="main" className="auth-card" tabIndex={-1}>
          <MfaSetup next={window.location.pathname} intro="System administration needs two-step verification. Turn it on now with an authenticator app; it takes about a minute." />
        </main>
      </div>
    );
  }
  if (gate !== 'ok' || !me) {
    return (
      <div className="auth">
        <main id="main" className="auth-card sys-gate" tabIndex={-1}>
          {gate === 'loading' ? <Skeleton label="Checking system access" /> : (
            <>
              <h1>{gate === 'hidden' ? 'Page not found' : gate === 'mfa' ? 'Two-step verification required' : 'Couldn’t open the system console'}</h1>
              {gate === 'mfa'
                ? <Notice tone="warning" title="System administration needs two-step verification">It’s on for your account. Sign in again to use it.</Notice>
                : <p className="lede">{gate === 'hidden' ? 'This page doesn’t exist, or you don’t have access to it.' : 'Try again in a moment.'}</p>}
              <div className="actions">
                {gate === 'mfa' && <Button onPress={async () => { await signOut(); navigate(`/sign-in?next=${encodeURIComponent(window.location.pathname)}`, { replace: true }); }}>Sign in again</Button>}
                <Link className="btn btn-secondary btn-md link-btn" to="/">Go to Procera</Link>
              </div>
            </>
          )}
        </main>
      </div>
    );
  }

  const nav = NAV.filter((n) => (!n.audit || sysCan(me.roles, 'audit.read')) && (!n.platform || sysCan(me.roles, 'platform.read')));
  return (
    <SystemCtx.Provider value={me}>
      <div className="app sys-app">
        <a className="skip-link" href="#main">Skip to main content</a>
        <aside className="sidebar sys-sidebar">
          <div className="sys-brand">
            <img src="/procera-symbol.svg" alt="" width={22} height={22} />
            <span>Procera</span>
            <span className="sys-label">System</span>
          </div>
          <nav className="nav" aria-label="System">
            <ul>
              {nav.map((n) => (
                <li key={n.key}>
                  <Link to={n.to} aria-current={n.key === current ? 'page' : undefined}>
                    <n.icon aria-hidden="true" size="1rem" />
                    <span>{n.label}</span>
                  </Link>
                </li>
              ))}
            </ul>
          </nav>
          <div className="sys-foot">
            <p className="sys-who"><span>{user?.display_name}</span><span>{me.roles.map((r) => r.replace('system.', '')).join(', ')}</span></p>
            {workspaces.length > 0 && <Link className="sys-foot-link" to="/"><ArrowLeftRight aria-hidden="true" size="0.875rem" /> Your workspaces</Link>}
            <Button variant="ghost" className="sys-signout" onPress={async () => { await signOut(); navigate('/sign-in?next=/system', { replace: true }); }}>
              <LogOut aria-hidden="true" size="0.875rem" /> Sign out
            </Button>
          </div>
        </aside>
        <div className="panel">
          <header className="topbar">
            <ol className="crumbs" aria-label="Breadcrumb"><li><span>System</span></li><li><span aria-current="page">{title}</span></li></ol>
            <span className="sys-note"><ShieldAlert aria-hidden="true" size="0.875rem" /> Metadata only. Workspace content isn’t visible here.</span>
          </header>
          <main id="main" className="page" tabIndex={-1}>{children}</main>
        </div>
      </div>
    </SystemCtx.Provider>
  );
}
