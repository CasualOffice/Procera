import { AlertTriangle, CircleAlert } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { Link } from '../lib/router';
import { useSession } from '../lib/session';
import type { Campaign } from './Campaigns';

/*
 * Role cards on Home (docs/05 §3.1): what each job keeps an eye on, shown only to people who can
 * act on it. Each card is quiet when there's nothing to say.
 */

/** HR and compliance: campaigns in flight, with progress and what's overdue. */
function ActiveCampaigns() {
  const [items, setItems] = useState<Campaign[] | null>(null);
  useEffect(() => {
    api<{ items: Campaign[] }>('/api/v1/campaigns').then((r) => setItems(r.items.filter((c) => c.state === 'active').slice(0, 4))).catch(() => setItems([]));
  }, []);
  if (!items || items.length === 0) return null;
  return (
    <section aria-labelledby="home-campaigns" className="section-gap">
      <div className="section-head"><h2 id="home-campaigns">Active campaigns</h2><Link className="section-link" to="/admin/campaigns">All</Link></div>
      <ul className="home-cards">
        {items.map((c) => {
          const pct = c.progress.assigned ? Math.round((c.progress.acknowledged / c.progress.assigned) * 100) : 0;
          return (
            <li key={c.id}>
              <Link to={`/admin/campaigns/${c.id}`} className="home-card-title">{c.title}</Link>
              <div className="home-card-bar" role="img" aria-label={`${pct}% acknowledged`}><span style={{ width: `${pct}%` }} /></div>
              <p className="home-card-meta">{c.progress.acknowledged} of {c.progress.assigned} done{c.progress.overdue ? <> · <span className="home-card-warn">{c.progress.overdue} overdue</span></> : null}</p>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

interface Attention { kind: string; severity: 'high' | 'medium' | 'low'; count: number; title: string; link: string }

/** Admins and IT: the top of Admin › Overview's "Needs attention". */
function NeedsAttention() {
  const [items, setItems] = useState<Attention[] | null>(null);
  useEffect(() => {
    api<{ attention: Attention[] }>('/api/v1/admin/overview?days=30').then((r) => setItems(r.attention.slice(0, 3))).catch(() => setItems([]));
  }, []);
  if (!items || items.length === 0) return null;
  return (
    <section aria-labelledby="home-attention" className="section-gap">
      <div className="section-head"><h2 id="home-attention">Needs attention</h2><Link className="section-link" to="/admin/overview">Overview</Link></div>
      <ul className="home-attention">
        {items.map((a) => (
          <li key={`${a.kind}-${a.title}`}>
            {a.severity === 'high' ? <CircleAlert aria-hidden="true" size="0.9375rem" className="sev-high" /> : <AlertTriangle aria-hidden="true" size="0.9375rem" className="sev-medium" />}
            <Link to={a.link}>{a.title}</Link>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function RoleCards() {
  const { can } = useSession();
  return (
    <>
      {can('monitoring:view_dashboards') && <NeedsAttention />}
      {can('campaign:view') && <ActiveCampaigns />}
    </>
  );
}
