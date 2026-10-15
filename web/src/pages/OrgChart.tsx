import { ChevronDown, ChevronRight } from 'lucide-react';
import { useMemo, useState } from 'react';
import { useResource } from '../lib/data';
import { Avatar } from '../ui/Avatar';
import { TextField } from '../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { TimeOffTabs } from './areaTabs';
import './teamcalendar.css';

interface Node { id: string; name: string; title: string | null; department: string | null; location: string | null; manager_id: string | null; out_today: boolean }

/**
 * Org chart (BPMS-225, docs/25 §8): everyone by reporting line. Search finds a person and opens the
 * branches above them; people without a manager are the top of the tree.
 */
export function OrgChart() {
  const [data, reload, err] = useResource<{ items: Node[] }>('/api/v1/org-chart');
  const [q, setQ] = useState('');
  const [open, setOpen] = useState<Set<string>>(new Set());
  const { kids, roots, byId } = useMemo(() => {
    const items = data?.items ?? [];
    const byId = new Map(items.map((n) => [n.id, n]));
    const kids = new Map<string, Node[]>();
    for (const n of items) if (n.manager_id && byId.has(n.manager_id)) kids.set(n.manager_id, [...(kids.get(n.manager_id) ?? []), n]);
    const roots = items.filter((n) => !n.manager_id || !byId.has(n.manager_id));
    return { kids, roots, byId };
  }, [data]);
  // Search: matches, and every manager above a match, are shown open.
  const matches = useMemo(() => {
    const t = q.trim().toLowerCase();
    if (!t) return null;
    const hit = new Set<string>();
    const path = new Set<string>();
    for (const n of byId.values()) {
      if (`${n.name} ${n.title ?? ''} ${n.department ?? ''}`.toLowerCase().includes(t)) {
        hit.add(n.id);
        let m = n.manager_id;
        const seen = new Set<string>();
        while (m && byId.has(m) && !seen.has(m)) { path.add(m); seen.add(m); m = byId.get(m)!.manager_id; }
      }
    }
    return { hit, path };
  }, [q, byId]);
  if (err) return <LoadError error={err} onRetry={reload} what="the org chart" />;
  if (!data) return <Skeleton variant="list" label="Loading the org chart" />;
  const isOpen = (id: string, depth: number) => (matches ? matches.path.has(id) : open.has(id) !== depth < 1);
  const toggle = (id: string) => setOpen((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const count = (id: string): number => (kids.get(id) ?? []).reduce((a, k) => a + 1 + count(k.id), 0);
  const render = (n: Node, depth: number, seen: Set<string>): React.ReactNode => {
    if (seen.has(n.id)) return null;
    const next = new Set(seen).add(n.id);
    const children = kids.get(n.id) ?? [];
    if (matches && !matches.hit.has(n.id) && !matches.path.has(n.id)) return null;
    const o = children.length > 0 && isOpen(n.id, depth);
    return (
      <li key={n.id} role="treeitem" aria-expanded={children.length ? o : undefined} aria-selected={false} className="oc-item">
        <div className={`oc-card${matches?.hit.has(n.id) ? ' is-hit' : ''}`}>
          {children.length > 0 ? (
            <button type="button" className="oc-toggle" onClick={() => toggle(n.id)} aria-label={`${o ? 'Hide' : 'Show'} ${n.name}’s team`}>
              {o ? <ChevronDown aria-hidden="true" size="0.875rem" /> : <ChevronRight aria-hidden="true" size="0.875rem" />}
            </button>
          ) : <span className="oc-toggle-gap" />}
          <Avatar name={n.name} size="2rem" />
          <span className="oc-main">
            <span className="oc-name">{n.name}{n.out_today && <span className="tag tag-gray">Out today</span>}</span>
            <span className="oc-sub">{[n.title, n.department, n.location].filter(Boolean).join(' · ') || ' '}</span>
          </span>
          {children.length > 0 && <span className="oc-count">{count(n.id)} {count(n.id) === 1 ? 'person' : 'people'}</span>}
        </div>
        {o && <ul role="group" className="oc-group">{children.map((c) => render(c, depth + 1, next))}</ul>}
      </li>
    );
  };
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Org chart</h1>
          <p>{data.items.length} {data.items.length === 1 ? 'person' : 'people'} by reporting line. Managers are set in People.</p>
        </div>
      </div>
      <TimeOffTabs current="org" />
      <div className="oc-bar">
        <TextField label="Find someone" value={q} onChange={setQ} placeholder="Name, title or department" />
      </div>
      {roots.length === 0 ? <Empty title="Nobody here yet" /> : (
        <ul role="tree" aria-label="Org chart" className="oc-tree">{roots.map((r) => render(r, 0, new Set()))}</ul>
      )}
      {matches && matches.hit.size === 0 && <p className="muted">Nobody matches “{q}”.</p>}
    </>
  );
}
