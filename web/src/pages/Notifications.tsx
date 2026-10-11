import { Bell, CheckCheck, UserPlus } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import { Link } from '../lib/router';
import { Button } from '../ui/components/Button';
import { timeAgo } from './Home';

interface Notification { id: string; kind: string; title: string; body: string; link: string | null; read_at: string | null; created_at: string }

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
          <p>{unread ? `${unread} unread` : 'All caught up'}</p>
        </div>
        {unread > 0 && (
          <Button variant="secondary" onPress={async () => { await api('/api/v1/notifications/read-all', { method: 'POST' }); await load(); }}>
            <CheckCheck aria-hidden="true" size={16} /> Mark all as read
          </Button>
        )}
      </div>
      <section className="card" aria-label="Notifications">
        {items === null ? <div className="card-empty">Loading…</div> : items.length === 0 ? <div className="card-empty">No notifications yet.</div> : (
          <ul className="rows">
            {items.map((n) => (
              <li key={n.id}>
                <span className={n.read_at ? 'read-dot' : 'unread-dot'} aria-hidden="true" />
                <span className="row-icon">{n.kind === 'member_joined' ? <UserPlus aria-hidden="true" size={16} /> : <Bell aria-hidden="true" size={16} />}</span>
                <div className="row-main">
                  <p className="row-title" style={{ fontWeight: n.read_at ? 500 : 600 }}>
                    {!n.read_at && <span className="visually-hidden">Unread: </span>}
                    {n.link ? <Link to={n.link} style={{ textDecoration: 'none' }}>{n.title}</Link> : n.title}
                  </p>
                  <p className="row-sub">{n.body}</p>
                </div>
                <span className="row-meta"><time dateTime={n.created_at}>{timeAgo(n.created_at)}</time></span>
                {!n.read_at && (
                  <Button variant="secondary" className="btn-sm" onPress={async () => { await api(`/api/v1/notifications/${n.id}/read`, { method: 'POST' }); await load(); }} aria-label={`Mark “${n.title}” as read`}>
                    Mark read
                  </Button>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>
    </>
  );
}
