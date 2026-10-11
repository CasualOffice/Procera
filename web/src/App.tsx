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
import { Retention } from './pages/Retention';
import { Encryption } from './pages/Encryption';
import { FilesAdmin } from './pages/FilesAdmin';
import { SignIn } from './pages/SignIn';
import { Invite } from './pages/Invite';
import { AckLink } from './pages/AckLink';
import { Person } from './pages/Person';
import { SystemAudit } from './pages/system/SystemAudit';
import { SystemHealth } from './pages/system/SystemHealth';
import { SystemQuotas } from './pages/system/SystemQuotas';
import { SystemWorkspaces } from './pages/system/SystemWorkspaces';
import { SystemShell } from './ui/SystemShell';
import { Security } from './pages/Security';

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
  // The invitation link works signed in or out (it creates the account or joins with one click).
  if (path === '/invite') return <PlainShell><Invite /></PlainShell>;
  if (status === 'signed_out') {
    if (path !== '/sign-in') {
      const here = path + window.location.search;
      return <Redirect to={path === '/' ? '/sign-in' : `/sign-in?next=${encodeURIComponent(here)}`} />;
    }
    return <PlainShell><SignIn /></PlainShell>;
  }
  const next = query.get('next');
  // Back to the page that asked for sign-in (same-origin paths only).
  const safeNext = next && next.startsWith('/') && !next.startsWith('//') && !next.startsWith('/sign-in') ? next : null;
  if (path === '/sign-in' && safeNext && (isSystemPath(safeNext) || safeNext.startsWith('/invite?') || status === 'ready')) return <Redirect to={safeNext} />;
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
  const post = match('/announcements/:id', path);
  if (post) return <Redirect to={`/announcements?open=${post.id}`} />;
  const ack = match('/acknowledgements/:id', path);
  if (ack) return <AppShell current="home" crumbs={['Home']}><AckLink id={ack.id!} /></AppShell>;
  if (path === '/announcements') return <AppShell current="announcements" crumbs={['Announcements']}><Announcements /></AppShell>;
  if (path === '/account/security') return <AppShell current="none" crumbs={['Account', 'Security']}><Security /></AppShell>;
  if (path === '/admin/overview') return <AppShell current="overview" crumbs={[ADMIN, 'Overview']}><AdminOverview /></AppShell>;
  if (path === '/admin/files') return <AppShell current="files" crumbs={[ADMIN, 'Files']}><FilesAdmin /></AppShell>;
  if (path === '/admin/encryption') return <AppShell current="encryption" crumbs={[ADMIN, 'Encryption']}><Encryption /></AppShell>;
  if (path === '/admin/retention') return <AppShell current="retention" crumbs={[ADMIN, 'Retention']}><Retention /></AppShell>;
  if (path === '/admin/people') return <AppShell current="people" crumbs={[ADMIN, 'People & access']}><AdminPeople /></AppShell>;
  const person = match('/members/:id', path);
  if (person) return <AppShell current="home" crumbs={[['Members', '/admin/members'], 'Person']}><Person id={person.id!} /></AppShell>;
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
