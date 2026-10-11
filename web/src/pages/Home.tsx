import { Bell, BookOpen, Check, FileText, ScrollText, UserPlus, Users } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { Link } from '../lib/router';
import { useSession } from '../lib/session';

interface Notification { id: string; kind: string; title: string; body: string; link: string | null; created_at: string }
interface Artifact { id: string; kind: string; key: string; title: string; state: string }
interface Member { status: string }

export function timeAgo(iso: string) {
  const s = Math.max(0, (Date.now() - Date.parse(iso)) / 1000);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

function greeting() {
  const h = new Date().getHours();
  return h < 12 ? 'Good morning' : h < 18 ? 'Good afternoon' : 'Good evening';
}

export const STATE_CHIP: Record<string, { label: string; tone: string }> = {
  draft: { label: 'Draft', tone: '' },
  in_review: { label: 'In review', tone: 'chip-info' },
  approved: { label: 'Approved', tone: 'chip-info' },
  scheduled: { label: 'Scheduled', tone: 'chip-warning' },
  published: { label: 'Published', tone: 'chip-success' },
  deprecated: { label: 'Superseded', tone: '' },
  retired: { label: 'Retired', tone: '' },
};

/** Home (docs/05 §3.1): what needs me today. */
export function Home() {
  const { user, workspace, can } = useSession();
  const [notes, setNotes] = useState<Notification[] | null>(null);
  const [unread, setUnread] = useState(0);
  const [docs, setDocs] = useState<Artifact[] | null>(null);
  const [members, setMembers] = useState<number | null>(null);
  const [invites, setInvites] = useState<number | null>(null);
  const canLibrary = can('policy:view') || can('sop:view');
  const admin = can('member:invite');

  useEffect(() => {
    api<{ items: Notification[]; unread_count: number }>('/api/v1/notifications?unread=true&limit=6').then((r) => { setNotes(r.items); setUnread(r.unread_count); }).catch(() => setNotes([]));
    if (canLibrary) api<{ items: Artifact[] }>('/api/v1/artifacts').then((r) => setDocs(r.items)).catch(() => setDocs([]));
    if (can('member:view')) api<{ items: Member[] }>('/api/v1/members').then((r) => setMembers(r.items.filter((m) => m.status === 'active').length)).catch(() => undefined);
    if (admin) api<{ items: unknown[] }>('/api/v1/members/invitations').then((r) => setInvites(r.items.length)).catch(() => undefined);
  }, [can, canLibrary, admin]);

  const published = docs?.filter((d) => d.state === 'published') ?? [];
  const inProgress = docs?.filter((d) => ['draft', 'in_review', 'approved', 'scheduled'].includes(d.state)) ?? [];
  const steps = [
    { done: (members ?? 0) > 1 || (invites ?? 0) > 0, title: 'Invite your team', text: 'They get an email with a link to join.', to: '/admin/members' },
    { done: published.some((d) => d.kind === 'policy'), title: 'Publish your first policy', text: 'Draft it, get it reviewed, publish it.', to: '/library' },
    { done: published.some((d) => d.kind === 'sop'), title: 'Document a procedure', text: 'Write an SOP people can follow.', to: '/library' },
  ];
  const doneCount = steps.filter((s) => s.done).length;

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Home</h1>
          <p>{greeting()}, {user?.display_name.split(' ')[0]}. Here’s what’s happening in {workspace?.name}.</p>
        </div>
      </div>

      <section className="card metrics" aria-label="Summary">
        <div className="metric">
          <p className="metric-label"><Bell aria-hidden="true" size={14} /> Unread</p>
          <p className="metric-value"><Link to="/notifications">{unread}</Link></p>
          <p className="metric-hint">notifications</p>
        </div>
        {canLibrary && (
          <div className="metric">
            <p className="metric-label"><BookOpen aria-hidden="true" size={14} /> Published</p>
            <p className="metric-value">{docs ? published.length : '–'}</p>
            <p className="metric-hint">policies and SOPs</p>
          </div>
        )}
        {canLibrary && (
          <div className="metric">
            <p className="metric-label"><FileText aria-hidden="true" size={14} /> In progress</p>
            <p className="metric-value">{docs ? inProgress.length : '–'}</p>
            <p className="metric-hint">drafts and reviews</p>
          </div>
        )}
        {members !== null && (
          <div className="metric">
            <p className="metric-label"><Users aria-hidden="true" size={14} /> Members</p>
            <p className="metric-value">{members}</p>
            <p className="metric-hint">{invites ? `${invites} invitation${invites === 1 ? '' : 's'} pending` : 'active'}</p>
          </div>
        )}
      </section>

      <div className="grid-2">
        <div className="col">
          <section className="card" aria-labelledby="attention">
            <div className="card-head">
              <h2 id="attention">Needs your attention</h2>
              <Link to="/notifications">View all</Link>
            </div>
            {notes === null ? <div className="card-empty">Loading…</div> : notes.length === 0 ? (
              <div className="card-empty">
                <Check aria-hidden="true" size={20} /><br />You’re all caught up.
              </div>
            ) : (
              <ul className="rows">
                {notes.map((n) => (
                  <li key={n.id} className="rows-link">
                    <span className="row-icon">{n.kind === 'member_joined' ? <UserPlus aria-hidden="true" size={16} /> : <Bell aria-hidden="true" size={16} />}</span>
                    <div className="row-main">
                      <p className="row-title">{n.link ? <Link to={n.link}>{n.title}</Link> : n.title}</p>
                      <p className="row-sub">{n.body}</p>
                    </div>
                    <span className="row-meta"><time dateTime={n.created_at}>{timeAgo(n.created_at)}</time></span>
                  </li>
                ))}
              </ul>
            )}
          </section>

          {canLibrary && (
            <section className="card" aria-labelledby="docs">
              <div className="card-head">
                <h2 id="docs">Library</h2>
                <Link to="/library">Open library</Link>
              </div>
              {docs === null ? <div className="card-empty">Loading…</div> : docs.length === 0 ? (
                <div className="card-empty">No policies or SOPs yet.</div>
              ) : (
                <ul className="rows">
                  {docs.slice(0, 6).map((d) => (
                    <li key={d.id} className="rows-link">
                      <span className="row-icon">{d.kind === 'sop' ? <ScrollText aria-hidden="true" size={16} /> : <FileText aria-hidden="true" size={16} />}</span>
                      <div className="row-main">
                        <p className="row-title"><Link to={`/library/${d.id}`}>{d.title}</Link></p>
                        <p className="row-sub">{d.kind === 'sop' ? 'Standard operating procedure' : 'Policy'} · {d.key}</p>
                      </div>
                      <span className={`chip ${STATE_CHIP[d.state]?.tone ?? ''}`}>{STATE_CHIP[d.state]?.label ?? d.state}</span>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          )}
        </div>

        {admin && (
          <div className="col">
            <section className="card" aria-labelledby="setup">
              <div className="card-head">
                <h2 id="setup">Set up {workspace?.name}</h2>
                <span className="muted" style={{ fontSize: 12 }}>{doneCount} of {steps.length}</span>
              </div>
              <div style={{ padding: '0 16px' }}>
                <div className="progress" role="progressbar" aria-label="Setup progress" aria-valuemin={0} aria-valuemax={steps.length} aria-valuenow={doneCount}>
                  <span style={{ width: `${(doneCount / steps.length) * 100}%` }} />
                </div>
              </div>
              <ul className="checklist">
                {steps.map((s) => (
                  <li key={s.title} className={s.done ? 'done' : undefined}>
                    <span className={`check${s.done ? ' check-done' : ''}`}>
                      {s.done && <Check aria-hidden="true" size={12} strokeWidth={3} />}
                      <span className="visually-hidden">{s.done ? 'Done:' : 'To do:'}</span>
                    </span>
                    <div>
                      <p><Link to={s.to}>{s.title}</Link></p>
                      <p className="muted" style={{ fontSize: 12 }}>{s.text}</p>
                    </div>
                  </li>
                ))}
              </ul>
            </section>
          </div>
        )}
      </div>
    </>
  );
}
