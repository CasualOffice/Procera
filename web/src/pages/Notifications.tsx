import { AlarmClock, ArrowRightLeft, AtSign, Bell, BookOpenCheck, Check, CheckCheck, CircleAlert, CircleCheck, Inbox, Megaphone, MessageSquare, Send, Settings2, UserPlus } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import { type LoadFailure, toFailure } from '../lib/data';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { timeAgo } from '../lib/format';
import { Link, navigate } from '../lib/router';
import { Button } from '../ui/components/Button';

interface Notification { id: string; kind: string; title: string; body: string; link: string | null; read_at: string | null; created_at: string; data?: { request_id?: string; work_item_id?: string } }

const KIND: Record<string, { icon: typeof Bell; tone: string }> = {
  member_joined: { icon: UserPlus, tone: 'green' },
  announcement: { icon: Megaphone, tone: 'orange' },
  campaign_assigned: { icon: BookOpenCheck, tone: 'purple' },
  campaign_reminder: { icon: BookOpenCheck, tone: 'purple' },
  campaign_escalated: { icon: BookOpenCheck, tone: 'red' },
  announcement_ack_reminder: { icon: Megaphone, tone: 'orange' },
  announcement_ack_escalated: { icon: Megaphone, tone: 'red' },
  announcement_approval: { icon: Megaphone, tone: 'orange' },
  announcement_decided: { icon: Megaphone, tone: 'orange' },
  review_requested: { icon: BookOpenCheck, tone: 'blue' },
  review_decided: { icon: BookOpenCheck, tone: 'blue' },
  work_assigned: { icon: Inbox, tone: 'green' },
  work_reminder: { icon: AlarmClock, tone: 'orange' },
  work_escalated: { icon: CircleAlert, tone: 'red' },
  work_reassigned: { icon: ArrowRightLeft, tone: 'blue' },
  request_decided: { icon: Send, tone: 'blue' },
  request_completed: { icon: CircleCheck, tone: 'green' },
  request_comment: { icon: MessageSquare, tone: 'blue' },
  request_mention: { icon: AtSign, tone: 'purple' },
};

/** A link for kinds whose payload has no `link`, from ids in it (requests and work items). */
function fallbackLink(n: Notification): string | null {
  const d = n.data ?? {};
  if ((n.kind === 'work_assigned' || n.kind === 'work_reminder') && d.work_item_id) return `/work/${d.work_item_id}`;
  if (['request_decided', 'request_completed', 'work_assigned', 'work_reminder', 'work_escalated', 'work_reassigned'].includes(n.kind) && d.request_id) return `/requests/${d.request_id}`;
  return n.kind === 'announcement' ? '/announcements' : null;
}

/** Server links may be absolute (email-ready) or API-shaped; the app routes on the path. */
function appLink(link: string | null): string | null {
  if (!link) return null;
  const path = link.replace(/^https?:\/\/[^/]+/, '').replace(/^\/api\/v1/, '');
  return path.startsWith('/') ? path : null;
}

/** Kinds that merge when several arrive the same day ("Gia, Finn and 4 others joined Acme"). */
const MERGEABLE = new Set(['member_joined']);
interface Row { key: string; items: Notification[] }

/** Consecutive notifications of a mergeable kind from the same day become one row. */
function groupRows(items: Notification[]): Row[] {
  const rows: Row[] = [];
  for (const n of items) {
    const last = rows[rows.length - 1];
    const head = last?.items[0];
    if (head && MERGEABLE.has(n.kind) && head.kind === n.kind && head.created_at.slice(0, 10) === n.created_at.slice(0, 10)) last.items.push(n);
    else rows.push({ key: n.id, items: [n] });
  }
  return rows;
}

/** "Gia Nguyen, Finn Walsh and 4 others joined Acme" from titles like "Gia Nguyen joined Acme". */
function mergedTitle(group: Notification[]): string {
  const m = /^(.*) joined (.*)$/.exec(group[0]!.title);
  if (!m) return `${group[0]!.title} and ${group.length - 1} more`;
  const names = group.map((n) => /^(.*) joined /.exec(n.title)?.[1] ?? n.title);
  const rest = names.length - 2;
  const who = rest <= 0 ? names.join(' and ') : `${names[0]}, ${names[1]} and ${rest} ${rest === 1 ? 'other' : 'others'}`;
  return `${who} joined ${m[2]}`;
}

