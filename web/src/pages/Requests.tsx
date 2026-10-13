import { ArrowRight, CalendarDays, FileText, KeyRound, Laptop, Plane, Plus, Receipt, Send, ShoppingCart, UserPlus, Wallet, type LucideIcon } from 'lucide-react';
import { useMemo } from 'react';
import { useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { Link } from '../lib/router';
import { type RequestRow, type RequestType, STATUS_TAG, people, whereItIs } from '../lib/requests';
import { useSession } from '../lib/session';
import { DataGrid, type GridColumn } from '../ui/components/DataGrid';
import { SectionTabs } from '../ui/SectionTabs';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag, type TagColor } from '../ui/Tag';
import './requests.css';

export type RequestView = 'new' | 'mine' | 'all';

/** Requests views (docs/05 §2.1): start one, mine, and everyone's for people who may see them. */
export function RequestTabs({ current }: { current: RequestView }) {
  const { can } = useSession();
  return (
    <SectionTabs label="Request views" tabs={[
      can('request:start') && { label: 'New request', to: '/requests', current: current === 'new' },
      (can('request:view_own') || can('request:start')) && { label: 'My requests', to: '/requests?view=mine', current: current === 'mine' },
      can('request:view_all') && { label: 'All requests', to: '/requests?view=all', current: current === 'all' },
    ]} />
  );
}

/** A request type's icon from its catalogue hint (`calendar`, `receipt`…), with a plain fallback. */
const ICONS: Record<string, LucideIcon> = {
  calendar: CalendarDays, leave: CalendarDays, receipt: Receipt, expense: Receipt, wallet: Wallet, money: Wallet,
  laptop: Laptop, equipment: Laptop, cart: ShoppingCart, purchase: ShoppingCart, plane: Plane, travel: Plane,
  key: KeyRound, access: KeyRound, user: UserPlus, onboarding: UserPlus, document: FileText,
};
const TONES: TagColor[] = ['purple', 'blue', 'green', 'orange', 'pink', 'yellow', 'brown'];
export function typeIcon(t: Pick<RequestType, 'icon' | 'key'>): LucideIcon {
  return ICONS[t.icon ?? ''] ?? ICONS[t.key.split(/[._-]/)[0] ?? ''] ?? Send;
}
/** A stable pastel per category, so the same kind of request always looks the same. */
export function categoryTone(category: string): TagColor {
  let h = 0;
  for (const c of category) h = (h * 31 + c.charCodeAt(0)) >>> 0;
  return TONES[h % TONES.length]!;
}

/** Requests area (BPMS-103): the catalogue, my requests, and all requests. */
export function Requests({ view }: { view: RequestView }) {
  const { can } = useSession();
  const shown: RequestView = view === 'new' && !can('request:start') ? 'mine' : view === 'all' && !can('request:view_all') ? 'mine' : view;
  const intro = shown === 'new' ? 'Ask for time off, an expense, equipment or access. Pick what you need; the right people get it.' : shown === 'mine' ? 'Everything you’ve asked for, and where it is now.' : 'Every request in this workspace.';
  return (
    <>
      <div className="page-head">
        <div><h1>Requests</h1><p>{intro}</p></div>
      </div>
      <RequestTabs current={shown} />
      {shown === 'new' ? <Catalogue /> : <RequestList scope={shown} />}
    </>
  );
}

