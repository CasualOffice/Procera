import { AlertOctagon, CalendarOff, Clock, RotateCcw, UserX, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { fieldErrors } from '../lib/requests';
import { Link } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../ui/States';
import './requests.css';

/*
 * Admin › Operations (docs/05 §3.18 "Ops monitor", PR-09): technical incidents (work the system gave
 * up on, retried or dismissed here with a reason) kept apart from business exceptions (steps with
 * nobody, overdue, people away holding work), which are fixed by moving work in Requests oversight.
 */

interface Incident { source: 'job' | 'timer'; id: string; what: string; request: { id: string; display_id: string; title: string } | null; attempts: number; error: string | null; failed_at: string | null }

export function Operations() {
  const { can } = useSession();
  const oversight = can('request:view_all') && can('work:reassign');
  return (
    <>
      <div className="page-head">
        <div><h1>Operations</h1><p>Incidents are the system failing: a step that couldn’t run, a reminder that couldn’t be sent. Retry them or dismiss them. Business exceptions are about people, and are fixed by moving work.</p></div>
      </div>
      <Incidents canAct={can('request:retry')} />
      {oversight && <Exceptions />}
    </>
  );
}

function Incidents({ canAct }: { canAct: boolean }) {
  const [items, reload, error] = useItems<Incident>('/api/v1/operations/incidents');
  const [acting, setActing] = useState<{ incident: Incident; kind: 'retry' | 'dismiss' } | null>(null);
  const [done, setDone] = useState<string | null>(null);
  return (
    <section className="section-gap" aria-labelledby="incidents-h">
      <div className="section-head"><h2 id="incidents-h">Incidents{items && items.length > 0 && <span className="count-badge" aria-label={`${items.length} open`}>{items.length}</span>}</h2></div>
      {done && <Notice tone="success" title={done} />}
      {error ? <LoadError error={error} onRetry={reload} what="incidents" />
        : items === null ? <Skeleton variant="table" rows={2} label="Loading incidents" />
        : items.length === 0 ? <Empty art="caught-up" title="No incidents" compact>When a step can’t run or a reminder keeps failing after its retries, it shows up here.</Empty>
        : (
          <ul className="incident-list" aria-label="Open incidents">
            {items.map((i) => (
              <li key={`${i.source}-${i.id}`} className="incident">
                <AlertOctagon aria-hidden="true" size="1.125rem" className="incident-icon" />
                <div className="incident-body">
                  <p className="incident-what">{i.what}</p>
                  <p className="incident-meta">
                    {i.request ? <Link to={`/requests/${i.request.id}`}>{i.request.display_id} · {i.request.title}</Link> : <span>No request</span>}
                    {' · '}{i.attempts} {i.attempts === 1 ? 'attempt' : 'attempts'}
                    {i.failed_at && <> · <time dateTime={i.failed_at} title={shortDate(i.failed_at)}>failed {timeAgo(i.failed_at)}</time></>}
                  </p>
                  {i.error && <code className="incident-error">{i.error}</code>}
                </div>
                {canAct && (
                  <span className="incident-actions">
                    <Button variant="secondary" className="btn-sm" onPress={() => { setDone(null); setActing({ incident: i, kind: 'retry' }); }} aria-label={`Retry: ${i.what}${i.request ? ` (${i.request.display_id})` : ''}`}><RotateCcw aria-hidden="true" size="0.875rem" /> Retry</Button>
                    <Button variant="ghost" className="btn-sm" onPress={() => { setDone(null); setActing({ incident: i, kind: 'dismiss' }); }} aria-label={`Dismiss: ${i.what}${i.request ? ` (${i.request.display_id})` : ''}`}><X aria-hidden="true" size="0.875rem" /> Dismiss</Button>
                  </span>
                )}
              </li>
            ))}
          </ul>
        )}
      {acting && <SettleDialog {...acting} onClose={() => setActing(null)} onDone={(m) => { setActing(null); setDone(m); reload(); }} />}
    </section>
  );
}

function SettleDialog({ incident, kind, onClose, onDone }: { incident: Incident; kind: 'retry' | 'dismiss'; onClose: () => void; onDone: (message: string) => void }) {
  const [reason, setReason] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const submit = async () => {
    if (!reason.trim()) { setError('Say why, for the audit log.'); return; }
    setBusy(true); setError(null); setFailure(null);
    try {
      await api(`/api/v1/operations/incidents/${incident.source}/${incident.id}/${kind}`, { method: 'POST', body: { reason: reason.trim() } });
      onDone(kind === 'retry' ? 'Retrying now. If it fails again it comes back here.' : 'Incident dismissed.');
    } catch (e) {
      const fe = fieldErrors(e);
      if (fe.reason) setError(fe.reason);
      else setFailure(e instanceof ApiError && e.status === 409 ? 'Someone handled this already. Close this and refresh.' : e instanceof Error ? e.message : 'Couldn’t do that. Try again.');
    } finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={kind === 'retry' ? 'Retry incident' : 'Dismiss incident'}
      description={kind === 'retry' ? `${incident.what}. It runs again now, with its attempts reset.` : `${incident.what}. Nothing runs again; the request stays where it is until someone moves it.`}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button variant={kind === 'dismiss' ? 'danger' : 'primary'} onPress={() => void submit()} isDisabled={busy}>{kind === 'retry' ? 'Retry' : 'Dismiss'}</Button></>}>
      {failure && <Notice tone="danger" title={failure} />}
      <TextField label="Reason" isRequired value={reason} onChange={setReason} isInvalid={!!error} errorMessage={error ?? undefined}
        description={kind === 'retry' ? 'For example “Mail server was down; back now”.' : 'For example “Request was cancelled by phone”.'} />
    </FormDialog>
  );
}

