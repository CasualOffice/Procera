import { useMemo, useState } from 'react';
import { useItems, useResource } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { Link, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import type { WorkItem } from '../lib/requests';
import { Button } from '../ui/components/Button';
import { DataGrid, type GridColumn } from '../ui/components/DataGrid';
import { Notice } from '../ui/components/Notice';
import { PersonAvatar } from '../ui/PersonAvatar';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import { dayText } from './outOfOffice';
import { ReassignDialog } from './reassign';
import './requests.css';

/*
 * Workload (FR-A03, docs/05 §3.18): per person, open steps, overdue, due this week and the oldest
 * waiting, with out-of-office cover. Managers see their reporting line on Team (`/team`); admins see
 * everyone on Requests oversight › Workload. Pick a person to see their steps and move one or many.
 */

interface Row { id: string; name: string; title: string | null; open: number; overdue: number; due_soon: number; oldest: string | null; away?: { until: string; delegate: string } }
interface Data { people: Row[]; totals: { people: number; open: number; overdue: number; due_soon: number; coverage_gaps: number } }

export function TeamPage() {
  return (
    <>
      <div className="page-head">
        <div><h1>Team</h1><p>What the people who report to you have waiting, who is away, and what is overdue. Move a step to keep it moving.</p></div>
      </div>
      <TeamWorkload scope="team" base="/team" />
    </>
  );
}

export function TeamWorkload({ scope, base }: { scope: 'team' | 'all'; base: string }) {
  const { query } = useRoute();
  const [data, reload, error] = useResource<Data>(`/api/v1/work/workload?scope=${scope}`);
  const person = query.get('person');
  const link = (id: string) => `${base}${base.includes('?') ? '&' : '?'}person=${id}`;
  const columns = useMemo<GridColumn<Row>[]>(() => [
    { id: 'name', header: 'Person', value: (r) => r.name, cell: (r) => (
      <span className="cell-entity">
        <PersonAvatar member={{ display_name: r.name, title: r.title, email: '', away: r.away }} card={false} />
        <span className="cell-stack"><Link to={link(r.id)} aria-current={r.id === person ? 'true' : undefined}>{r.name}</Link>{r.title && <span className="cell-sub">{r.title}</span>}</span>
      </span>
    ) },
    { id: 'open', header: 'Open', type: 'number', value: (r) => r.open, cell: (r) => <span className="num">{r.open}</span> },
    { id: 'overdue', header: 'Overdue', type: 'number', value: (r) => r.overdue, cell: (r) => (r.overdue ? <Tag color="red" dot>{r.overdue}</Tag> : <span className="cell-muted num">0</span>) },
    { id: 'soon', header: 'Due this week', type: 'number', value: (r) => r.due_soon, cell: (r) => <span className={r.due_soon ? 'num' : 'cell-muted num'}>{r.due_soon}</span> },
    { id: 'oldest', header: 'Oldest waiting', type: 'date', value: (r) => r.oldest ?? '', cell: (r) => (r.oldest ? <time className="cell-muted" dateTime={r.oldest} title={shortDate(r.oldest)}>{timeAgo(r.oldest)}</time> : <span className="cell-muted">—</span>) },
    { id: 'status', header: 'Status', value: (r) => (r.away ? 'away' : 'available'), cell: (r) => (r.away
      ? <span className="cell-stack"><span><Tag color="orange" dot>Away until {dayText(r.away.until)}</Tag></span><span className="cell-sub">{r.away.delegate} covers{r.open ? ` · ${r.open} still with them` : ''}</span></span>
      : <span className="cell-muted">Available</span>) },
  ], [person, base]);
  if (error) return <LoadError error={error} onRetry={reload} what={scope === 'team' ? 'your team’s work' : 'workload'} />;
  if (!data) return <Skeleton variant="table" rows={4} label="Loading workload" />;
  const t = data.totals;
  const chosen = data.people.find((p) => p.id === person) ?? null;
  return (
    <>
      <dl className="workload-stats" aria-label="Workload summary">
        <div><dt>People</dt><dd>{t.people}</dd></div>
        <div><dt>Open steps</dt><dd>{t.open}</dd></div>
        <div className={t.overdue ? 'is-bad' : ''}><dt>Overdue</dt><dd>{t.overdue}</dd></div>
        <div><dt>Due this week</dt><dd>{t.due_soon}</dd></div>
        <div className={t.coverage_gaps ? 'is-warn' : ''}><dt>Away with open steps</dt><dd>{t.coverage_gaps}</dd></div>
      </dl>
      <DataGrid<Row> caption={scope === 'team' ? 'Your team' : 'Everyone'} showCaption={false} data={data.people} columns={columns} initialSort={[{ id: 'overdue', desc: true }]}
        empty={scope === 'team'
          ? <Empty art="people" title="Nobody reports to you yet">When people list you as their manager (Admin › Members › a person › Manager), their work shows up here.</Empty>
          : <Empty art="people" title="No active members">Invite people to see their workload.</Empty>} />
      {chosen && <PersonSteps key={chosen.id} person={chosen} team={scope === 'team' ? data.people.map((p) => p.id) : undefined} onChanged={reload} />}
    </>
  );
}

function PersonSteps({ person, team, onChanged }: { person: Row; team?: string[]; onChanged: () => void }) {
  const { can, membershipId } = useSession();
  // Without the grant, a manager moves steps within their team (or takes them).
  const only = team && !can('work:reassign') ? [...team, ...(membershipId ? [membershipId] : [])] : undefined;
  const [items, reload, error] = useItems<WorkItem>(`/api/v1/work/workload/${person.id}`);
  const [moving, setMoving] = useState<{ rows: WorkItem[]; clear?: () => void } | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const columns = useMemo<GridColumn<WorkItem>[]>(() => [
    { id: 'request', header: 'Request', value: (w) => w.request.title, cell: (w) => (
      <span className="cell-stack"><Link to={`/requests/${w.request.id}`}>{w.request.title}</Link><span className="cell-sub">{w.request.display_id} · {w.request.type_name}</span></span>
    ) },
    { id: 'step', header: 'Step', value: (w) => w.step },
    { id: 'initiator', header: 'Requested by', value: (w) => w.request.initiator.name, optional: true },
    { id: 'waiting', header: 'Waiting since', type: 'date', value: (w) => w.created_at, cell: (w) => <time className="cell-muted" dateTime={w.created_at} title={shortDate(w.created_at)}>{timeAgo(w.created_at)}</time> },
    { id: 'due', header: 'Due', type: 'date', value: (w) => w.due_at ?? '', cell: (w) => (w.due_at ? (
      <span className="cell-stack"><time dateTime={w.due_at}>{shortDate(w.due_at)}</time>{w.overdue && <span><Tag color="red" dot>Overdue</Tag></span>}</span>
    ) : <span className="cell-muted">No due date</span>) },
    { id: 'actions', header: 'Actions', sortable: false, value: () => '', cell: (w) => (
      <Button variant="secondary" className="btn-sm" onPress={() => { setDone(null); setMoving({ rows: [w] }); }} aria-label={`Reassign ${w.step} of ${w.request.display_id}…`}>Reassign…</Button>
    ) },
  ], []);
  return (
    <section className="section-gap workload-person" aria-labelledby="person-steps">
      <div className="section-head">
        <h2 id="person-steps">Steps with {person.name}</h2>
      </div>
      {person.away && <Notice tone="warning" title={`${person.name} is away until ${dayText(person.away.until)}.`}>{person.away.delegate} can decide these on their behalf. Move the urgent ones if they can’t wait.</Notice>}
      {done && <Notice tone="success" title={done} />}
      {error ? <LoadError error={error} onRetry={reload} what={`${person.name}’s steps`} />
        : items === null ? <Skeleton variant="table" rows={2} label="Loading steps" />
        : (
          <DataGrid<WorkItem> caption={`Steps with ${person.name}`} showCaption={false} data={items} columns={columns} selectable rowLabel={(w) => `${w.request.display_id} ${w.step}`}
            initialSort={[{ id: 'due', desc: false }]}
            bulkActions={(sel, clear) => <Button variant="secondary" className="btn-sm" onPress={() => { setDone(null); setMoving({ rows: sel, clear }); }}>Reassign {sel.length}…</Button>}
            empty={<Empty art="caught-up" title={`${person.name} has nothing waiting`} compact>Steps offered to them show up here.</Empty>} />
        )}
      <ReassignDialog items={moving?.rows ?? []} exclude={person.id} only={only} onClose={() => setMoving(null)}
        onDone={(message) => { moving?.clear?.(); setMoving(null); setDone(message); reload(); onChanged(); }} />
    </section>
  );
}
