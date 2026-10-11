import { AuditExports } from './auditExports';
import { ChevronRight, ShieldAlert, ShieldCheck } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { api } from '../lib/api';
import { type LoadFailure, type Member, toFailure, useItems, useResource } from '../lib/data';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { Drawer } from '../ui/Drawer';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag, type TagColor } from '../ui/Tag';
import { useRoute } from '../lib/router';

export interface AuditEvent {
  seq: number; id: string; occurred_at: string; actor_subject_ref: string | null; actor_kind: string; actor_name: string | null; on_behalf_of: string | null;
  ip: string | null; user_agent: string | null; category: string; action: string; object_type: string | null; object_id: string | null;
  result: string; reason: string | null; policy_ids: string[]; diff: unknown; trace_id: string | null; hash: string;
}
interface Page { items: AuditEvent[]; next_cursor: string | null }
interface Filters { category: string; result: string; action: string; member: string; from: string; to: string }

const CATEGORIES: [string, string][] = [['', 'All categories'], ['auth', 'Sign-in'], ['access', 'Access'], ['config', 'Configuration'], ['content', 'Content'], ['runtime', 'Requests'], ['file', 'Files'], ['data_access', 'Data access'], ['export', 'Exports'], ['admin', 'Administration'], ['system', 'System']];
const RESULT: Record<string, { label: string; color: TagColor }> = { allowed: { label: 'Allowed', color: 'green' }, denied: { label: 'Denied', color: 'red' }, error: { label: 'Error', color: 'orange' } };
const EMPTY: Filters = { category: '', result: '', action: '', member: '', from: '', to: '' };

/** "artifact_version.published" → "Artifact version published". */
export function actionText(a: string): string {
  const s = a.replace(/[._]/g, ' ');
  return s.charAt(0).toUpperCase() + s.slice(1);
}
const time = (iso: string) => new Date(iso).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit', second: '2-digit' });

function query(f: Filters, cursor?: string | null) {
  const p = new URLSearchParams();
  if (f.category) p.set('category', f.category);
  if (f.result) p.set('result', f.result);
  if (f.action.trim()) p.set('action', f.action.trim());
  if (f.member) p.set('member', f.member);
  if (f.from) p.set('from', new Date(`${f.from}T00:00:00`).toISOString());
  if (f.to) p.set('to', new Date(`${f.to}T23:59:59`).toISOString());
  p.set('page[size]', '50');
  if (cursor) p.set('page[cursor]', cursor);
  return p.toString();
}

/** Where an explorer reads from: the workspace chain, or the system chain in the system console. */
export interface AuditSource {
  title: string;
  lede: string;
  /** Events endpoint (takes the explorer's query string). */
  events: string;
  verify: string | null;
  /** One object's events; `newestFirst` when the endpoint returns them newest first. */
  timeline: (type: string, id: string) => string;
  newestFirst?: boolean;
  people?: Member[] | null;
  ownOnly?: boolean;
  actionHint: string;
  /** Offer background exports (workspace audit log with `audit:export`). */
  exportable?: boolean;
}

/**
 * Admin · Audit log (docs/05 §3.19): read-only, newest first, filters, keyset paging, a row expands
 * to the full record, and any object opens its timeline. Chain verification shows at the top.
 */
export function Audit() {
  const { can } = useSession();
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  return (
    <AuditExplorer source={{
      title: 'Audit log',
      lede: 'Every change and sensitive read in this workspace, newest first. Records can’t be edited or deleted.',
      events: '/api/v1/audit/events',
      verify: can('audit:view') ? '/api/v1/audit/verify' : null,
      timeline: (type, id) => `/api/v1/audit/objects/${encodeURIComponent(type)}/${id}/timeline`,
      people: members,
      ownOnly: !can('audit:view'),
      actionHint: 'announcement.published',
      exportable: can('audit:view') && can('audit:export'),
    }} />
  );
}