function Exceptions() {
  const [counts, setCounts] = useState<{ unassigned: number; overdue: number; away: number } | null>(null);
  useEffect(() => {
    const n = (path: string) => api<{ items: unknown[] }>(path).then((r) => r.items.length).catch(() => 0);
    void Promise.all([
      n('/api/v1/work?state=unassigned'), n('/api/v1/work?state=overdue'),
      api<{ totals: { coverage_gaps: number } }>('/api/v1/work/workload?scope=all').then((r) => r.totals.coverage_gaps).catch(() => 0),
    ]).then(([unassigned, overdue, away]) => setCounts({ unassigned, overdue, away }));
  }, []);
  const tiles = [
    { key: 'unassigned', label: 'Steps with nobody', hint: 'No approver could be found', icon: UserX, to: '/admin/requests?tab=unassigned', n: counts?.unassigned },
    { key: 'overdue', label: 'Overdue steps', hint: 'Past their due date', icon: Clock, to: '/admin/requests?tab=overdue', n: counts?.overdue },
    { key: 'away', label: 'Away with open steps', hint: 'Their delegate can decide them', icon: CalendarOff, to: '/admin/requests?tab=workload', n: counts?.away },
  ];
  return (
    <section className="section-gap" aria-labelledby="exceptions-h">
      <div className="section-head"><h2 id="exceptions-h">Business exceptions</h2><Link className="section-link" to="/admin/requests">Requests oversight</Link></div>
      <ul className="exception-tiles">
        {tiles.map((t) => (
          <li key={t.key} className={t.n ? 'has-some' : ''}>
            <Link to={t.to} aria-label={`${t.n ?? 'Loading'} ${t.label.toLowerCase()}: open in Requests oversight`}>
              <t.icon aria-hidden="true" size="1rem" />
              <span className="exception-n">{t.n ?? '–'}</span>
              <span className="exception-label">{t.label}</span>
              <span className="exception-hint">{t.hint}</span>
            </Link>
          </li>
        ))}
      </ul>
    </section>
  );
}
