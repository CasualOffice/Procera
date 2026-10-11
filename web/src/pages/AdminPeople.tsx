import { ShieldAlert, ShieldCheck } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { shortDate, timeAgo } from '../lib/format';
import { navigate, useRoute } from '../lib/router';
import { Avatar } from '../ui/Avatar';
import { Tag, type TagColor } from '../ui/Tag';
import '../ui/admin.css';
import { Skeleton } from '../ui/States';

interface Person { membership_id: string; name: string; email: string; department: string | null; roles: string[]; mfa: boolean; last_active_at: string | null; joined_at: string; privileged: boolean; dormant: boolean; failed_logins_30d: number; grant_expiring: boolean }
interface People {
  filter: string;
  counts: { members: number; no_mfa: number; dormant: number; privileged: number; expiring: number; pending_invites: number };
  items: Person[];
  expiring_grants: { membership_id: string; name: string; role: string; expires_at: string }[];
  pending_invites: { id: string; email: string; invited_at: string; expires_at: string }[];
}

const FILTERS = [['all', 'Everyone', 'members'], ['no_mfa', 'No two-step verification', 'no_mfa'], ['privileged', 'Admins', 'privileged'], ['dormant', 'Dormant', 'dormant'], ['expiring', 'Grants expiring', 'expiring']] as const;
const ROLE_COLOR: Record<string, TagColor> = { 'workspace.owner': 'red', 'workspace.admin': 'orange', 'security.admin': 'purple', reviewer: 'blue', member: 'gray' };
const roleLabel = (k: string) => k.replace('workspace.', '').replace(/[._]/g, ' ').replace(/^\w/, (c) => c.toUpperCase());

/** Admin · People & access (FR-A06): who is here, how they sign in, and which access needs a look. */
export function AdminPeople() {
  const { query } = useRoute();
  const filter = query.get('filter') ?? 'all';
  const [data, setData] = useState<People | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    setData(null); setFailed(false);
    api<People>(`/api/v1/admin/people?filter=${encodeURIComponent(filter)}`).then(setData).catch(() => setFailed(true));
  }, [filter]);

  return (
    <>
      <div className="page-head"><div><h1>People &amp; access</h1><p>Sign-in health, admin access and time-boxed grants.</p></div></div>
      <div className="filter-chips" role="group" aria-label="Show">
        {FILTERS.map(([key, label, count]) => (
          <button key={key} type="button" className="filter-chip" aria-pressed={filter === key} onClick={() => navigate(key === 'all' ? '/admin/people' : `/admin/people?filter=${key}`, { replace: true })}>
            {label}{data && <span className="filter-chip-count">{data.counts[count]}</span>}
          </button>
        ))}
      </div>
      {failed && <p className="muted" role="alert">People could not be loaded. Try again in a moment.</p>}
      {!data && !failed && <Skeleton variant="table" rows={5} label="Loading people" />}
      {data && (
        <>
          {data.items.length === 0 ? (
            <div className="overview-empty"><ShieldCheck aria-hidden="true" size="1rem" /><span>{filter === 'all' ? 'No members yet.' : 'Nobody matches this view. That’s good news.'}</span></div>
          ) : (
            <div className="table-wrap">
              <table className="table" aria-label="People">
                <thead><tr><th>Name</th><th>Roles</th><th>Two-step</th><th>Last active</th><th className="col-optional">Department</th></tr></thead>
                <tbody>{data.items.map((p) => (
                  <tr key={p.membership_id} id={p.membership_id}>
                    <td><span className="cell-entity"><Avatar name={p.name} size="1.75rem" /><span style={{ minWidth: 0 }}><span style={{ display: 'block' }}>{p.name}</span><span className="cell-muted" style={{ fontWeight: 400, fontSize: '0.75rem' }}>{p.email}</span></span></span></td>
                    <td><span style={{ display: 'inline-flex', gap: '0.25rem', flexWrap: 'wrap' }}>{p.roles.map((r) => <Tag key={r} color={ROLE_COLOR[r] ?? 'gray'}>{roleLabel(r)}</Tag>)}{p.grant_expiring && <Tag color="yellow">Expiring</Tag>}</span></td>
                    <td>{p.mfa ? <Tag color="green" dot>On</Tag> : <Tag color={p.privileged ? 'red' : 'gray'} dot>{p.privileged ? 'Off (admin)' : 'Off'}</Tag>}</td>
                    <td className="cell-muted">{p.last_active_at ? timeAgo(p.last_active_at) : 'Never'}{p.dormant && <> · <Tag color="yellow">Dormant</Tag></>}{p.failed_logins_30d > 0 && <span className="failed-hint"><ShieldAlert aria-hidden="true" size="0.8125rem" /> {p.failed_logins_30d} failed</span>}</td>
                    <td className="cell-muted col-optional">{p.department ?? '—'}</td>
                  </tr>
                ))}</tbody>
              </table>
            </div>
          )}
          {data.expiring_grants.length > 0 && (
            <section aria-labelledby="exp-h" className="people-section">
              <h2 id="exp-h">Grants ending in the next 14 days</h2>
              <ul className="plain-list">{data.expiring_grants.map((g) => <li key={`${g.membership_id}-${g.role}`}><strong>{g.name}</strong> · {roleLabel(g.role)} · ends {shortDate(g.expires_at)}</li>)}</ul>
            </section>
          )}
          {data.pending_invites.length > 0 && (
            <section aria-labelledby="inv-h" className="people-section">
              <h2 id="inv-h">Pending invitations</h2>
              <ul className="plain-list">{data.pending_invites.map((i) => <li key={i.id}><strong>{i.email}</strong> · invited {shortDate(i.invited_at)} · expires {shortDate(i.expires_at)}</li>)}</ul>
            </section>
          )}
        </>
      )}
    </>
  );
}
