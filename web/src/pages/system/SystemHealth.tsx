import { CircleAlert, CircleCheck, RotateCw, TriangleAlert } from 'lucide-react';
import { useResource } from '../../lib/data';
import { timeAgo } from '../../lib/format';
import { Button } from '../../ui/components/Button';
import { LoadError, Skeleton } from '../../ui/States';

type Status = 'ok' | 'degraded' | 'down';
interface Health {
  status: Status;
  checked_at: string;
  version: string;
  database: { status: Status; latency_ms: number };
  cache: { status: Status; backend: string };
  outbox: { status: Status; pending: number; retrying: number; oldest_at: string | null; lag_seconds: number; dispatcher: string };
  mail: { status: Status; queued: number; failed: number; oldest_age_seconds: number };
  migrations: { status: Status; applied: number | null; latest: number | null; expected: number; expected_latest: number | null };
}

const LABEL: Record<Status, string> = { ok: 'Healthy', degraded: 'Needs a look', down: 'Down' };
const ICON = { ok: CircleCheck, degraded: TriangleAlert, down: CircleAlert } as const;
const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString()} ${n === 1 ? one : many}`;
const secs = (s: number) => (s < 60 ? `${s} s` : s < 3600 ? `${Math.floor(s / 60)} min` : `${Math.floor(s / 3600)} h`);

function Row({ name, status, children }: { name: string; status: Status; children: React.ReactNode }) {
  const Icon = ICON[status];
  return (
    <div className="health-row">
      <dt><Icon aria-hidden="true" size="1rem" className={`health-${status}`} />{name}</dt>
      <dd><strong>{LABEL[status]}</strong> · {children}</dd>
    </div>
  );
}

/** System · Platform health (docs/05 §3.20, FR-S08): one row per dependency, worst first in the summary. */
export function SystemHealth() {
  const [h, reload, error] = useResource<Health>('/sysapi/v1/health');
  if (error) return <LoadError error={error} onRetry={reload} what="platform health" />;
  const SummaryIcon = h ? ICON[h.status] : CircleCheck;
  return (
    <>
      <div className="page-head">
        <div><h1>Platform health</h1><p>Database, cache, background delivery, email and schema. Totals only, across all workspaces.</p></div>
        <Button variant="secondary" onPress={reload}><RotateCw aria-hidden="true" size="0.875rem" /> Check again</Button>
      </div>
      {h === null ? <Skeleton rows={6} label="Checking platform health" /> : (
        <>
          <p className="health-summary" role="status">
            <SummaryIcon aria-hidden="true" size="1.25rem" className={`health-${h.status}`} />
            {h.status === 'ok' ? 'Everything is running normally.' : h.status === 'down' ? 'The database is unreachable.' : 'Something needs a look.'}
            <span className="muted"> Checked {timeAgo(h.checked_at)} · version {h.version}</span>
          </p>
          <dl className="health-list" aria-label="Health checks">
            <Row name="Database" status={h.database.status}>{h.database.status === 'ok' ? `responded in ${h.database.latency_ms} ms` : 'not responding'}</Row>
            <Row name="Cache" status={h.cache.status}>{h.cache.backend === 'memory' ? 'in-process cache' : h.cache.backend}{h.cache.status !== 'ok' ? ', running on fallbacks' : ''}</Row>
            <Row name="Event delivery" status={h.outbox.status}>
              {h.outbox.pending === 0 ? 'nothing waiting' : `${plural(h.outbox.pending, 'event')} waiting, oldest ${secs(h.outbox.lag_seconds)}`}
              {h.outbox.retrying > 0 && `, ${plural(h.outbox.retrying, 'retry', 'retries')}`}
              {h.outbox.dispatcher !== 'running' && ', no dispatcher in this process'}
            </Row>
            <Row name="Email queue" status={h.mail.status}>
              {h.mail.queued === 0 ? 'nothing queued' : `${plural(h.mail.queued, 'message')} queued, oldest ${secs(h.mail.oldest_age_seconds)}`}
              {h.mail.failed > 0 && `, ${plural(h.mail.failed, 'message')} failed`}
            </Row>
            <Row name="Schema" status={h.migrations.status}>
              {h.migrations.applied === null ? 'unknown' : `${h.migrations.applied} of ${h.migrations.expected} migrations applied`}
              {h.migrations.latest !== null && ` (latest ${String(h.migrations.latest).padStart(4, '0')})`}
            </Row>
          </dl>
        </>
      )}
    </>
  );
}
