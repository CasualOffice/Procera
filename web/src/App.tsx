import { useEffect } from 'react';
import { Compass } from 'lucide-react';
import { Link, match, navigate, useRoute } from './lib/router';
import { useSession } from './lib/session';
import { AppShell, type Crumb, PlainShell } from './ui/AppShell';
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
import { Templates } from './pages/Templates';
import { Announcements } from './pages/Announcements';
import { ManageAnnouncements } from './pages/ManageAnnouncements';
import { CampaignDetail, Campaigns } from './pages/Campaigns';
import { Requests } from './pages/Requests';
import { RequestNew } from './pages/RequestNew';
import { RequestDetail } from './pages/RequestDetail';
import { MyWork } from './pages/MyWork';
import { TeamPage } from './pages/Workload';
import { Settings } from './pages/Settings';
import { SettingsCategory } from './pages/SettingsCategory';
import { categoryById } from './pages/settingsMeta';
import { Audit } from './pages/Audit';
import { Members } from './pages/Members';
import { Departments } from './pages/Departments';
import { GroupDetail, Groups } from './pages/Groups';
import { Roles } from './pages/Roles';
import { AdminOverview } from './pages/AdminOverview';
import { AdminPeople } from './pages/AdminPeople';
import { Retention } from './pages/Retention';
import { AdminRequests } from './pages/AdminRequests';
import { Operations } from './pages/Operations';
import { Processes } from './pages/Processes';
import { Designer } from './pages/studio/Designer';
import { Encryption } from './pages/Encryption';
import { FilesAdmin } from './pages/FilesAdmin';
import { SignIn } from './pages/SignIn';
import { Invite } from './pages/Invite';
import { Setup } from './pages/Setup';
import { ForgotPassword } from './pages/ForgotPassword';
import { ResetPassword } from './pages/ResetPassword';
import { AckLink } from './pages/AckLink';
import { Person } from './pages/Person';
import { SystemAudit } from './pages/system/SystemAudit';
import { SystemEmail } from './pages/system/SystemEmail';
import { SystemFloors } from './pages/system/SystemFloors';
import { SystemHealth } from './pages/system/SystemHealth';
import { SystemQuotas } from './pages/system/SystemQuotas';
import { SystemSetup } from './pages/system/SystemSetup';
import { SystemWorkspaces } from './pages/system/SystemWorkspaces';
import { SystemShell } from './ui/SystemShell';
import { Profile } from './pages/Profile';
import { AdminHub } from './pages/AdminHub';
import { Welcome } from './pages/onboarding';
import { Security } from './pages/Security';
import { ApiTokens } from './pages/ApiTokens';
import { EmailApprove } from './pages/EmailApprove';
import { Webhooks } from './pages/Webhooks';
import { SingleSignOn } from './pages/SingleSignOn';
import { Holidays } from './pages/Holidays';
import { TimeOff } from './pages/TimeOff';
import { TeamCalendar } from './pages/TeamCalendar';
import { OrgChart } from './pages/OrgChart';
import { TimeOffAdmin } from './pages/TimeOffAdmin';
import { Signatures } from './pages/signing/Signatures';
import { BatchPage } from './pages/signing/BatchPage';
import { Ceremony } from './pages/signing/Ceremony';
import { DocumentNew } from './pages/signing/DocumentNew';
import { Documents } from './pages/signing/Documents';
import { EnvelopeEditor } from './pages/signing/EnvelopeEditor';
import { EnvelopePage } from './pages/signing/EnvelopePage';

function Redirect({ to }: { to: string }) {
  useEffect(() => navigate(to, { replace: true }), [to]);
  return null;
}

const ADMIN: Crumb = ['Admin', '/admin'];

/** The system console (docs/05 §2.3) lives at `/system`, outside any workspace. */
const isSystemPath = (p: string) => p === '/system' || p.startsWith('/system/');