/** The shared explorer component (docs/05 §3.20: the system console reuses it on the system chain). */
export function AuditExplorer({ source }: { source: AuditSource }) {
  // Deep links (e.g. from the overview) open pre-filtered: ?action=…&from=YYYY-MM-DD&to=…&category=…
  const { query: q } = useRoute();
  const fromUrl: Filters = { ...EMPTY, action: q.get('action') ?? '', from: q.get('from') ?? '', to: q.get('to') ?? '', category: q.get('category') ?? '' };
  const [draft, setDraft] = useState<Filters>(fromUrl);
  const [filters, setFilters] = useState<Filters>(fromUrl);
  const [items, setItems] = useState<AuditEvent[] | null>(null);
  const [cursor, setCursor] = useState<string | null>(null);
  const [error, setError] = useState<LoadFailure | null>(null);
  const [more, setMore] = useState(false);
  const [open, setOpen] = useState<number | null>(null);
  const [timeline, setTimeline] = useState<{ type: string; id: string } | null>(null);
  const members = source.people;
  const [chain] = useResource<{ ok: boolean; checked: number; first_broken: number | null }>(source.verify);

  const load = useCallback(async (f: Filters, after?: string | null) => {
    try {
      const r = await api<Page>(`${source.events}?${query(f, after)}`);
      setItems((cur) => (after ? [...(cur ?? []), ...r.items] : r.items));
      setCursor(r.next_cursor);
      setError(null);
    } catch (e) { setError(toFailure(e)); if (!after) setItems([]); }
  }, [source.events]);
  useEffect(() => { setItems(null); setOpen(null); void load(filters); }, [filters, load]);
  const active = Object.values(filters).filter(Boolean).length;

  return (
    <>
      <div className="page-head">
        <div><h1>{source.title}</h1><p>{source.lede}</p></div>
        <div className="page-actions">
        {source.exportable && <AuditExports filters={filters} />}
        {chain && (chain.ok
          ? <span className="chain chain-ok"><ShieldCheck aria-hidden="true" size="0.9375rem" /> Chain verified <span className="muted">· {chain.checked.toLocaleString()} events</span></span>
          : <span className="chain chain-bad" role="alert"><ShieldAlert aria-hidden="true" size="0.9375rem" /> Chain broken at event {chain.first_broken}</span>)}
        </div>
      </div>
      {source.ownOnly && <p className="muted audit-own">You can see your own activity only.</p>}

      <form className="audit-filters" aria-label="Filter events" onSubmit={(e) => { e.preventDefault(); setFilters(draft); }}>
        <label className="af"><span>Category</span>
          <select className="field-input" value={draft.category} onChange={(e) => { const d = { ...draft, category: e.target.value }; setDraft(d); setFilters(d); }}>
            {CATEGORIES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
          </select>
        </label>
        <label className="af"><span>Result</span>
          <select className="field-input" value={draft.result} onChange={(e) => { const d = { ...draft, result: e.target.value }; setDraft(d); setFilters(d); }}>
            <option value="">Any result</option><option value="allowed">Allowed</option><option value="denied">Denied</option><option value="error">Error</option>
          </select>
        </label>
        {members && members.length > 0 && (
          <label className="af"><span>Person</span>
            <select className="field-input" value={draft.member} onChange={(e) => { const d = { ...draft, member: e.target.value }; setDraft(d); setFilters(d); }}>
              <option value="">Anyone</option>
              {members.map((m) => <option key={m.id} value={m.id}>{m.display_name}</option>)}
            </select>
          </label>
        )}
        <label className="af af-wide"><span>Action</span><input className="field-input" placeholder={source.actionHint} value={draft.action} onChange={(e) => setDraft({ ...draft, action: e.target.value })} /></label>
        <label className="af"><span>From</span><input type="date" className="field-input" value={draft.from} onChange={(e) => setDraft({ ...draft, from: e.target.value })} /></label>
        <label className="af"><span>To</span><input type="date" className="field-input" value={draft.to} onChange={(e) => setDraft({ ...draft, to: e.target.value })} /></label>
        <div className="af-actions">
          <Button type="submit" variant="secondary">Apply</Button>
          {active > 0 && <Button variant="ghost" onPress={() => { setDraft(EMPTY); setFilters(EMPTY); }}>Clear</Button>}
        </div>
      </form>

      {error && items?.length === 0 ? <LoadError error={error} onRetry={() => void load(filters)} what="the audit log" /> : items === null ? <Skeleton variant="table" rows={8} label="Loading events" /> : items.length === 0 ? (
        active ? <Empty art="no-match" title="No events match these filters" action={<Button variant="secondary" onPress={() => { setDraft(EMPTY); setFilters(EMPTY); }}>Clear filters</Button>} />
          : <Empty art="audit" title="No events yet">Sign-ins, changes and exports will be recorded here as they happen.</Empty>
      ) : (
        <>
          <p className="muted audit-count" role="status">{items.length}{cursor ? '+' : ''} {items.length === 1 && !cursor ? 'event' : 'events'}</p>
          <div className="table-wrap">
            <table className="table audit-table" aria-label="Audit events">
              <thead><tr><th>When</th><th className="col-optional">Who</th><th>What</th><th className="col-optional">Object</th><th>Result</th></tr></thead>
              <tbody>
                {items.map((e) => (
                  <EventRow key={e.seq} e={e} open={open === e.seq} onToggle={() => setOpen(open === e.seq ? null : e.seq)} onTimeline={(type, id) => setTimeline({ type, id })} />
                ))}
              </tbody>
            </table>
          </div>
          <div className="audit-more">
            <span className="muted">{items.length} {items.length === 1 ? 'event' : 'events'} shown</span>
            {cursor && <Button variant="secondary" isDisabled={more} onPress={async () => { setMore(true); await load(filters, cursor); setMore(false); }}>{more ? 'Loading…' : 'Load older events'}</Button>}
          </div>
        </>
      )}
      {timeline && <Timeline type={timeline.type} id={timeline.id} url={source.timeline(timeline.type, timeline.id)} newestFirst={source.newestFirst} onClose={() => setTimeline(null)} />}
    </>
  );
}

function EventRow({ e, open, onToggle, onTimeline }: { e: AuditEvent; open: boolean; onToggle: () => void; onTimeline: (type: string, id: string) => void }) {
  const who = e.actor_name ?? (e.actor_kind === 'system' ? 'System' : e.actor_kind === 'anonymous' ? 'Someone signed out' : 'Unknown');
  return (
    <>
      <tr className={open ? 'audit-open' : ''}>
        <td className="cell-muted"><button type="button" className="audit-toggle" aria-expanded={open} onClick={onToggle} aria-label={`${actionText(e.action)} at ${time(e.occurred_at)}: details`}><ChevronRight aria-hidden="true" size="0.875rem" /></button><time dateTime={e.occurred_at}>{time(e.occurred_at)}</time></td>
        <td className="col-optional">{e.actor_name ? <span className="person"><Avatar name={e.actor_name} size="1.25rem" />{e.actor_name}</span> : <span className="cell-muted">{who}</span>}</td>
        <td><span className="audit-action">{actionText(e.action)}</span><span className="audit-who-sm">{who}</span></td>
        <td className="col-optional">{e.object_type && e.object_id ? <button type="button" className="audit-obj" onClick={() => onTimeline(e.object_type!, e.object_id!)} title="Open this object’s timeline">{e.object_type.replace(/_/g, ' ')} <code>{e.object_id.slice(-6)}</code></button> : <span className="cell-muted" />}</td>
        <td>{e.result === 'allowed' ? <span className="cell-muted">Allowed</span> : <Tag color={RESULT[e.result]?.color} dot>{RESULT[e.result]?.label ?? e.result}</Tag>}</td>
      </tr>
      {open && (
        <tr className="audit-detail"><td colSpan={5}>
          <dl className="audit-dl">
            <dt>Action</dt><dd><code>{e.action}</code> <span className="muted">· {e.category}</span></dd>
            {e.reason && <><dt>Reason</dt><dd>{e.reason}</dd></>}
            {e.object_type && <><dt>Object</dt><dd><code>{e.object_type}/{e.object_id}</code></dd></>}
            {e.ip && <><dt>From</dt><dd>{e.ip}{e.user_agent ? <span className="muted"> · {e.user_agent}</span> : null}</dd></>}
            {e.policy_ids.length > 0 && <><dt>Policies</dt><dd>{e.policy_ids.map((p) => <code key={p}>{p}</code>)}</dd></>}
            {e.trace_id && <><dt>Trace</dt><dd><code>{e.trace_id}</code></dd></>}
            <dt>Record</dt><dd><code>#{e.seq}</code> <span className="muted">· hash</span> <code title={e.hash}>{e.hash.slice(0, 16)}…</code></dd>
          </dl>
          {e.diff != null && <><p className="audit-diff-h">Change</p><pre className="audit-diff">{JSON.stringify(e.diff, null, 2)}</pre></>}
        </td></tr>
      )}
    </>
  );
}

/** Every event for one object, oldest first (docs/05 §3.19 "per object" timeline). */
export function Timeline({ type, id, url, newestFirst = false, onClose }: { type: string; id: string; url: string; newestFirst?: boolean; onClose: () => void }) {
  const [raw, , error] = useResource<Page>(url);
  const page = raw && newestFirst ? { ...raw, items: [...raw.items].reverse() } : raw;
  return (
    <Drawer title={`${actionText(type)} history`} onClose={onClose}>
      <p className="drawer-next"><code className="mono">{type}/{id}</code></p>
      {error ? <LoadError error={error} what="this timeline" /> : page === null ? <Skeleton rows={4} label="Loading timeline" /> : page.items.length === 0 ? <Empty compact art="audit" title="No events for this object" /> : (
        <ol className="tl">
          {page.items.map((e) => (
            <li key={e.seq} className={e.result !== 'allowed' ? 'tl-bad' : ''}>
              <span className="tl-dot" aria-hidden="true" />
              <div>
                <p className="tl-title">{actionText(e.action)}{e.result !== 'allowed' && <> <Tag color={RESULT[e.result]?.color}>{RESULT[e.result]?.label}</Tag></>}</p>
                <p className="tl-meta">{e.actor_name ?? 'System'} · <time dateTime={e.occurred_at}>{time(e.occurred_at)}</time></p>
                {e.reason && <p className="tl-reason">{e.reason}</p>}
              </div>
            </li>
          ))}
        </ol>
      )}
    </Drawer>
  );
}
