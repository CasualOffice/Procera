import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import { Link } from '../lib/router';
import { Button } from '../ui/components/Button';

interface Notification { id: string; title: string; body: string; link: string | null; read_at: string | null; created_at: string }

/** The member's inbox (FR-N01). */
export function Notifications() {
  const [items, setItems] = useState<Notification[] | null>(null);
  const load = useCallback(() => api<{ items: Notification[] }>('/api/v1/notifications').then((r) => setItems(r.items)), []);
  useEffect(() => { void load(); }, [load]);
  const unread = items?.filter((n) => !n.read_at).length ?? 0;
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Notifications</h1>
          <p className="lede">{unread ? `${unread} unread` : 'All caught up'}</p>
        </div>
        {unread > 0 && <Button variant="secondary" onPress={async () => { await api('/api/v1/notifications/read-all', { method: 'POST' }); await load(); }}>Mark all as read</Button>}
      </div>
      {items === null ? <p className="muted">Loading…</p> : items.length === 0 ? <p className="muted">No notifications yet.</p> : (
        <ul className="feed">
          {items.map((n) => (
            <li key={n.id} className={n.read_at ? 'feed-item' : 'feed-item feed-unread'}>
              <div>
                <p className="feed-title">{!n.read_at && <span className="visually-hidden">Unread: </span>}{n.link ? <Link to={n.link}>{n.title}</Link> : n.title}</p>
                <p className="muted">{n.body} · <time dateTime={n.created_at}>{new Date(n.created_at).toLocaleString()}</time></p>
              </div>
              {!n.read_at && <Button variant="ghost" onPress={async () => { await api(`/api/v1/notifications/${n.id}/read`, { method: 'POST' }); await load(); }} aria-label={`Mark “${n.title}” as read`}>Mark read</Button>}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
