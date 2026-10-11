import { FileUp, MailPlus, Network, UsersRound } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { shortDate } from '../lib/format';
import { Link, navigate, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Tag, type TagColor } from '../ui/Tag';
import { LoadError, Skeleton } from '../ui/States';
import { DataGrid } from '../ui/components/DataGrid';
import { DirectoryImport } from './DirectoryImport';
import { PeopleTabs } from '../ui/PeopleTabs';
import type { Department, Group } from '../lib/directory';
import { type BulkRow, BulkAddToGroup, BulkSetDepartment } from './membersBulk';

/** Specialist roles add to the employee basics, so invites pair them with Member (narrow custom roles stay possible through the API). */
const BASE_ROLES = new Set(['member', 'workspace.owner', 'workspace.admin']);

interface PersonRow { id: string; name: string; email: string; roles: string[]; title: string | null; status: string; date: string; invite: boolean }
interface Invitation { id: string; email: string; role_keys: string[]; expires_at: string }
const ROLE_COLOR: Record<string, TagColor> = { 'workspace.owner': 'red', 'workspace.admin': 'orange', reviewer: 'blue', 'compliance.officer': 'purple', member: 'gray' };
const roleLabel = (k: string) => k.replace('workspace.', '').replace(/[._]/g, ' ').replace(/^\w/, (c) => c.toUpperCase());

/** Admin · Members (docs/05 §3.14). */
export function Members() {
  const { can } = useSession();
  const { query } = useRoute();
  const [members, reload, loadError] = useItems<Member>('/api/v1/members');
  const [invites, reloadInvites] = useItems<Invitation>(can('member:invite') ? '/api/v1/members/invitations' : null);
  const [inviting, setInviting] = useState(query.get('invite') === '1');
  const [importing, setImporting] = useState(false);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(null);
  const [revoking, setRevoking] = useState<PersonRow | null>(null);
  const manageDir = can('directory:manage');
  const [departments] = useItems<Department>(manageDir ? '/api/v1/departments' : null);
  const [groups] = useItems<Group>(manageDir ? '/api/v1/groups' : null);
  // Bulk directory changes on the selected rows (docs/design/ia-competitive-analysis.md §8.3).
  const [bulk, setBulk] = useState<{ kind: 'department' | 'group'; rows: BulkRow[]; clear: () => void } | null>(null);
  const bulkDone = (title: string, tone: 'success' | 'danger') => { bulk?.clear(); setBulk(null); setNotice({ tone, title }); reload(); };
  const toBulk = (rows: PersonRow[]): BulkRow[] => rows.map((p) => ({ id: p.id, name: p.name, active: p.status === 'Active', invite: p.invite }));
  const fail = (e: unknown) => setNotice({ tone: 'danger', title: e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'That didn’t work. Try again.' });
  // Resending is inviting again: a new link goes out and the earlier one stops working (docs/08 "Members").
  const resend = async (p: PersonRow) => {
    try {
      await api('/api/v1/members/invitations', { method: 'POST', body: { email: p.email, roles: p.roles } });
      setNotice({ tone: 'success', title: `Invitation sent again to ${p.email}. The earlier link no longer works.` });
      reloadInvites();
    } catch (e) { fail(e); }
  };
  const revoke = async (p: PersonRow) => {
    setRevoking(null);
    try {
      await api(`/api/v1/members/invitations/${p.id}`, { method: 'DELETE' });
      setNotice({ tone: 'success', title: `Invitation to ${p.email} revoked.` });
      reloadInvites();
    } catch (e) { fail(e); }
  };
  useEffect(() => { if (query.get('invite') === '1') setInviting(true); }, [query]);
  const people = useMemo<PersonRow[]>(() => [
    ...(members ?? []).map((m) => ({ id: m.id, name: m.display_name, email: m.email, roles: m.roles, title: m.title, status: m.status === 'active' ? 'Active' : 'Deactivated', date: m.joined_at, invite: false })),
    ...(invites ?? []).map((i) => ({ id: i.id, name: i.email, email: i.email, roles: i.role_keys, title: null, status: 'Invited', date: i.expires_at, invite: true })),
  ], [members, invites]);
  const active = (members ?? []).filter((m) => m.status === 'active').length;

  return (
    <>
      <PeopleTabs current="members" />
      <div className="page-head">
        <div><h1>Members</h1><p>{members ? `${active} active ${active === 1 ? 'member' : 'members'}${invites?.length ? ` · ${invites.length} pending invitation${invites.length === 1 ? '' : 's'}` : ''}` : 'People in this workspace'}</p></div>
        <div className="actions">
          {can('directory:manage') && <Button variant="secondary" onPress={() => setImporting(true)}><FileUp aria-hidden="true" size="1rem" /> Import directory</Button>}
          {can('member:invite') && <Button onPress={() => setInviting(true)}><MailPlus aria-hidden="true" size="1rem" /> Invite people</Button>}
        </div>
      </div>
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      <DirectoryImport isOpen={importing} onClose={() => setImporting(false)} onApplied={() => { setNotice({ tone: 'success', title: 'Directory updated.' }); reload(); }} />
      {loadError ? <LoadError error={loadError} onRetry={reload} what="members" /> : members === null ? <Skeleton variant="table" rows={6} label="Loading members" /> : (
        <DataGrid<PersonRow> caption="Members" showCaption={false} list="members" data={people} selectable={manageDir} rowLabel={(p) => p.name}
          bulkActions={(sel, clear) => <>
            <Button variant="secondary" className="btn-sm" onPress={() => setBulk({ kind: 'department', rows: toBulk(sel), clear })}><Network aria-hidden="true" size="0.875rem" /> Set department</Button>
            <Button variant="secondary" className="btn-sm" onPress={() => setBulk({ kind: 'group', rows: toBulk(sel), clear })}><UsersRound aria-hidden="true" size="0.875rem" /> Add to group</Button>
          </>}
          initialSearch={query.get('q') ?? ''} initialSort={[{ id: 'name', desc: false }]}
          columns={[
            { id: 'name', header: 'Name', value: (p) => `${p.name} ${p.email}`, cell: (p) => <span className="cell-entity"><Avatar name={p.name} size="1.75rem" tone={p.invite ? 'neutral' : 'person'} /><span style={{ minWidth: 0 }}>{p.invite ? <span style={{ display: 'block' }}>{p.name}</span> : <Link to={`/members/${p.id}`} style={{ display: 'block' }}>{p.name}</Link>}{!p.invite && <span className="cell-muted" style={{ fontWeight: 400, fontSize: '0.75rem' }}>{p.email}</span>}</span></span> },
            { id: 'roles', header: 'Roles', type: 'enum', options: [...new Set(people.flatMap((p) => p.roles.map(roleLabel)))].sort(), value: (p) => p.roles.map(roleLabel).join(', '), cell: (p) => <span style={{ display: 'inline-flex', gap: '0.25rem', flexWrap: 'wrap' }}>{p.roles.map((r) => <Tag key={r} color={ROLE_COLOR[r] ?? 'gray'}>{roleLabel(r)}</Tag>)}</span> },
            { id: 'title', header: 'Title', optional: true, value: (p) => p.title ?? '' },
            { id: 'status', header: 'Status', type: 'enum', value: (p) => p.status, cell: (p) => <Tag color={p.status === 'Active' ? 'green' : p.status === 'Invited' ? 'yellow' : 'gray'} dot>{p.status}</Tag> },
            { id: 'date', header: 'Joined', type: 'date', optional: true, value: (p) => p.date, cell: (p) => <span className="cell-muted">{p.invite ? `expires ${shortDate(p.date)}` : shortDate(p.date)}</span> },
            ...(can('member:invite') && invites?.length ? [{ id: 'invitation', header: 'Invitation', sortable: false, value: (p: PersonRow) => (p.invite ? 'pending' : ''), cell: (p: PersonRow) => p.invite && (
              <span className="actions" style={{ flexWrap: 'nowrap' }}>
                <Button variant="secondary" className="btn-sm" aria-label={`Resend invitation to ${p.email}`} onPress={() => void resend(p)}>Resend</Button>
                <Button variant="ghost" className="btn-sm" aria-label={`Revoke invitation to ${p.email}`} onPress={() => setRevoking(p)}>Revoke</Button>
              </span>
            ) }] : []),
          ]} />
      )}
      {bulk?.kind === 'department' && <BulkSetDepartment rows={bulk.rows} departments={departments ?? []} onClose={() => setBulk(null)} onDone={bulkDone} />}
      {bulk?.kind === 'group' && <BulkAddToGroup rows={bulk.rows} groups={groups ?? []} onClose={() => setBulk(null)} onDone={bulkDone} />}
      <FormDialog isOpen={revoking !== null} onOpenChange={(o) => !o && setRevoking(null)} title={`Revoke the invitation to ${revoking?.email ?? ''}?`}
        description="The link in their email stops working at once. You can invite them again later."
        footer={<><Button variant="secondary" autoFocus onPress={() => setRevoking(null)}>Keep</Button><Button variant="danger" onPress={() => revoking && void revoke(revoking)}>Revoke invitation</Button></>}>
        <p className="muted" style={{ margin: 0 }}>This is recorded in the audit log.</p>
      </FormDialog>
      <InviteDialog isOpen={inviting} onClose={() => { setInviting(false); if (query.get('invite')) navigate('/admin/members', { replace: true }); }} onInvited={() => { reload(); reloadInvites(); }} />
    </>
  );
}