/** The member's inbox (FR-N01): opening an item marks it read. */
export function Notifications() {
  const [items, setItems] = useState<Notification[] | null>(null);
  const [error, setError] = useState<LoadFailure | null>(null);
  const load = useCallback(() => api<{ items: Notification[] }>('/api/v1/notifications').then((r) => { setItems(r.items); setError(null); }).catch((e: unknown) => setError(toFailure(e))), []);
  useEffect(() => { void load(); }, [load]);
  const unread = items?.filter((n) => !n.read_at).length ?? 0;
  const markRead = async (...ids: string[]) => { await Promise.all(ids.map((id) => api(`/api/v1/notifications/${id}/read`, { method: 'POST' }))); await load(); };
  const [open, setOpen] = useState<Set<string>>(new Set());
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Notifications</h1>
          <p>{unread ? `${unread} unread` : 'All caught up'}</p>
        </div>
        <div className="page-actions">
          <Link to="/notifications/preferences" className="btn btn-secondary btn-md link-btn"><Settings2 aria-hidden="true" size="1rem" /> Preferences</Link>
          {unread > 0 && (
            <Button variant="secondary" onPress={async () => { await api('/api/v1/notifications/read-all', { method: 'POST' }); await load(); }}>
              <CheckCheck aria-hidden="true" size="1rem" /> Mark all as read
            </Button>
          )}
        </div>
      </div>
      {error ? <LoadError error={error} onRetry={() => void load()} what="notifications" /> : items === null ? <Skeleton rows={5} label="Loading notifications" /> : items.length === 0 ? (
        <Empty art="notifications" title="No notifications yet">You’ll hear here when something needs you: a document to acknowledge, a review, or a new announcement.</Empty>
      ) : (
      <section className="card" aria-label="Notifications">
        {(
          <ul className="rows">
            {groupRows(items).map(({ key, items: group }) => {
              const n = group[0]!;
              const merged = group.length > 1;
              const unreadIds = group.filter((x) => !x.read_at).map((x) => x.id);
              const isRead = unreadIds.length === 0;
              const title = merged ? mergedTitle(group) : n.title;
              const k = KIND[n.kind] ?? { icon: Bell, tone: 'gray' };
              const target = appLink(n.link) ?? fallbackLink(n);
              const expanded = open.has(key);
              return (
                <li key={key} className="inbox-row rows-link">
                  <span className={isRead ? 'read-dot' : 'unread-dot'} aria-hidden="true" />
                  <span className={`task-icon tag-${k.tone}`}><k.icon aria-hidden="true" size="1rem" /></span>
                  <div className="row-main">
                    <p className="row-title" style={{ fontWeight: isRead ? 500 : 600 }}>
                      {!isRead && <span className="visually-hidden">Unread: </span>}
                      {target && !merged ? <Link to={target} onClick={() => { if (!isRead) void markRead(...unreadIds); }}>{title}</Link> : title}
                    </p>
                    {merged ? (
                      <>
                        <button type="button" className="inbox-more" aria-expanded={expanded} onClick={() => setOpen((o) => { const x = new Set(o); if (x.has(key)) x.delete(key); else x.add(key); return x; })}>
                          {expanded ? 'Hide' : `Show all ${group.length}`}
                        </button>
                        {expanded && <ul className="inbox-sub">{group.map((g) => <li key={g.id}>{g.title}<span className="row-meta"> · {timeAgo(g.created_at)}</span></li>)}</ul>}
                      </>
                    ) : <p className="row-sub">{n.body}</p>}
                  </div>
                  <span className="row-meta"><time dateTime={n.created_at}>{timeAgo(n.created_at)}</time></span>
                  {!isRead && (
                    <button type="button" className="icon-btn mark-read" onClick={() => void markRead(...unreadIds)} aria-label={`Mark “${title}” as read`} title="Mark as read">
                      <Check aria-hidden="true" size="1rem" />
                    </button>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </section>
      )}
    </>
  );
}
