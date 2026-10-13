import { ArrowLeft, ArrowUpRight, Check, Inbox, ListChecks, X } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { type Member, useItems, useResource } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { Link, navigate, useRoute } from '../lib/router';
import { type RequestDetail, type WorkItem, awayUntil, displayValue, dueInfo, normalizeDetail, outcomePast } from '../lib/requests';
import { setCrumb } from '../ui/AppShell';
import { Avatar } from '../ui/Avatar';
import { Notice } from '../ui/components/Notice';
import { SectionTabs } from '../ui/SectionTabs';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import { Stepper } from './RequestDetail';
import { DecisionPanel } from './workDecision';
import { BulkDecideDialog, bulkable } from './bulkDecide';
import { Button } from '../ui/components/Button';
import './requests.css';

/**
 * My work (BPMS-105, `/work`, `/work/{id}`): what's waiting for my decision. A list on the left, the
 * selected item on the right with the request's answers and the decision panel; on phones the two
 * are separate screens. J and K move through the list. Deciding removes the item from Open.
 */
export function MyWork({ selected }: { selected: string | null }) {
  const { query } = useRoute();
  const state = query.get('state') === 'done' ? 'done' : 'open';
  const [items, reload, error] = useItems<WorkItem>(`/api/v1/work?state=${state}`);
  // A decision (here or on a request page) changes the list even though its address stays the same.
  useEffect(() => { const on = () => reload(); window.addEventListener('procera:work-changed', on); return () => window.removeEventListener('procera:work-changed', on); }, [reload]);
  const kind = state === 'open' ? (KINDS.find((k) => k.id === query.get('kind'))?.id ?? 'all') : 'all';
  const suffix = state === 'done' ? '?state=done' : kind !== 'all' ? `?kind=${kind}` : '';
  const [picking, setPicking] = useState(false);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  // The items are fixed when the dialog opens, so its list stays put while decisions reload My work.
  const [bulk, setBulk] = useState<{ outcome: 'approved' | 'rejected'; items: WorkItem[] } | null>(null);
  const [bulkDone, setBulkDone] = useState<string | null>(null);
  const counts = useMemo(() => Object.fromEntries(KINDS.map((k) => [k.id, (items ?? []).filter((w) => k.id === 'all' || (w.kind ?? 'approval') === k.id).length])), [items]);
  const sorted = useMemo(() => [...(items ?? [])].filter((w) => kind === 'all' || (w.kind ?? 'approval') === kind).sort((a, b) => (state === 'open' ? due(a) - due(b) : Date.parse(b.decided_at ?? b.created_at) - Date.parse(a.decided_at ?? a.created_at))), [items, state, kind]);
  const current = sorted.find((w) => w.id === selected) ?? null;
  const decided = query.get('decided');
  const ref = query.get('ref');

  // J / K: next and previous item (FR-R03), unless typing.
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || !['j', 'k'].includes(e.key)) return;
      const t = e.target as HTMLElement;
      if (t.closest('input, textarea, select, [contenteditable="true"], [role="listbox"], [role="dialog"]')) return;
      if (!sorted.length) return;
      const i = sorted.findIndex((w) => w.id === selected);
      const next = sorted[Math.max(0, Math.min(sorted.length - 1, i < 0 ? 0 : i + (e.key === 'j' ? 1 : -1)))];
      if (next) { e.preventDefault(); navigate(`/work/${next.id}${suffix}`); requestAnimationFrame(() => (document.querySelector(`[data-work="${next.id}"]`) as HTMLElement | null)?.focus()); }
    };
    window.addEventListener('keydown', on);
    return () => window.removeEventListener('keydown', on);
  }, [sorted, selected, suffix]);

  return (
    <div className={selected ? 'work has-selection' : 'work'}>
      <div className="page-head work-head">
        <div><h1>My work</h1><p>Requests waiting for your decision. Decide here or open the full request.</p></div>
      </div>
      <SectionTabs label="Work views" tabs={[
        { label: 'Open', to: '/work', current: state === 'open' },
        { label: 'Done', to: '/work?state=done', current: state === 'done' },
        { label: 'Signatures', to: '/work/signatures', current: false },
      ]} />
      {decided && ref && !bulkDone && <Notice tone="success" title={`${outcomePast(decided)} ${ref}. It’s off your list.`} />}
      {bulk && <BulkDecideDialog items={bulk.items} outcome={bulk.outcome} onClose={() => setBulk(null)}
            onDone={(n) => { setBulk(null); setPicked(new Set()); setPicking(false); setBulkDone(`${bulk.outcome === 'approved' ? 'Approved' : 'Rejected'} ${n} ${n === 1 ? 'request' : 'requests'}.`); reload(); }} />}
      {bulkDone && <Notice tone="success" title={bulkDone} />}
      {state === 'open' && (items ?? []).length > 0 && (
        <div className="work-toolbar">
          <nav className="work-kinds" aria-label="Kinds of work">
            {KINDS.filter((k) => k.id === 'all' || counts[k.id]).map((k) => (
              <Link key={k.id} to={k.id === 'all' ? '/work' : `/work?kind=${k.id}`} aria-current={kind === k.id ? 'page' : undefined} className="work-kind">
                {k.label}<span className="work-kind-n">{counts[k.id]}</span>
              </Link>
            ))}
          </nav>
          {sorted.some(bulkable) && (picking ? (
            <span className="work-bulk" role="group" aria-label="Decide several">
              <span className="muted">{picked.size} selected</span>
              <Button className="btn-sm" isDisabled={!picked.size} onPress={() => setBulk({ outcome: 'approved', items: sorted.filter((w) => picked.has(w.id)) })}><Check aria-hidden="true" size="0.875rem" /> Approve {picked.size || ''}…</Button>
              <Button variant="secondary" className="btn-sm btn-reject" isDisabled={!picked.size} onPress={() => setBulk({ outcome: 'rejected', items: sorted.filter((w) => picked.has(w.id)) })}><X aria-hidden="true" size="0.875rem" /> Reject {picked.size || ''}…</Button>
              <Button variant="ghost" className="btn-sm" onPress={() => { setPicking(false); setPicked(new Set()); }}>Done</Button>
            </span>
          ) : <Button variant="secondary" className="btn-sm" onPress={() => { setPicking(true); setBulkDone(null); }}><ListChecks aria-hidden="true" size="0.875rem" /> Decide several</Button>)}
        </div>
      )}
      {error ? <LoadError error={error} onRetry={reload} what="your work" /> : items === null ? <Skeleton rows={4} label="Loading your work" /> : sorted.length === 0 && !selected ? (
        state === 'open'
          ? <Empty art="caught-up" title="Nothing is waiting for you">Approvals and tasks assigned to you land here. You’ll also get a notification.</Empty>
          : <Empty art="generic" title="Nothing decided yet">Items you approve, send back or reject are kept here.</Empty>
      ) : (
        <div className="work-panes">
          <nav className="work-list" aria-label={state === 'open' ? 'Open work' : 'Done work'}>
            <ul>
              {sorted.map((w) => {
                const d = state === 'open' ? (w.overdue ? { label: 'Overdue', color: 'red' as const, overdue: true } : dueInfo(w.due_at, w.overdue === false)) : null;
                return (
                  <li key={w.id} className={picking ? 'is-picking' : undefined}>
                    {picking && (
                      <span className="work-pick-box" title={bulkable(w) ? undefined : 'This one has something to fill in or confirm: decide it on its own.'}>
                        <input type="checkbox" aria-label={`Select ${w.request.display_id} ${w.request.title}`} disabled={!bulkable(w)} checked={picked.has(w.id)}
                          onChange={(e) => setPicked((p) => { const n = new Set(p); if (e.target.checked) n.add(w.id); else n.delete(w.id); return n; })} />
                      </span>
                    )}
                    <Link to={`/work/${w.id}${suffix}`} data-work={w.id} aria-current={w.id === selected ? 'page' : undefined} className="work-item">
                      <Avatar name={w.request.initiator.name} size="1.75rem" />
                      <span className="work-item-main">
                        <span className="work-item-title">{w.request.title}</span>
                        <span className="work-item-meta">{w.request.display_id} · {w.request.initiator.name} · {w.step}</span>
                        {w.on_behalf_of && <span className="work-item-meta"><Tag color="orange">Away</Tag> For {w.on_behalf_of.name}</span>}
                      </span>
                      <span className="work-item-side">
                        {d ? <Tag color={d.color} dot={d.overdue}>{d.label}</Tag> : state === 'done' && w.outcome ? <Tag>{outcomePast(w.outcome)}</Tag> : <time className="work-item-time" dateTime={w.created_at}>{timeAgo(w.created_at)}</time>}
                      </span>
                    </Link>
                  </li>
                );
              })}
            </ul>
          </nav>
          <div className="work-pane">
            {current ? <WorkPane key={current.id} item={current} open={state === 'open'} suffix={suffix} />
              : selected ? <Empty icon={Inbox} title="This item isn’t here any more">It may have been decided already, or moved to someone else.</Empty>
              : <div className="work-pick"><Inbox aria-hidden="true" size="1.25rem" /><p>Pick an item to see the request and decide.</p><p className="muted"><kbd>J</kbd> and <kbd>K</kbd> move through the list.</p></div>}
          </div>
        </div>
      )}
    </div>
  );
}