function InviteDialog({ isOpen, onClose, onInvited }: { isOpen: boolean; onClose: () => void; onInvited: () => void }) {
  const [email, setEmail] = useState('');
  const [role, setRole] = useState('member');
  const [error, setError] = useState<string | null>(null);
  const submit = async () => {
    setError(null);
    try {
      await api('/api/v1/members/invitations', { method: 'POST', body: { email, roles: BASE_ROLES.has(role) ? [role] : ['member', role] } });
      setEmail(''); onInvited(); onClose();
    } catch (e) { setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not send the invitation.'); }
  };
  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} title="Invite people" description="They’ll get an email with a link to join. The link works once and expires in 7 days."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={!email.includes('@')}>Send invitation</Button></>}>
      {error && <Notice tone="danger" title={error} />}
      <Form onSubmit={(e) => { e.preventDefault(); void submit(); }}><TextField label="Email address" type="email" value={email} onChange={setEmail} autoFocus /></Form>
      <div className="radio-cards" role="group" aria-label="Role">
        {[['member', 'Member', 'Reads, acknowledges, takes part'], ['reviewer', 'Reviewer', 'Reviews and approves drafts'], ['workspace.admin', 'Admin', 'Manages people and settings'], ['compliance.officer', 'Compliance', 'Tracks campaigns and evidence']].map(([k, label, hint]) => (
          <button key={k} type="button" className="radio-card" aria-pressed={role === k} onClick={() => setRole(k!)}><span><strong>{label}</strong><small>{hint}</small></span></button>
        ))}
      </div>
    </FormDialog>
  );
}
