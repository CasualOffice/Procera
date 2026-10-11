import { useEffect } from 'react';
import { match, navigate, useRoute } from './lib/router';
import { useSession } from './lib/session';
import { AppShell, PlainShell } from './ui/AppShell';
import { ChooseWorkspace } from './pages/ChooseWorkspace';
import { DesignGallery } from './pages/DesignGallery';
import { Home } from './pages/Home';
import { Notifications } from './pages/Notifications';
import { SignIn } from './pages/SignIn';

function Redirect({ to }: { to: string }) {
  useEffect(() => navigate(to, { replace: true }), [to]);
  return null;
}

export function App() {
  const { path } = useRoute();
  const { status } = useSession();

  // Public pages.
  if (path === '/design') return <AppShellPublic><DesignGallery /></AppShellPublic>;
  if (status === 'loading') return <PlainShell><p className="muted" role="status">Loading…</p></PlainShell>;
  if (status === 'signed_out') {
    if (path !== '/sign-in') return <Redirect to={`/sign-in`} />;
    return <PlainShell><SignIn /></PlainShell>;
  }
  if (status === 'choose_workspace') return <PlainShell><ChooseWorkspace /></PlainShell>;
  if (path === '/sign-in') return <Redirect to="/" />;

  if (path === '/') return <AppShell current="home"><Home /></AppShell>;
  if (path === '/notifications') return <AppShell current="none"><Notifications /></AppShell>;
  if (match('/x/:never', path)) return null;
  return (
    <AppShell current="none">
      <h1>Page not found</h1>
      <p className="lede">This page doesn’t exist or you don’t have access to it.</p>
    </AppShell>
  );
}

/** The design gallery keeps the static shell look without a session. */
function AppShellPublic({ children }: { children: React.ReactNode }) {
  return <PlainShellWide>{children}</PlainShellWide>;
}

function PlainShellWide({ children }: { children: React.ReactNode }) {
  return (
    <>
      <a className="skip-link" href="#main">Skip to main content</a>
      <header className="shell-header">
        <span className="shell-brand"><img src="/procera-symbol.png" alt="" width={24} height={24} /><span>Procera</span></span>
      </header>
      <main id="main" className="shell-main" tabIndex={-1}>{children}</main>
    </>
  );
}
