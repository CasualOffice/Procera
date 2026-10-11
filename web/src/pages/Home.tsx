import { BookOpenCheck, FilePlus2, FileText, Megaphone, ScrollText, UserPlus } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { type Artifact, type Member, memberMap, useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { KIND_TAG, STATE_TAG, Tag } from '../ui/Tag';

interface Ack { id: string; campaign_title: string; due_at: string; overdue: boolean; subject: { artifact_id: string; kind: string; title: string; version_no: number } }
interface FeedItem { id: string; title: string; priority: string; pinned: boolean; require_ack: boolean; published_at: string | null; read_at: string | null; acked_at: string | null }
interface Notification { id: string; kind: string; title: string; body: string; link: string | null; created_at: string }

function greeting() {
  const h = new Date().getHours();
  return h < 12 ? 'Good morning' : h < 18 ? 'Good afternoon' : 'Good evening';
}

/** Home (docs/05 §3.1): what needs me today — actions first. */
export function Home() {
  const { user, workspace, can } = useSession();
  const canLibrary = can('policy:view') || can('sop:view');
  const [acks] = useItems<Ack>('/api/v1/me/acknowledgements?state=pending');
  const [feed] = useItems<FeedItem>(can('announcement:view') ? '/api/v1/announcements/feed' : null);
  const [docs] = useItems<Artifact>(canLibrary ? '/api/v1/artifacts' : null);
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  const [notes, setNotes] = useState<Notification[] | null>(null);
  useEffect(() => { api<{ items: Notification[] }>('/api/v1/notifications?limit=6').then((r) => setNotes(r.items)).catch(() => setNotes([])); }, []);

  const people = memberMap(members);
  const reviews = (docs ?? []).filter((d) => d.state === 'in_review' && (can(`${d.kind}:review`) || can(`${d.kind}:approve`)));
  const unreadNews = (feed ?? []).filter((f) => !f.read_at || (f.require_ack && !f.acked_at));
  const todo = [
    ...(acks ?? []).map((a) => ({
      key: `ack-${a.id}`, icon: <BookOpenCheck aria-hidden="true" size={16} />, tone: a.overdue ? 'red' : 'purple',
      title: `Acknowledge “${a.subject.title}”`, meta: `${a.campaign_title} · due ${shortDate(a.due_at)}`,
      tag: a.overdue ? <Tag color="red" dot>Overdue</Tag> : <Tag color="purple">v{a.subject.version_no}</Tag>,
      action: <Button variant="secondary" className="btn-sm" onPress={() => navigate(`/library/${a.subject.artifact_id}`)}>Read and acknowledge</Button>,
    })),
    ...reviews.map((d) => ({
      key: `rev-${d.id}`, icon: <FileText aria-hidden="true" size={16} />, tone: 'yellow',
      title: `Review “${d.title}”`, meta: `${KIND_TAG[d.kind]?.label ?? d.kind} waiting for review`,
      tag: <Tag color="yellow" dot>In review</Tag>,
      action: <Button variant="secondary" className="btn-sm" onPress={() => navigate(`/library/${d.id}`)}>Review</Button>,
    })),
    ...unreadNews.map((f) => ({
      key: `ann-${f.id}`, icon: <Megaphone aria-hidden="true" size={16} />, tone: f.priority === 'critical' ? 'red' : 'blue',
      title: f.title, meta: `Announcement · ${timeAgo(f.published_at)}`,
      tag: f.require_ack && !f.acked_at ? <Tag color="orange">Acknowledge</Tag> : <Tag color="blue">New</Tag>,
      action: <Button variant="secondary" className="btn-sm" onPress={() => navigate('/announcements')}>Open</Button>,
    })),
  ];
  const loading = acks === null || (canLibrary && docs === null);
  const date = new Date().toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' });
  const admin = can('member:invite');

  return (
    <>
      <div className="home-hero">
        <p className="home-date">{date}</p>
        <h1 className="home-title">{greeting()}, {user?.display_name.split(' ')[0]}</h1>
        <div className="quick-actions" role="group" aria-label="Quick actions">
          {can('policy:create') && <Link className="quick" to="/library?new=policy"><span className="quick-icon tag-purple"><FilePlus2 aria-hidden="true" size={16} /></span>New policy</Link>}
          {can('sop:create') && <Link className="quick" to="/library?new=sop"><span className="quick-icon tag-blue"><ScrollText aria-hidden="true" size={16} /></span>New SOP</Link>}
          {admin && <Link className="quick" to="/admin/members?invite=1"><span className="quick-icon tag-green"><UserPlus aria-hidden="true" size={16} /></span>Invite people</Link>}
          {can('announcement:view') && <Link className="quick" to="/announcements"><span className="quick-icon tag-orange"><Megaphone aria-hidden="true" size={16} /></span>Announcements</Link>}
        </div>
      </div>

      <div className="home-grid">
        <section aria-labelledby="foryou" className="home-main">
          <div className="section-head">
            <h2 id="foryou">For you</h2>
            {todo.length > 0 && <span className="count-pill">{todo.length}</span>}
          </div>
          {loading ? <div className="skeleton-list" aria-label="Loading"><span /><span /><span /></div> : todo.length === 0 ? (
            <div className="empty-soft">
              <p className="empty-title">You’re all caught up</p>
              <p className="muted">Acknowledgements, reviews and announcements that need you will show up here.</p>
            </div>
          ) : (
            <ul className="tasks">
              {todo.map((t) => (
                <li key={t.key}>
                  <span className={`task-icon tag-${t.tone}`}>{t.icon}</span>
                  <div className="task-main">
                    <p className="task-title">{t.title}</p>
                    <p className="task-meta">{t.meta}</p>
                  </div>
                  {t.tag}
                  {t.action}
                </li>
              ))}
            </ul>
          )}

          {canLibrary && (
            <>
              <div className="section-head" style={{ marginTop: 36 }}>
                <h2 id="lib">Library</h2>
                <Link className="section-link" to="/library">View all</Link>
              </div>
              {docs === null ? <div className="skeleton-list"><span /><span /></div> : docs.length === 0 ? (
                <div className="empty-soft"><p className="empty-title">No documents yet</p><p className="muted">Write your first policy or SOP.</p></div>
              ) : (
                <div className="table-wrap">
                  <table className="table" aria-labelledby="lib">
                    <thead><tr><th>Name</th><th>Type</th><th>Status</th><th>Owner</th></tr></thead>
                    <tbody>
                      {docs.slice(0, 6).map((d) => {
                        const o = d.owner_membership_id ? people.get(d.owner_membership_id) : undefined;
                        return (
                          <tr key={d.id}>
                            <td><span className="cell-entity"><span className={`doc-icon tag-${KIND_TAG[d.kind]?.color ?? 'gray'}`}>{d.kind === 'sop' ? <ScrollText aria-hidden="true" size={14} /> : <FileText aria-hidden="true" size={14} />}</span><Link to={`/library/${d.id}`}>{d.title}</Link></span></td>
                            <td><Tag color={KIND_TAG[d.kind]?.color}>{KIND_TAG[d.kind]?.label ?? d.kind}</Tag></td>
                            <td><Tag color={STATE_TAG[d.state]?.color} dot>{STATE_TAG[d.state]?.label ?? d.state}</Tag></td>
                            <td>{o ? <span className="person"><Avatar name={o.display_name} size={20} />{o.display_name}</span> : <span className="cell-muted">—</span>}</td>
                          </tr>
                        );
                      })}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          )}
        </section>

        <aside className="home-rail" aria-label="Activity">
          <div className="section-head"><h2 id="activity">Recent activity</h2></div>
          {notes === null ? <div className="skeleton-list"><span /><span /></div> : notes.length === 0 ? <p className="muted">Nothing yet.</p> : (
            <ul className="activity">
              {notes.map((n) => (
                <li key={n.id}>
                  {n.kind === 'member_joined' ? <Avatar name={n.title.replace(/ joined .*$/, '')} size={24} /> : <span className={`activity-icon tag-${n.kind.startsWith('campaign') ? 'purple' : n.kind === 'announcement' ? 'orange' : 'gray'}`}>{n.kind.startsWith('campaign') ? <BookOpenCheck aria-hidden="true" size={13} /> : n.kind === 'announcement' ? <Megaphone aria-hidden="true" size={13} /> : <FileText aria-hidden="true" size={13} />}</span>}
                  <div>
                    <p className="activity-text">{n.link ? <Link to={n.link}>{n.title}</Link> : n.title}</p>
                    <p className="activity-time"><time dateTime={n.created_at}>{timeAgo(n.created_at)}</time></p>
                  </div>
                </li>
              ))}
            </ul>
          )}
          {members && (
            <>
              <div className="section-head" style={{ marginTop: 28 }}><h2>People</h2><Link className="section-link" to="/admin/members">{members.filter((m) => m.status === 'active').length} members</Link></div>
              <div className="avatar-stack" aria-label={`${members.length} members`}>
                {members.filter((m) => m.status === 'active').slice(0, 8).map((m) => <span key={m.id} title={m.display_name}><Avatar name={m.display_name} size={30} /></span>)}
              </div>
            </>
          )}
        </aside>
      </div>
    </>
  );
}
