import { useEffect } from 'react';
import { Compass } from 'lucide-react';
import { Link, match, navigate, useRoute } from './lib/router';
import { useSession } from './lib/session';
import { AppShell, PlainShell } from './ui/AppShell';
import { Empty, Skeleton } from './ui/States';
import { ChooseWorkspace } from './pages/ChooseWorkspace';
import { DesignGallery } from './pages/DesignGallery';
import { Home } from './pages/Home';
import { Notifications } from './pages/Notifications';
import { NotificationPreferences } from './pages/NotificationPreferences';
import { Library } from './pages/Library';
import { LibraryDoc } from './pages/LibraryDoc';
import { Compare } from './pages/Compare';
import { Policies } from './pages/Policies';
import { Announcements } from './pages/Announcements';
import { ManageAnnouncements } from './pages/ManageAnnouncements';
import { CampaignDetail, Campaigns } from './pages/Campaigns';
import { Settings } from './pages/Settings';
import { Audit } from './pages/Audit';
import { Members } from './pages/Members';
import { AdminOverview } from './pages/AdminOverview';
import { AdminPeople } from './pages/AdminPeople';
import { SignIn } from './pages/SignIn';
import { SystemAudit } from './pages/system/SystemAudit';
import { SystemHealth } from './pages/system/SystemHealth';
import { SystemQuotas } from './pages/system/SystemQuotas';
import { SystemWorkspaces } from './pages/system/SystemWorkspaces';
import { SystemShell } from './ui/SystemShell';

function Redirect({ to }: { to: string }) {
  useEffect(() => navigate(to, { replace: true }), [to]);
  return null;
}

const ADMIN = 'Administration';

/** The system console (docs/05 §2.3) lives at `/system`, outside any workspace. */
const isSystemPath = (p: string) => p === '/system' || p.startsWith('/system/');

function SystemRoutes({ path }: { path: string }) {
  if (path === '/system' || path === '/system/') return <Redirect to="/system/workspaces" />;
  if (path === '/system/workspaces') return <SystemShell current="workspaces" title="Workspaces"><SystemWorkspaces /></SystemShell>;
  if (path === '/system/quotas') return <SystemShell current="quotas" title="Quotas & usage"><SystemQuotas /></SystemShell>;
  if (path === '/system/health') return <SystemShell current="health" title="Platform health"><SystemHealth /></SystemShell>;
  if (path === '/system/audit') return <SystemShell current="audit" title="System audit"><SystemAudit /></SystemShell>;
  return <Redirect to="/system/workspaces" />;
}

export function App() {
  const { path, query } = useRoute();
  const { status } = useSession();

  // Public pages.
  if (path === '/design') return <AppShellPublic><DesignGallery /></AppShellPublic>;
  if (status === 'loading') return <PlainShell><Skeleton variant="doc" label="Loading Procera" /></PlainShell>;
  if (status === 'signed_out') {
    if (path !== '/sign-in') return <Redirect to={isSystemPath(path) ? `/sign-in?next=${encodeURIComponent(path)}` : '/sign-in'} />;
    return <PlainShell><SignIn /></PlainShell>;
  }
  const next = query.get('next');
  if (path === '/sign-in' && next && isSystemPath(next)) return <Redirect to={next} />;
  if (isSystemPath(path)) return <SystemRoutes path={path} />;
  if (status === 'choose_workspace') return <PlainShell><ChooseWorkspace /></PlainShell>;
  if (path === '/sign-in') return <Redirect to="/" />;

  if (path === '/') return <AppShell current="home" crumbs={['Home']}><Home /></AppShell>;
  if (path === '/notifications/preferences') return <AppShell current="inbox" crumbs={[['Notifications', '/notifications'], 'Preferences']}><NotificationPreferences /></AppShell>;
  if (path === '/notifications') return <AppShell current="inbox" crumbs={['Notifications']}><Notifications /></AppShell>;
  if (path === '/library') return <AppShell current="library" crumbs={['Library']}><Library /></AppShell>;
  if (path === '/policies') return <AppShell current="policies" crumbs={['Policies']}><Policies /></AppShell>;
  const cmp = match('/library/:id/compare', path);
  if (cmp) return <AppShell current="library" crumbs={[['Library', '/library'], 'Compare']}><Compare id={cmp.id!} /></AppShell>;
  const doc = match('/library/:id', path);
  if (doc) return <AppShell current="library" crumbs={[['Library', '/library'], 'Document']}><LibraryDoc id={doc.id!} /></AppShell>;
  if (path === '/announcements') return <AppShell current="announcements" crumbs={['Announcements']}><Announcements /></AppShell>;
  if (path === '/admin/overview') return <AppShell current="overview" crumbs={[ADMIN, 'Overview']}><AdminOverview /></AppShell>;
  if (path === '/admin/people') return <AppShell current="people" crumbs={[ADMIN, 'People & access']}><AdminPeople /></AppShell>;
  if (path === '/admin/members' || path === '/members') return <AppShell current="members" crumbs={[ADMIN, 'Members']}><Members /></AppShell>;
  if (path === '/admin/announcements') return <AppShell current="manage-announcements" crumbs={[ADMIN, 'Manage announcements']}><ManageAnnouncements /></AppShell>;
  if (path === '/admin/campaigns') return <AppShell current="campaigns" crumbs={[ADMIN, 'Campaigns']}><Campaigns /></AppShell>;
  const camp = match('/admin/campaigns/:id', path);
  if (camp) return <AppShell current="campaigns" crumbs={[ADMIN, ['Campaigns', '/admin/campaigns'], 'Campaign']}><CampaignDetail id={camp.id!} /></AppShell>;
  if (path === '/admin/settings') return <AppShell current="settings" crumbs={[ADMIN, 'Settings']}><Settings /></AppShell>;
  if (path === '/admin/audit') return <AppShell current="audit" crumbs={[ADMIN, 'Audit log']}><Audit /></AppShell>;
  return (
    <AppShell current="none" crumbs={['Not found']}>
      <div className="page-head"><div><h1>Page not found</h1><p>This page doesn’t exist, or you don’t have access to it.</p></div></div>
      <Empty icon={Compass} title="Nothing here" action={<Link className="btn btn-secondary link-btn" to="/">Back to Home</Link>}>Check the address, or press {navigator.platform.includes('Mac') ? '⌘K' : 'Ctrl K'} to search.</Empty>
    </AppShell>
  );
}

/** The design gallery keeps the static shell look without a session. */
function AppShellPublic({ children }: { children: React.ReactNode }) {
  return <PlainShellWide>{children}</PlainShellWide>;
}

function PlainShellWide({ children }: { children: React.ReactNode }) {
  return (
    <div className="gallery">
      <a className="skip-link" href="#main">Skip to main content</a>
      <header className="gallery-head"><img src="/procera-symbol.png" alt="" width={24} height={24} /><span>Procera</span></header>
      <main id="main" className="gallery-main" tabIndex={-1}>{children}</main>
    </div>
  );
}
