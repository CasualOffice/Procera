import { MailPlus, Search } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form, Input, SearchField } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { shortDate } from '../lib/format';
import { navigate, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Tag, type TagColor } from '../ui/Tag';

interface Invitation { id: string; email: string; role_keys: string[]; expires_at: string }
const ROLE_COLOR: Record<string, TagColor> = { 'workspace.owner': 'red', 'workspace.admin': 'orange', reviewer: 'blue', 'compliance.officer': 'purple', member: 'gray' };
const roleLabel = (k: string) => k.replace('workspace.', '').replace(/[._]/g, ' ').replace(/^\w/, (c) => c.toUpperCase());

/** Admin · Members (docs/05 §3.14). */
export function Members() {
  const { can } = useSession();
  const { query } = useRoute();
  const [members, reload] = useItems<Member>('/api/v1/members');
  const [invites, reloadInvites] = useItems<Invitation>(can('member:invite') ? '/api/v1/members/invitations' : null);
  const [q, setQ] = useState('');
  const [inviting, setInviting] = useState(query.get('invite') === '1');
  useEffect(() => { if (query.get('invite') === '1') setInviting(true); }, [query]);
  const rows = useMemo(() => (members ?? []).filter((m) => !q || `${m.display_name} ${m.email}`.toLowerCase().includes(q.toLowerCase())), [members, q]);
  const active = (members ?? []).filter((m) => m.status === 'active').length;

  return (
    <>
      <div className="page-head">
        <div><h1>Members</h1><p>{members ? `${active} active ${active === 1 ? 'member' : 'members'}${invites?.length ? ` · ${invites.length} pending invitation${invites.length === 1 ? '' : 's'}` : ''}` : 'People in this workspace'}</p></div>
        {can('member:invite') && <Button onPress={() => setInviting(true)}><MailPlus aria-hidden="true" size={16} /> Invite people</Button>}
      </div>
      <div className="toolbar">
        <SearchField className="search-input" aria-label="Filter members" value={q} onChange={setQ}>
          <Search aria-hidden="true" size={15} /><Input placeholder="Search by name or email…" />
        </SearchField>
      </div>
      {members === null ? <div className="skeleton-list"><span /><span /><span /></div> : (
        <div className="table-wrap">
          <table className="table" aria-label="Members">
            <thead><tr><th>Name</th><th>Roles</th><th className="col-optional">Title</th><th>Status</th><th className="col-optional">Joined</th></tr></thead>
            <tbody>
              {rows.map((m) => (
                <tr key={m.id}>
                  <td><span className="cell-entity"><Avatar name={m.display_name} size={28} /><span style={{ minWidth: 0 }}><span style={{ display: 'block' }}>{m.display_name}</span><span className="cell-muted" style={{ fontWeight: 400, fontSize: 12 }}>{m.email}</span></span></span></td>
                  <td><span style={{ display: 'inline-flex', gap: 4, flexWrap: 'wrap' }}>{m.roles.map((r) => <Tag key={r} color={ROLE_COLOR[r] ?? 'gray'}>{roleLabel(r)}</Tag>)}</span></td>
                  <td className="cell-muted col-optional">{m.title ?? '—'}</td>
                  <td>{m.status === 'active' ? <Tag color="green" dot>Active</Tag> : <Tag color="gray" dot>Deactivated</Tag>}</td>
                  <td className="cell-muted col-optional">{shortDate(m.joined_at)}</td>
                </tr>
              ))}
              {(invites ?? []).map((i) => (
                <tr key={i.id}>
                  <td><span className="cell-entity"><Avatar name={i.email} size={28} tone="neutral" /><span>{i.email}</span></span></td>
                  <td><span style={{ display: 'inline-flex', gap: 4 }}>{i.role_keys.map((r) => <Tag key={r} color={ROLE_COLOR[r] ?? 'gray'}>{roleLabel(r)}</Tag>)}</span></td>
                  <td className="cell-muted col-optional">—</td>
                  <td><Tag color="yellow" dot>Invited</Tag></td>
                  <td className="cell-muted col-optional">expires {shortDate(i.expires_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
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
      await api('/api/v1/members/invitations', { method: 'POST', body: { email, roles: [role] } });
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
