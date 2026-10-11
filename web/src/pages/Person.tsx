import { Mail, UserRoundMinus, UserRoundX } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { type Member, useItems, useResource } from '../lib/data';
import { shortDate } from '../lib/format';
import { Link } from '../lib/router';
import { Avatar } from '../ui/Avatar';
import { setCrumb } from '../ui/AppShell';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import { AccountSecurity } from './personSecurity';

const roleLabel = (k: string) => k.replace('workspace.', '').replace(/[._]/g, ' ').replace(/^\w/, (c) => c.toUpperCase());

/**
 * A person in the workspace (`/members/{id}`): who they are and how to reach them. Linked from
 * manager escalations ("Ben has not acknowledged …") and joiner notices.
 */
export function Person({ id }: { id: string }) {
  const [members, reload, error] = useItems<Member>('/api/v1/members');
  const m = members?.find((x) => x.id === id) ?? null;
  const { can, user } = useSession();
  const [dir] = useResource<{ title: string | null; department_id: string | null; location: string | null; resolved_manager: { membership_id: string | null; via: string } }>(can('directory:view') ? `/api/v1/members/${id}/directory` : null, [members]);
  const [departments] = useItems<{ id: string; name: string }>(can('directory:view') ? '/api/v1/departments' : null);
  const dept = departments?.find((d) => d.id === dir?.department_id)?.name;
  const manager = members?.find((x) => x.id === dir?.resolved_manager.membership_id);
  const [confirming, setConfirming] = useState(false);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(null);
  const deactivate = async () => {
    try {
      await api(`/api/v1/members/${id}/deactivate`, { method: 'POST' });
      setNotice({ tone: 'success', title: `${m?.display_name ?? 'They'} can no longer sign in to this workspace.` });
      reload();
    } catch (e) {
      setNotice({ tone: 'danger', title: e instanceof ApiError ? (e.code === 'last_owner' ? 'This is the only owner. Make someone else an owner first.' : e.problem.detail ?? e.problem.title) : 'That didn’t work. Try again.' });
    } finally { setConfirming(false); }
  };
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
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      {can('member:deactivate') && m.status === 'active' && m.user_id !== user?.id && (
        <div className="actions" style={{ marginBottom: '1rem' }}>
          <Button variant="secondary" onPress={() => setConfirming(true)}><UserRoundMinus aria-hidden="true" size="0.9375rem" /> Deactivate</Button>
        </div>
      )}
      <dl className="props">
        <dt><Mail aria-hidden="true" size="0.9375rem" /> Email</dt>
        <dd><a href={`mailto:${m.email}`}>{m.email}</a></dd>
        {dir && <>
          <dt>Department</dt><dd>{dept ?? <span className="muted">None</span>}</dd>
          <dt>Manager</dt><dd>{manager ? <Link to={`/members/${manager.id}`}>{manager.display_name}</Link> : <span className="muted">None</span>}{manager && dir.resolved_manager.via === 'department_head' && <span className="muted"> (department head)</span>}</dd>
          <dt>Location</dt><dd>{dir.location ?? <span className="muted">None</span>}</dd>
        </>}
        <dt>Roles</dt>
        <dd style={{ display: 'flex', gap: '0.25rem', flexWrap: 'wrap' }}>{m.roles.map((r) => <Tag key={r}>{roleLabel(r)}</Tag>)}</dd>
        <dt>Joined</dt>
        <dd>{shortDate(m.joined_at)}</dd>
      </dl>
      {can('member:security.manage') && m.status === 'active' && <AccountSecurity membershipId={m.id} name={m.display_name} />}
      <FormDialog isOpen={confirming} onOpenChange={setConfirming} title={`Deactivate ${m.display_name}?`}
        description="They are signed out of this workspace at once and can’t act in it. Their history and records stay. To bring them back, invite them again."
        footer={<><Button variant="secondary" onPress={() => setConfirming(false)}>Keep</Button><Button variant="danger" onPress={() => void deactivate()}>Deactivate</Button></>}>
        <p className="muted" style={{ margin: 0 }}>This is recorded in the audit log.</p>
      </FormDialog>
    </div>
  );
}
