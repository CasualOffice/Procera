import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { Link } from '../lib/router';
import { useSession } from '../lib/session';

interface Notification { id: string; title: string; body: string; link: string | null; created_at: string }
interface Artifact { id: string; kind: string; title: string; state: string }

/** Home (docs/05 §3.1): what needs me today. */
export function Home() {
  const { user, workspace, can } = useSession();
  const [notes, setNotes] = useState<Notification[] | null>(null);
  const [recent, setRecent] = useState<Artifact[] | null>(null);

  useEffect(() => {
    api<{ items: Notification[] }>('/api/v1/notifications?unread=true&limit=5').then((r) => setNotes(r.items)).catch(() => setNotes([]));
    if (can('policy:view') || can('sop:view')) {
      api<{ items: Artifact[] }>('/api/v1/artifacts?state=published').then((r) => setRecent(r.items.slice(0, 5))).catch(() => setRecent([]));
    } else setRecent([]);
  }, [can]);

  const admin = can('member:invite');
  return (
    <>
      <h1>Home</h1>
      <p className="lede">{workspace?.name} · signed in as {user?.display_name}</p>

      <section aria-labelledby="needs">
        <h2 id="needs">Needs your attention</h2>
        {notes === null ? <p className="muted">Loading…</p> : notes.length === 0 ? <p className="muted">Nothing new. You’re up to date.</p> : (
          <ul className="plain-list">
            {notes.map((n) => (
              <li key={n.id}>
                {n.link ? <Link to={n.link}>{n.title}</Link> : <strong>{n.title}</strong>}
                <span className="muted"> — {n.body}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      {admin && (
        <section aria-labelledby="start">
          <h2 id="start">Get started</h2>
          <ol className="plain-list">
            <li><Link to="/admin/members">Invite your team</Link> — they get an email with a link to join.</li>
            <li><Link to="/library">Publish your first policy</Link> — draft it, review it, publish it.</li>
          </ol>
        </section>
      )}

      {recent && recent.length > 0 && (
        <section aria-labelledby="recent">
          <h2 id="recent">Published in the library</h2>
          <ul className="plain-list">
            {recent.map((a) => (
              <li key={a.id}><Link to={`/library/${a.id}`}>{a.title}</Link> <span className="muted">{a.kind === 'sop' ? 'SOP' : 'Policy'}</span></li>
            ))}
          </ul>
        </section>
      )}
    </>
  );
}
