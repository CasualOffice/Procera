import { SectionTabs } from '../ui/SectionTabs';
import { useSession } from '../lib/session';

/** Library views (docs/05 §2.1): everything I can see, what applies to me, what I own. */
export function LibraryTabs({ current }: { current: 'all' | 'applies' | 'mine' }) {
  const { can } = useSession();
  return (
    <SectionTabs label="Library views" tabs={[
      (can('policy:view') || can('sop:view')) && { label: 'All documents', to: '/library', current: current === 'all' },
      can('policy:view') && { label: 'Applies to me', to: '/policies', current: current === 'applies' },
      (can('policy:create') || can('sop:create')) && { label: 'My documents', to: '/library?view=mine', current: current === 'mine' },
    ]} />
  );
}

/** Announcements views: the feed for everyone, Manage for people who write or approve them. */
export function AnnouncementTabs({ current }: { current: 'feed' | 'manage' }) {
  const { can } = useSession();
  return (
    <SectionTabs label="Announcement views" tabs={[
      can('announcement:view') && { label: 'Feed', to: '/announcements', current: current === 'feed' },
      (can('announcement:create') || can('announcement:publish') || can('announcement:manage')) && { label: 'Manage', to: '/admin/announcements', current: current === 'manage' },
    ]} />
  );
}
