import { Mail, UserRoundX } from 'lucide-react';
import { useEffect } from 'react';
import { type Member, useItems } from '../lib/data';
import { shortDate } from '../lib/format';
import { Link } from '../lib/router';
import { Avatar } from '../ui/Avatar';
import { setCrumb } from '../ui/AppShell';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';

const roleLabel = (k: string) => k.replace('workspace.', '').replace(/[._]/g, ' ').replace(/^\w/, (c) => c.toUpperCase());

/**
 * A person in the workspace (`/members/{id}`): who they are and how to reach them. Linked from
 * manager escalations ("Ben has not acknowledged …") and joiner notices.
 */
export function Person({ id }: { id: string }) {
  const [members, reload, error] = useItems<Member>('/api/v1/members');
  const m = members?.find((x) => x.id === id) ?? null;
  useEffect(() => { if (m) setCrumb(m.display_name); return () => setCrumb(null); }, [m]);
  if (error) return <LoadError error={error} onRetry={reload} what="this person" />;
  if (members === null) return <div className="doc"><Skeleton variant="doc" label="Loading person" /></div>;
  if (!m) {
    return <Empty icon={UserRoundX} title="This person isn’t in the workspace" action={<Link className="btn btn-secondary btn-md" to="/">Go to Home</Link>}>They may have left, or the link is wrong.</Empty>;
  }
  return (
    <div className="person">
      <div className="person-head">
        <Avatar name={m.display_name} size="3.5rem" />
        <div>
          <h1>{m.display_name}</h1>
          <p className="muted">{m.title ?? 'No title'}{m.status !== 'active' && <> · <Tag color="gray">Deactivated</Tag></>}</p>
        </div>
      </div>
      <dl className="props">
        <dt><Mail aria-hidden="true" size="0.9375rem" /> Email</dt>
        <dd><a href={`mailto:${m.email}`}>{m.email}</a></dd>
        <dt>Roles</dt>
        <dd style={{ display: 'flex', gap: '0.25rem', flexWrap: 'wrap' }}>{m.roles.map((r) => <Tag key={r}>{roleLabel(r)}</Tag>)}</dd>
        <dt>Joined</dt>
        <dd>{shortDate(m.joined_at)}</dd>
      </dl>
    </div>
  );
}