function Catalogue() {
  const [types, reload, error] = useItems<RequestType>('/api/v1/request-types');
  const groups = useMemo(() => {
    const by = new Map<string, RequestType[]>();
    for (const t of types ?? []) by.set(t.category || 'General', [...(by.get(t.category || 'General') ?? []), t]);
    return [...by.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [types]);
  if (error) return <LoadError error={error} onRetry={reload} what="request types" />;
  if (types === null) return <Skeleton variant="list" rows={4} label="Loading request types" />;
  if (types.length === 0) return <Empty art="templates" title="Nothing to request yet">When an admin publishes request types such as leave or expenses, they appear here.</Empty>;
  return (
    <div className="catalogue">
      {groups.map(([category, items]) => (
        <section key={category} aria-labelledby={`cat-${category}`} className="catalogue-group">
          <h2 id={`cat-${category}`} className="catalogue-label">{category}</h2>
          <ul className="catalogue-list">
            {items.map((t) => {
              const Icon = typeIcon(t);
              return (
                <li key={t.key} className="type-card">
                  <span className={`type-icon tag-${categoryTone(category)}`}><Icon aria-hidden="true" size="1.125rem" /></span>
                  <span className="type-text">
                    <Link className="type-name" to={`/requests/new/${encodeURIComponent(t.key)}`} aria-describedby={`type-${t.key}-desc`}>{t.name}</Link>
                    <span id={`type-${t.key}-desc`} className="type-desc">{t.description}</span>
                  </span>
                  <ArrowRight aria-hidden="true" size="1rem" className="type-go" />
                </li>
              );
            })}
          </ul>
        </section>
      ))}
    </div>
  );
}

function RequestList({ scope }: { scope: 'mine' | 'all' }) {
  const { can } = useSession();
  const [rows, reload, error] = useItems<RequestRow>(`/api/v1/requests?scope=${scope}`);
  const columns = useMemo<GridColumn<RequestRow>[]>(() => [
    { id: 'title', header: 'Request', value: (r) => r.title, cell: (r) => (
      <span className="cell-stack"><Link to={`/requests/${r.id}`}>{r.title}</Link><span className="cell-sub">{r.type_name}</span></span>
    ) },
    { id: 'id', header: 'Number', value: (r) => r.display_id, cell: (r) => <span className="req-id">{r.display_id}</span>, optional: true },
    ...(scope === 'all' ? [{ id: 'initiator', header: 'Requested by', value: (r: RequestRow) => r.initiator.name } satisfies GridColumn<RequestRow>] : []),
    { id: 'status', header: 'Status', type: 'enum', value: (r) => STATUS_TAG[r.status]?.label ?? r.status, cell: (r) => <Tag color={STATUS_TAG[r.status]?.color} dot>{STATUS_TAG[r.status]?.label ?? r.status}</Tag> },
    { id: 'step', header: 'Where it is', value: (r) => (r.status === 'in_progress' || r.status === 'changes_requested' ? `${(r.open_steps ?? []).join(' ') || (r.current_step ?? '')} ${people(r.assignees)}` : ''), cell: (r) => (r.status === 'in_progress' || r.status === 'changes_requested') && r.current_step ? (
      <span className="cell-stack"><span>{whereItIs(r)}</span>{r.assignees.length > 0 && <span className="cell-sub">with {people(r.assignees)}</span>}</span>
    ) : <span className="cell-muted">Finished</span>, optional: true },
    { id: 'updated', header: 'Updated', type: 'date', value: (r) => r.updated_at, cell: (r) => <time className="cell-muted" dateTime={r.updated_at} title={shortDate(r.updated_at)}>{timeAgo(r.updated_at)}</time> },
  ], [scope]);
  if (error) return <LoadError error={error} onRetry={reload} what="requests" />;
  if (rows === null) return <Skeleton variant="table" rows={4} label="Loading requests" />;
  return (
    <DataGrid<RequestRow> caption={scope === 'mine' ? 'My requests' : 'All requests'} showCaption={false} data={rows} columns={columns} rowHref={(r) => `/requests/${r.id}`} initialSort={[{ id: 'updated', desc: true }]}
      empty={scope === 'mine' ? (
        <Empty art="generic" title="You haven’t asked for anything yet" action={can('request:start') ? <Link className="btn btn-primary btn-md link-btn" to="/requests"><Plus aria-hidden="true" size="1rem" /> New request</Link> : undefined}>Requests you make show up here with where they are and who has them.</Empty>
      ) : <Empty art="generic" title="No requests yet">Requests from everyone in this workspace show up here.</Empty>} />
  );
}