const KINDS = [
  { id: 'all', label: 'All' },
  { id: 'approval', label: 'Approvals' },
  { id: 'task', label: 'Tasks' },
  { id: 'acknowledge', label: 'Acknowledgements' },
] as const;

const due = (w: WorkItem) => (w.due_at ? Date.parse(w.due_at) : Number.MAX_SAFE_INTEGER);

function WorkPane({ item, open, suffix }: { item: WorkItem; open: boolean; suffix: string }) {
  const [raw, reload, error] = useResource<RequestDetail>(`/api/v1/requests/${item.request.id}`);
  const r = useMemo(() => (raw ? normalizeDetail(raw) : null), [raw]);
  const [members] = useItems<Member>(r?.form.fields.some((f) => f.type === 'principal') ? '/api/v1/members' : null, [r?.id]);
  const names = useMemo(() => new Map((members ?? []).map((m) => [m.id, m.display_name])), [members]);
  useEffect(() => { setCrumb(item.request.display_id); return () => setCrumb(null); }, [item]);
  return (
    <article className="work-detail" aria-labelledby={`work-${item.id}-title`}>
      <Link className="back-link work-back" to={`/work${suffix}`}><ArrowLeft aria-hidden="true" size="0.875rem" /> All work</Link>
      <p className="req-eyebrow"><span className="req-id">{item.request.display_id}</span> · {item.request.type_name}</p>
      <h2 id={`work-${item.id}-title`} className="work-detail-title">{item.request.title}</h2>
      <p className="work-detail-by"><Avatar name={item.request.initiator.name} size="1.25rem" /> From <strong>{item.request.initiator.name}</strong> · <time dateTime={item.created_at}>{shortDate(item.created_at)}</time>
        <Link className="section-link" to={`/requests/${item.request.id}`}>Open request <ArrowUpRight aria-hidden="true" size="0.875rem" /></Link></p>
      {error ? <LoadError error={error} onRetry={reload} what="this request" /> : !r ? <Skeleton rows={3} label="Loading the request" /> : (
        <>
          {r.form.fields.length > 0 && (
            <dl className="req-values req-values-compact" aria-label="Answers">
              {r.form.fields.map((f) => <div key={f.name} className={f.type === 'text' ? 'req-value req-value-wide' : 'req-value'}><dt>{f.label}</dt><dd>{displayValue(f, r.input[f.name], names)}</dd></div>)}
            </dl>
          )}
          {item.on_behalf_of && (
            <Notice tone="info" title={`You’re deciding this for ${item.on_behalf_of.name}.`}>
              {open ? `${awayUntil(item.on_behalf_of)}; you’re their delegate. The decision is recorded as yours, on their behalf.` : 'It was recorded as yours, on their behalf.'}
            </Notice>
          )}
          {open ? <DecisionPanel item={item} detail={r} onDecided={(o) => navigate(`/work?decided=${encodeURIComponent(o)}&ref=${encodeURIComponent(item.request.display_id)}`)} />
            : <Notice tone="info" title={item.outcome ? `You ${outcomePast(item.outcome).toLowerCase()} this${item.decided_at ? ` on ${shortDate(item.decided_at)}` : ''}.` : 'This step is finished.'} />}
          <details className="work-steps">
            <summary>Approval steps</summary>
            <Stepper steps={r.steps} />
          </details>
        </>
      )}
    </article>
  );
}
