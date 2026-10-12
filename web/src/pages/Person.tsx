import { Mail, Pencil, Plus, UserRoundCheck, UserRoundMinus, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { type Member, useItems, useResource } from '../lib/data';
import { type Department, type Group, type MemberDirectory, deptPath, problemText } from '../lib/directory';
import { shortDate } from '../lib/format';
import { Link } from '../lib/router';
import { Avatar } from '../ui/Avatar';
import { setCrumb } from '../ui/AppShell';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import { AccountSecurity } from './personSecurity';
import { AddToGroup, EditProfile, Reactivate } from './personProfile';
import { LeaveDialog } from './personLeave';
import { OutOfOfficeSection } from './outOfOffice';
import '../ui/directory.css';

const roleLabel = (k: string) => k.replace('workspace.', '').replace(/[._]/g, ' ').replace(/^\w/, (c) => c.toUpperCase());
type Dialog = 'edit' | 'group' | 'deactivate' | 'reactivate' | null;

/** "(from CSV import)" next to a value the import set (the source is kept per field, BPMS-023). */
function Source({ dir, field }: { dir: MemberDirectory; field: string }) {
  return dir.field_sources?.[field] === 'csv' ? <span className="muted"> (from CSV import)</span> : null;
}

/**
 * A person in the workspace (`/members/{id}`): who they are and how to reach them. Linked from
 * manager escalations ("Ben has not acknowledged …") and joiner notices. Admins edit the profile,
 * groups and status here (docs/design/ia-competitive-analysis.md §8.3).
 */
export function Person({ id }: { id: string }) {
  const [members, reload, error] = useItems<Member>('/api/v1/members');
  const m = members?.find((x) => x.id === id) ?? null;
  const { can, user, membershipId } = useSession();
  const view = can('directory:view');
  const manage = can('directory:manage');
  const [dir, reloadDir] = useResource<MemberDirectory>(view ? `/api/v1/members/${id}/directory` : null, [members]);
  const [departments] = useItems<Department>(view ? '/api/v1/departments' : null);
  const [groups, reloadGroups] = useItems<Group>(view ? '/api/v1/groups' : null);
  const [roles] = useItems<{ id: string; key: string; name: string }>(can('role:view') ? '/api/v1/roles' : null);
  const dept = deptPath(dir?.department_id, departments ?? []);
  const manager = members?.find((x) => x.id === dir?.resolved_manager.membership_id);
  const myGroups = (dir?.group_ids ?? []).map((g) => groups?.find((x) => x.id === g)).filter((g): g is Group => !!g);
  const [dialog, setDialog] = useState<Dialog>(null);
  const [leaving, setLeaving] = useState<Group | null>(null);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string; details?: string[] } | null>(null);
  const refresh = () => { reload(); reloadDir(); reloadGroups(); };
  const finish = (title: string, tone: 'success' | 'danger' = 'success') => { setNotice({ tone, title }); setDialog(null); refresh(); };
  const leaveGroup = async (g: Group) => {
    setLeaving(null);
    try {
      await api(`/api/v1/groups/${g.id}/members/${id}`, { method: 'DELETE' });
      finish(`${m?.display_name} removed from ${g.name}.`);
    } catch (e) { setNotice({ tone: 'danger', title: problemText(e) }); }
  };
  useEffect(() => { if (m) setCrumb(m.display_name); return () => setCrumb(null); }, [m]);
  if (error) return <LoadError error={error} onRetry={reload} what="this person" />;
  if (members === null) return <div className="doc"><Skeleton variant="doc" label="Loading person" /></div>;
  if (!m) {
    return <Empty art="people" title="This person isn’t in the workspace" action={<Link className="btn btn-secondary btn-md" to="/">Go to Home</Link>}>They may have left, or the link is wrong.</Empty>;
  }
  const active = m.status === 'active';
  const roleOf = (k: string) => roles?.find((r) => r.key === k);
  const canEdit = manage && active && dir !== null;
  return (
    <div className="person">
      <div className="person-head">
        <Avatar name={m.display_name} size="3.5rem" />
        <div>
          <h1>{m.display_name}</h1>
          <p className="muted">{m.title ?? 'No title'}{!active && <> · <Tag color="gray">Deactivated</Tag></>}</p>
        </div>
      </div>
      {notice && <Notice tone={notice.tone} title={notice.title}>{notice.details?.length ? <ul style={{ margin: 0, paddingLeft: 'var(--space-5)' }}>{notice.details.map((d) => <li key={d}>{d}</li>)}</ul> : null}</Notice>}
      {(canEdit || (can('member:deactivate') && m.user_id !== user?.id)) && (
        <div className="actions" style={{ marginBottom: '1rem' }}>
          {canEdit && <Button variant="secondary" onPress={() => setDialog('edit')}><Pencil aria-hidden="true" size="0.9375rem" /> Edit profile</Button>}
          {can('member:deactivate') && m.user_id !== user?.id && (active
            ? <Button variant="secondary" onPress={() => setDialog('deactivate')}><UserRoundMinus aria-hidden="true" size="0.9375rem" /> Deactivate</Button>
            : <Button variant="secondary" onPress={() => setDialog('reactivate')}><UserRoundCheck aria-hidden="true" size="0.9375rem" /> Reactivate</Button>)}
        </div>
      )}
      <dl className="props">
        <dt><Mail aria-hidden="true" size="0.9375rem" /> Email</dt>
        <dd><a href={`mailto:${m.email}`}>{m.email}</a></dd>
        {dir && <>
          <dt>Department</dt><dd>{dept ?? <span className="muted">None</span>}<Source dir={dir} field="department" /></dd>
          <dt>Manager</dt><dd>{manager ? <Link to={`/members/${manager.id}`}>{manager.display_name}</Link> : <span className="muted">None</span>}{manager && dir.resolved_manager.via === 'department_head' && <span className="muted"> (department head)</span>}<Source dir={dir} field="manager" /></dd>
          <dt>Location</dt><dd>{dir.location ?? <span className="muted">None</span>}<Source dir={dir} field="location" /></dd>
          <dt>Groups</dt>
          <dd>
            {myGroups.length ? (
              <ul className="chip-list" aria-label="Groups">
                {myGroups.map((g) => (
                  <li key={g.id} className="chip-btn" style={{ cursor: 'default' }}>
                    {manage ? <Link to={`/admin/groups/${g.id}`}>{g.name}</Link> : g.name}
                    {manage && active && <button type="button" className="icon-btn" style={{ width: '1.5rem', height: '1.5rem' }} aria-label={`Remove from ${g.name}`} onClick={() => setLeaving(g)}><X aria-hidden="true" size="0.75rem" /></button>}
                  </li>
                ))}
              </ul>
            ) : <span className="muted">None</span>}
            {manage && active && <Button variant="ghost" className="btn-sm" onPress={() => setDialog('group')}><Plus aria-hidden="true" size="0.875rem" /> Add to group</Button>}
          </dd>
        </>}
        <dt>Roles</dt>
        <dd style={{ display: 'flex', gap: '0.25rem', flexWrap: 'wrap' }}>
          {m.roles.map((r) => { const role = roleOf(r); return role ? <Link key={r} to={`/admin/roles?role=${role.id}`} className="tag-link" aria-label={`Role: ${role.name}`}><Tag>{roleLabel(r)}</Tag></Link> : <Tag key={r}>{roleLabel(r)}</Tag>; })}
        </dd>
        <dt>Joined</dt>
        <dd>{shortDate(m.joined_at)}</dd>
      </dl>
      {can('member:security.manage') && active && <AccountSecurity membershipId={m.id} name={m.display_name} />}
      {can('work:delegate') && (can('work:reassign') || (!!membershipId && dir?.resolved_manager.membership_id === membershipId)) && active && m.user_id !== user?.id && <OutOfOfficeSection key={m.id} memberId={m.id} name={m.display_name} />}
      {dialog === 'edit' && dir && <EditProfile member={m} dir={dir} members={members} departments={departments ?? []} onClose={() => setDialog(null)} onSaved={() => finish('Profile saved.')} />}
      {dialog === 'group' && <AddToGroup member={m} groups={groups ?? []} current={dir?.group_ids ?? []} onClose={() => setDialog(null)} onAdded={(g) => finish(`${m.display_name} added to ${g.name}.`)} />}
      {dialog === 'reactivate' && <Reactivate member={m} roleNames={m.roles.map((r) => roleOf(r)?.name ?? roleLabel(r))} onClose={() => setDialog(null)} onDone={finish} />}
      <FormDialog isOpen={leaving !== null} onOpenChange={(o) => !o && setLeaving(null)} title={`Remove ${m.display_name} from ${leaving?.name ?? ''}?`}
        description="They stay in the workspace. Roles that come from this group stop applying to them."
        footer={<><Button variant="secondary" autoFocus onPress={() => setLeaving(null)}>Keep</Button><Button variant="danger" onPress={() => leaving && void leaveGroup(leaving)}>Remove</Button></>}>
        <p className="muted" style={{ margin: 0 }}>You can add them back at any time.</p>
      </FormDialog>
      {dialog === 'deactivate' && <LeaveDialog member={m} members={members} managerId={dir?.resolved_manager.membership_id ?? null} onClose={() => setDialog(null)}
        onDone={(r) => { setDialog(null); setNotice(r); refresh(); }} />}
    </div>
  );
}