function SystemRoutes({ path }: { path: string }) {
  if (path === '/system' || path === '/system/') return <Redirect to="/system/workspaces" />;
  if (path === '/system/workspaces') return <SystemShell current="workspaces" title="Workspaces"><SystemWorkspaces /></SystemShell>;
  if (path === '/system/quotas') return <SystemShell current="quotas" title="Quotas & usage"><SystemQuotas /></SystemShell>;
  if (path === '/system/floors') return <SystemShell current="floors" title="Policy floors"><SystemFloors /></SystemShell>;
  if (path === '/system/health') return <SystemShell current="health" title="Platform health"><SystemHealth /></SystemShell>;
  if (path === '/system/email') return <SystemShell current="email" title="Email & platform"><SystemEmail /></SystemShell>;
  if (path === '/system/setup') return <SystemShell current="setup" title="Finish setting up"><SystemSetup /></SystemShell>;
  if (path === '/system/audit') return <SystemShell current="audit" title="System audit"><SystemAudit /></SystemShell>;
  return <Redirect to="/system/workspaces" />;
}

export function App() {
  const { path, query } = useRoute();
  const { status } = useSession();

  // Public pages.
  if (path === '/design') return <AppShellPublic><DesignGallery /></AppShellPublic>;
  if (status === 'loading') return <PlainShell art={false}><Skeleton variant="doc" label="Loading Procera" /></PlainShell>;
  // The invitation link works signed in or out (it creates the account or joins with one click).
  if (path === '/invite') return <PlainShell><Invite /></PlainShell>;
  const emailAction = match('/approve/:token', path);
  if (emailAction) return <PlainShell art={false}><EmailApprove token={emailAction.token!} /></PlainShell>;
  // First-run setup (FR-S01): the one-time bootstrap token creates the first system owner.
  if (path === '/setup') return <PlainShell><Setup /></PlainShell>;
  // Account recovery (docs/06 §3.6.1): reachable signed out; a reset link also works signed in.
  if (path === '/forgot-password') return <PlainShell><ForgotPassword /></PlainShell>;
  if (path === '/reset-password') return <PlainShell><ResetPassword /></PlainShell>;
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
  if (path === '/library') return query.get('view') === 'mine' ? <AppShell current="my-documents" crumbs={[['Library', '/library'], 'My documents']}><Library /></AppShell> : <AppShell current="library" crumbs={['Library']}><Library /></AppShell>;
  if (path === '/requests') {
    const v = query.get('view');
    return <AppShell current="requests" crumbs={v === 'mine' ? [['Requests', '/requests'], 'My requests'] : v === 'all' ? [['Requests', '/requests'], 'All requests'] : ['Requests']}><Requests view={v === 'mine' ? 'mine' : v === 'all' ? 'all' : 'new'} /></AppShell>;
  }
  const reqNew = match('/requests/new/:type', path);
  if (reqNew) return <AppShell current="requests" crumbs={[['Requests', '/requests'], 'New request']}><RequestNew typeKey={reqNew.type!} /></AppShell>;
  const req = match('/requests/:id', path);
  if (req) return <AppShell current="requests" crumbs={[['Requests', '/requests?view=mine'], 'Request']}><RequestDetail id={req.id!} /></AppShell>;
  if (path === '/team') return <AppShell current="team" crumbs={['Team']}><TeamPage /></AppShell>;
  if (path === '/work') return <AppShell current="work" crumbs={['My work']}><MyWork selected={null} /></AppShell>;
  if (path === '/work/signatures') return <AppShell current="work" crumbs={[['My work', '/work'], 'Signatures']}><Signatures /></AppShell>;
  const signing = match('/sign/:id', path);
  if (signing) return <AppShell current="work" crumbs={[['My work', '/work/signatures'], 'Sign']}><Ceremony recipient={signing.id!} /></AppShell>;
  const workItem = match('/work/:id', path);
  if (workItem) return <AppShell current="work" crumbs={[['My work', '/work'], 'Item']}><MyWork selected={workItem.id!} /></AppShell>;
  if (path === '/welcome') return <AppShell current="home" crumbs={[['Home', '/'], 'Set up']}><Welcome /></AppShell>;
  if (path === '/admin') return <AppShell current="admin" crumbs={['Admin']}><AdminHub /></AppShell>;
  if (path === '/templates') return <AppShell current="templates" crumbs={['Templates']}><Templates /></AppShell>;
  if (path === '/policies') return <AppShell current="library" crumbs={[['Library', '/library'], 'Applies to me']}><Policies /></AppShell>;
  const cmp = match('/library/:id/compare', path);
  if (cmp) return <AppShell current="library" crumbs={[['Library', '/library'], 'Compare']}><Compare id={cmp.id!} /></AppShell>;
  const doc = match('/library/:id', path);
  if (doc) return <AppShell current="library" crumbs={[['Library', '/library'], 'Document']}><LibraryDoc id={doc.id!} /></AppShell>;
  const post = match('/announcements/:id', path);
  if (post) return <Redirect to={`/announcements?open=${post.id}`} />;
  const ack = match('/acknowledgements/:id', path);
  if (ack) return <AppShell current="home" crumbs={['Home']}><AckLink id={ack.id!} /></AppShell>;
  if (path === '/announcements') return <AppShell current="announcements" crumbs={['Announcements']}><Announcements /></AppShell>;
  if (path === '/documents') return <AppShell current="documents" crumbs={['Signing']}><Documents /></AppShell>;
  const batchPage = match('/documents/batches/:id', path);
  if (batchPage) return <AppShell current="documents" crumbs={[['Signing', '/documents'], 'Group send']}><BatchPage key={batchPage.id} id={batchPage.id!} /></AppShell>;
  if (path === '/documents/new') return <AppShell current="documents" crumbs={[['Signing', '/documents'], 'Send for signature']}><DocumentNew /></AppShell>;
  const envEdit = match('/documents/:id/edit', path);
  if (envEdit) return <AppShell current="documents" crumbs={[['Signing', '/documents'], 'Draft']}><EnvelopeEditor key={envEdit.id} id={envEdit.id!} /></AppShell>;
  const envelope = match('/documents/:id', path);
  if (envelope) return <AppShell current="documents" crumbs={[['Signing', '/documents'], 'Document']}><EnvelopePage key={envelope.id} id={envelope.id!} /></AppShell>;
  if (path === '/account/profile') return <AppShell current="none" crumbs={['Account', 'Profile']}><Profile /></AppShell>;
  if (path === '/account/security') return <AppShell current="none" crumbs={['Account', 'Security']}><Security /></AppShell>;
  if (path === '/account/api-tokens') return <AppShell current="none" crumbs={['Account', 'API tokens']}><ApiTokens /></AppShell>;
  if (path === '/admin/processes') return <AppShell current="processes" crumbs={[ADMIN, 'Processes']}><Processes startNew={query.get('new') === '1'} /></AppShell>;
  const proc = match('/admin/processes/:id', path);
  if (proc) return <AppShell current="processes" crumbs={[ADMIN, ['Processes', '/admin/processes'], 'Process']}><Designer key={proc.id} id={proc.id!} /></AppShell>;
  if (path === '/admin/operations') return <AppShell current="operations" crumbs={[ADMIN, 'Operations']}><Operations /></AppShell>;
  if (path === '/admin/requests') return <AppShell current="request-oversight" crumbs={[ADMIN, 'Requests oversight']}><AdminRequests /></AppShell>;
  if (path === '/admin/overview') return <AppShell current="overview" crumbs={[ADMIN, 'Overview']}><AdminOverview /></AppShell>;
  if (path === '/admin/files') return <AppShell current="files" crumbs={[ADMIN, 'Files']}><FilesAdmin /></AppShell>;
  if (path === '/admin/encryption') return <AppShell current="encryption" crumbs={[ADMIN, 'Encryption']}><Encryption /></AppShell>;
  if (path === '/time-off/calendar') return <AppShell current="time-off" crumbs={[['Time off', '/time-off'], 'Team calendar']}><TeamCalendar /></AppShell>;
  if (path === '/org-chart') return <AppShell current="time-off" crumbs={[['Time off', '/time-off'], 'Org chart']}><OrgChart /></AppShell>;
  if (path === '/time-off') return <AppShell current="time-off" crumbs={['Time off']}><TimeOff /></AppShell>;
  if (path === '/admin/time-off') return <AppShell current="time-off-admin" crumbs={[ADMIN, 'Time off']}><TimeOffAdmin /></AppShell>;
  if (path === '/admin/holidays') return <AppShell current="holidays" crumbs={[ADMIN, 'Holidays and locations']}><Holidays /></AppShell>;
  if (path === '/admin/sso') return <AppShell current="sso" crumbs={[ADMIN, 'Single sign-on']}><SingleSignOn /></AppShell>;
  if (path === '/admin/webhooks') return <AppShell current="webhooks" crumbs={[ADMIN, 'Webhooks']}><Webhooks /></AppShell>;
  if (path === '/admin/retention') return <AppShell current="retention" crumbs={[ADMIN, 'Retention']}><Retention /></AppShell>;
  if (path === '/admin/people') return <AppShell current="people" crumbs={[ADMIN, 'People & access']}><AdminPeople /></AppShell>;
  const person = match('/members/:id', path);
  if (person) return <AppShell current="members" crumbs={[ADMIN, ['Members', '/admin/members'], 'Person']}><Person id={person.id!} /></AppShell>;
  if (path === '/admin/members' || path === '/members') return <AppShell current="members" crumbs={[ADMIN, 'Members']}><Members /></AppShell>;
  if (path === '/admin/departments') return <AppShell current="departments" crumbs={[ADMIN, 'Departments']}><Departments /></AppShell>;
  if (path === '/admin/groups') return <AppShell current="groups" crumbs={[ADMIN, 'Groups']}><Groups /></AppShell>;
  const grp = match('/admin/groups/:id', path);
  if (grp) return <AppShell current="groups" crumbs={[ADMIN, ['Groups', '/admin/groups'], 'Group']}><GroupDetail id={grp.id!} created={query.get('created') === '1'} /></AppShell>;
  if (path === '/admin/roles') return <AppShell current="roles" crumbs={[ADMIN, 'Roles & access']}><Roles /></AppShell>;
  if (path === '/admin/announcements') return <AppShell current="announcements" crumbs={[['Announcements', '/announcements'], 'Manage']}><ManageAnnouncements /></AppShell>;
  if (path === '/admin/campaigns') return <AppShell current="campaigns" crumbs={['Campaigns']}><Campaigns /></AppShell>;
  const camp = match('/admin/campaigns/:id', path);
  if (camp) return <AppShell current="campaigns" crumbs={[['Campaigns', '/admin/campaigns'], 'Campaign']}><CampaignDetail id={camp.id!} /></AppShell>;
  if (path === '/admin/settings') return <AppShell current="settings" crumbs={[ADMIN, 'Settings']}><Settings /></AppShell>;
  const setCat = match('/admin/settings/:category', path);
  if (setCat && categoryById(setCat.category!)) return <AppShell current="settings" crumbs={[ADMIN, ['Settings', '/admin/settings'], categoryById(setCat.category!)!.label]}><SettingsCategory key={setCat.category} id={setCat.category!} /></AppShell>;
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
      <header className="gallery-head"><img src="/procera-symbol.svg" alt="" width={24} height={24} /><span>Procera</span></header>
      <main id="main" className="gallery-main" tabIndex={-1}>{children}</main>
    </div>
  );
}
