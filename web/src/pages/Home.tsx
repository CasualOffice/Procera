import { BookOpenCheck, CircleCheck, FilePlus2, FileText, Megaphone, ScrollText, SquarePen, UserPlus } from 'lucide-react';
import { type Artifact, type Member, useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { KIND_TAG, Tag } from '../ui/Tag';
import { term, useTerms } from '../lib/terms';

interface Ack { id: string; campaign_title: string; due_at: string; overdue: boolean; acknowledged_at: string | null; subject: { artifact_id?: string; announcement_id?: string; kind: string; title: string; version_no?: number; link?: string } }
interface FeedItem { id: string; title: string; priority: string; pinned: boolean; require_ack: boolean; published_at: string | null; read_at: string | null; acked_at: string | null }

function greeting() {
  const h = new Date().getHours();
  return h < 12 ? 'Good morning' : h < 18 ? 'Good afternoon' : 'Good evening';
}

const DAY = 86_400_000;
function dueTag(iso: string, overdue: boolean) {
  if (overdue) return <Tag color="red" dot>Overdue</Tag>;
  const days = Math.ceil((Date.parse(iso) - Date.now()) / DAY);
  return <Tag color={days <= 3 ? 'orange' : 'gray'}>{days <= 1 ? 'Due today' : `Due in ${days} days`}</Tag>;
}

/**
 * Home (docs/05 §3.1): what needs me, nothing else first. One task list with one verb per kind of
 * work ("Read and acknowledge", "Review", "Read"); tags only state facts (due, overdue, critical).
 */
export function Home() {
  useTerms(); // the workspace's terms (CFG-008)
  const { user, can } = useSession();
  const canLibrary = can('policy:view') || can('sop:view');
  const [acks, reloadAcks, acksError] = useItems<Ack>('/api/v1/me/acknowledgements?state=pending');
  const [done] = useItems<Ack>('/api/v1/me/acknowledgements?state=completed');
  const [feed] = useItems<FeedItem>(can('announcement:view') ? '/api/v1/announcements/feed' : null);
  const [docs] = useItems<Artifact>(canLibrary ? '/api/v1/artifacts' : null);
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);

  const reviews = (docs ?? []).filter((d) => d.state === 'in_review' && (can(`${d.kind}:review`) || can(`${d.kind}:approve`)));
  const news = (feed ?? []).filter((f) => (f.require_ack && !f.acked_at) || !f.read_at);
  const todo = [
    // Announcement acknowledgements already appear from the feed below.
    ...(acks ?? []).filter((a) => a.subject.kind !== 'announcement').map((a) => ({
      key: `ack-${a.id}`, kind: 'ack', icon: BookOpenCheck, tone: 'purple', sort: a.overdue ? 0 : 1,
      title: `Acknowledge “${a.subject.title}”`, meta: `${KIND_TAG[a.subject.kind]?.label ?? 'Document'} v${a.subject.version_no} · ${a.campaign_title}`,
      tag: dueTag(a.due_at, a.overdue), label: 'Read and acknowledge', to: `/library/${a.subject.artifact_id}`,
    })),
    ...news.filter((f) => f.require_ack && !f.acked_at).map((f) => ({
      key: `ann-${f.id}`, kind: 'ann', icon: Megaphone, tone: 'orange', sort: f.priority === 'critical' ? 0 : 1,
      title: f.title, meta: `Announcement · posted ${timeAgo(f.published_at)}`,
      tag: f.priority === 'critical' ? <Tag color="red" dot>Critical</Tag> : null, label: 'Read and acknowledge', to: `/announcements?open=${f.id}`,
    })),
    ...reviews.map((d) => ({
      key: `rev-${d.id}`, kind: 'rev', icon: d.kind === 'sop' ? ScrollText : FileText, tone: 'yellow', sort: 2,
      title: `Review “${d.title}”`, meta: `${KIND_TAG[d.kind]?.label ?? d.kind} · waiting for a reviewer`,
      tag: d.criticality === 'controlled' ? <Tag color="red">Controlled</Tag> : null, label: 'Review', to: `/library/${d.id}`,
    })),
    ...news.filter((f) => !(f.require_ack && !f.acked_at)).map((f) => ({
      key: `new-${f.id}`, kind: 'new', icon: Megaphone, tone: 'blue', sort: f.priority === 'critical' ? 0 : 3,
      title: f.title, meta: `Announcement · posted ${timeAgo(f.published_at)}`,
      tag: f.priority === 'critical' ? <Tag color="red" dot>Critical</Tag> : null, label: 'Read', to: `/announcements?open=${f.id}`,
    })),
  ].sort((a, b) => a.sort - b.sort);
  const loading = acks === null || (canLibrary && docs === null) || (can('announcement:view') && feed === null);
  const overdue = (acks ?? []).filter((a) => a.overdue).length;
  const date = new Date().toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long' });
  const summary = loading ? ' ' : todo.length === 0 ? 'Nothing needs you right now.' : `${todo.length} ${todo.length === 1 ? 'thing needs' : 'things need'} you${overdue ? `, ${overdue} overdue` : ''}.`;
  const published = (docs ?? []).filter((d) => d.state === 'published').slice(0, 5);
  const receipts = (done ?? []).filter((a) => a.acknowledged_at).slice(0, 3);

  const create = [
    can('policy:create') && { to: '/library?new=policy', icon: FilePlus2, tone: 'purple', label: 'New policy' },
    can('sop:create') && { to: '/library?new=sop', icon: ScrollText, tone: 'blue', label: `New ${term('sop')}` },
    (can('announcement:create') || can('announcement:manage')) && { to: '/admin/announcements?new=1', icon: SquarePen, tone: 'orange', label: 'New announcement' },
    can('member:invite') && { to: '/admin/members?invite=1', icon: UserPlus, tone: 'green', label: 'Invite people' },
  ].filter(Boolean) as { to: string; icon: typeof FilePlus2; tone: string; label: string }[];

  return (
    <>
      <div className="home-hero">
        <p className="home-date">{date}</p>
        <h1 className="home-title">{greeting()}, {user?.display_name.split(' ')[0]}</h1>
        <p className="home-summary">{summary}</p>
        {create.length > 0 && (
          <div className="quick-actions" role="group" aria-label="Quick actions">
            {create.map((c) => <Link key={c.to} className="quick" to={c.to}><span className={`quick-icon tag-${c.tone}`}><c.icon aria-hidden="true" size="1rem" /></span>{c.label}</Link>)}
          </div>
        )}
      </div>

      <div className="home-grid">
        <section aria-labelledby="foryou" className="home-main">
          <div className="section-head">
            <h2 id="foryou">For you</h2>
            {todo.length > 0 && <span className="count-pill">{todo.length}</span>}
          </div>
          {acksError ? <LoadError error={acksError} onRetry={reloadAcks} what="your tasks" /> : loading ? <Skeleton rows={3} label="Loading your tasks" /> : todo.length === 0 ? (
            <Empty icon={CircleCheck} title="You’re all caught up">Documents to acknowledge, drafts to review and new announcements will appear here.</Empty>
          ) : (
            <ul className="tasks">
              {todo.map((t) => (
                <li key={t.key}>
                  <span className={`task-icon tag-${t.tone}`}><t.icon aria-hidden="true" size="1rem" /></span>
                  <div className="task-main">
                    <p className="task-title">{t.title}</p>
                    <p className="task-meta">{t.meta}</p>
                  </div>
                  {t.tag}
                  <Button variant="secondary" className="btn-sm" onPress={() => navigate(t.to)}>{t.label}</Button>
                </li>
              ))}
            </ul>
          )}

          {receipts.length > 0 && (
            <>
              <div className="section-head section-gap"><h2 id="receipts">Recently acknowledged</h2></div>
              <ul className="receipts" aria-labelledby="receipts">
                {receipts.map((r) => (
                  <li key={r.id}>
                    <CircleCheck aria-hidden="true" size="0.9375rem" className="receipt-icon" />
                    <span>You acknowledged <Link to={r.subject.link ?? `/library/${r.subject.artifact_id}`}>{r.subject.title}</Link>{r.subject.version_no ? ` v${r.subject.version_no}` : ''}</span>
                    <time className="receipt-time" dateTime={r.acknowledged_at!}>{shortDate(r.acknowledged_at)}</time>
                  </li>
                ))}
              </ul>
            </>
          )}
        </section>

        <aside className="home-rail" aria-label="Workspace">
          {canLibrary && (
            <section aria-labelledby="published">
              <div className="section-head"><h2 id="published">Published documents</h2><Link className="section-link" to="/library">Library</Link></div>
              {docs === null ? <Skeleton rows={3} label="Loading documents" /> : published.length === 0 ? (
                <Empty compact title="Nothing published yet">Published policies and {term('sop', { plural: true })} show up here.</Empty>
              ) : (
                <ul className="mini-docs">
                  {published.map((d) => (
                    <li key={d.id}>
                      <span className={`doc-icon doc-icon-sm tag-${KIND_TAG[d.kind]?.color ?? 'gray'}`}>{d.kind === 'sop' ? <ScrollText aria-hidden="true" size="0.8125rem" /> : <FileText aria-hidden="true" size="0.8125rem" />}</span>
                      <Link to={`/library/${d.id}`}>{d.title}</Link>
                      <span className="mini-docs-kind">{KIND_TAG[d.kind]?.label}</span>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          )}
          {members && members.length > 0 && (
            <section aria-labelledby="people" className="section-gap">
              <div className="section-head"><h2 id="people">People</h2>{can('member:invite') && <Link className="section-link" to="/admin/members">Manage</Link>}</div>
              <div className="avatar-stack" role="img" aria-label={`${members.filter((m) => m.status === 'active').length} active members`}>
                {members.filter((m) => m.status === 'active').slice(0, 9).map((m) => <span key={m.id} title={m.display_name}><Avatar name={m.display_name} size="1.875rem" /></span>)}
              </div>
              <p className="rail-note">{members.filter((m) => m.status === 'active').length} people in this workspace</p>
            </section>
          )}
        </aside>
      </div>
    </>
  );
}
