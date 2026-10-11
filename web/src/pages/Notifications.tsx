import { Bell, BookOpenCheck, Check, CheckCheck, Megaphone, Settings2, UserPlus } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import { type LoadFailure, toFailure } from '../lib/data';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { timeAgo } from '../lib/format';
import { Link, navigate } from '../lib/router';
import { Button } from '../ui/components/Button';

interface Notification { id: string; kind: string; title: string; body: string; link: string | null; read_at: string | null; created_at: string }

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
};

/** The member's inbox (FR-N01): opening an item marks it read. */
export function Notifications() {
  const [items, setItems] = useState<Notification[] | null>(null);
  const [error, setError] = useState<LoadFailure | null>(null);
  const load = useCallback(() => api<{ items: Notification[] }>('/api/v1/notifications').then((r) => { setItems(r.items); setError(null); }).catch((e: unknown) => setError(toFailure(e))), []);
  useEffect(() => { void load(); }, [load]);
  const unread = items?.filter((n) => !n.read_at).length ?? 0;
  const markRead = async (id: string) => { await api(`/api/v1/notifications/${id}/read`, { method: 'POST' }); await load(); };
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Notifications</h1>
          <p>{unread ? `${unread} unread` : 'All caught up'}</p>
        </div>
        <div className="page-actions">
          <Link to="/notifications/preferences" className="btn btn-secondary btn-md"><Settings2 aria-hidden="true" size="1rem" /> Preferences</Link>
          {unread > 0 && (
            <Button variant="secondary" onPress={async () => { await api('/api/v1/notifications/read-all', { method: 'POST' }); await load(); }}>
              <CheckCheck aria-hidden="true" size="1rem" /> Mark all as read
            </Button>
          )}
        </div>
      </div>
      {error ? <LoadError error={error} onRetry={() => void load()} what="notifications" /> : items === null ? <Skeleton rows={5} label="Loading notifications" /> : items.length === 0 ? (
        <Empty icon={Bell} title="No notifications yet">You’ll hear here when something needs you: a document to acknowledge, a review, or a new announcement.</Empty>
      ) : (
      <section className="card" aria-label="Notifications">
        {(
          <ul className="rows">
            {items.map((n) => {
              const k = KIND[n.kind] ?? { icon: Bell, tone: 'gray' };
              const target = n.link ?? (n.kind === 'announcement' ? '/announcements' : null);
              return (
                <li key={n.id} className="inbox-row rows-link">
                  <span className={n.read_at ? 'read-dot' : 'unread-dot'} aria-hidden="true" />
                  <span className={`task-icon tag-${k.tone}`}><k.icon aria-hidden="true" size="1rem" /></span>
                  <div className="row-main">
                    <p className="row-title" style={{ fontWeight: n.read_at ? 500 : 600 }}>
                      {!n.read_at && <span className="visually-hidden">Unread: </span>}
                      {target ? <Link to={target} onClick={() => { if (!n.read_at) void markRead(n.id); }}>{n.title}</Link> : n.title}
                    </p>
                    <p className="row-sub">{n.body}</p>
                  </div>
                  <span className="row-meta"><time dateTime={n.created_at}>{timeAgo(n.created_at)}</time></span>
                  {!n.read_at && (
                    <button type="button" className="icon-btn mark-read" onClick={() => void markRead(n.id)} aria-label={`Mark “${n.title}” as read`} title="Mark as read">
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
