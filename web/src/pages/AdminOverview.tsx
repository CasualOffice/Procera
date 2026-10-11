import { AlertTriangle, ArrowUpRight, CircleAlert, Info } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { Link, navigate, useRoute } from '../lib/router';
import { Tag } from '../ui/Tag';
import '../ui/admin.css';
import { Skeleton } from '../ui/States';

interface Attention { kind: string; severity: 'high' | 'medium' | 'low'; count: number; title: string; link: string }
interface Overview {
  range: { from: string; to: string; department: string | null };
  summary: { overdue_acknowledgements: number; overdue_reviews: number; pending_reviews: number; pending_announcements: number };
  attention: Attention[];
  metrics: {
    acknowledgements: { assigned: number; completed: number; pending: number; completion_rate: number | null };
    published_versions: { total: number; bucket: 'day' | 'week'; series: { at: string; count: number }[] };
    announcements_published: number;
  };
  security: { failed_logins: number; members_without_mfa: number; new_admins: number; break_glass_sessions: number };
  departments: { id: string; name: string; members: number; overdue_acknowledgements: number }[];
  requests: null; storage: null;
}

const RANGES = [['7', 'Last 7 days'], ['30', 'Last 30 days'], ['90', 'Last 90 days']] as const;
const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Admin · Overview (docs/05 §3.13): a typographic summary, a ranked "Needs attention" list, compact metrics, and a security strip. */
export function AdminOverview() {
  const { query } = useRoute();
  const days = query.get('range') ?? '7';
  const department = query.get('department');
  const [data, setData] = useState<Overview | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setData(null); setFailed(false);
    // The server computes the range on its own clock.
    const qs = new URLSearchParams({ days });
    if (department) qs.set('department', department);
    api<Overview>(`/api/v1/admin/overview?${qs}`).then(setData).catch(() => setFailed(true));
  }, [days, department]);

  const setParam = (k: string, v: string | null) => {
    const q = new URLSearchParams(query);
    if (v) q.set(k, v); else q.delete(k);
    navigate(`/admin/overview${q.toString() ? `?${q}` : ''}`, { replace: true });
  };

  return (
    <>
      <div className="page-head">
        <div><h1>Overview</h1><p>What needs your attention in this workspace.</p></div>
        <div className="overview-filters">
          <label className="select-inline"><span className="visually-hidden">Time range</span>
            <select aria-label="Time range" value={days} onChange={(e) => setParam('range', e.target.value === '7' ? null : e.target.value)}>
              {RANGES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </label>
          {data && data.departments.length > 0 && (
            <label className="select-inline"><span className="visually-hidden">Department</span>
              <select aria-label="Department" value={department ?? ''} onChange={(e) => setParam('department', e.target.value || null)}>
                <option value="">All departments</option>
                {data.departments.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
              </select>
            </label>
          )}
        </div>
      </div>

      {failed && <p className="muted" role="alert">The overview could not be loaded. Try again in a moment.</p>}
      {!data && !failed && <div className="stack"><Skeleton variant="tiles" rows={4} label="Loading overview" /><Skeleton rows={3} label="Loading what needs attention" /></div>}
      {data && <OverviewBody data={data} />}
    </>
  );
}

function OverviewBody({ data }: { data: Overview }) {
  const s = data.summary;
  const parts: React.ReactNode[] = [];
  if (s.overdue_acknowledgements) parts.push(<Link key="a" to="/admin/campaigns">{plural(s.overdue_acknowledgements, 'overdue acknowledgement')}</Link>);
  if (s.overdue_reviews) parts.push(<Link key="r" to="/library?review=overdue">{plural(s.overdue_reviews, 'document')} past review</Link>);
  if (s.pending_reviews) parts.push(<Link key="p" to="/library?state=in_review">{plural(s.pending_reviews, 'draft')} waiting for review</Link>);
  if (s.pending_announcements) parts.push(<Link key="n" to="/announcements">{plural(s.pending_announcements, 'announcement')} to approve</Link>);
  const ack = data.metrics.acknowledgements;
  const pub = data.metrics.published_versions;

  return (
    <>
      <p className="overview-summary">
        {parts.length === 0 ? 'Nothing is overdue. ' : parts.reduce<React.ReactNode[]>((acc, p, i) => (i === 0 ? [p] : [...acc, i === parts.length - 1 ? ' and ' : ', ', p]), [])}
        {parts.length > 0 && '.'}
      </p>

      <div className="overview-grid">
        <section aria-labelledby="attention-h" className="overview-attention">
          <h2 id="attention-h">Needs attention</h2>
          {data.attention.length === 0 ? (
            <div className="overview-empty"><Info aria-hidden="true" size="1rem" /><span>All clear. Overdue acknowledgements, documents past their review date, drafts waiting for review and admins without two-step verification will show up here.</span></div>
          ) : (
            <ol className="attention-list">
              {data.attention.map((a, i) => (
                <li key={`${a.kind}-${i}`} className={`attention-item sev-${a.severity}`}>
                  {a.severity === 'high' ? <CircleAlert aria-hidden="true" size="1rem" /> : a.severity === 'medium' ? <AlertTriangle aria-hidden="true" size="1rem" /> : <Info aria-hidden="true" size="1rem" />}
                  <Link to={a.link}>{a.title}</Link>
                  <ArrowUpRight aria-hidden="true" size="0.875rem" className="attention-go" />
                </li>
              ))}
            </ol>
          )}
        </section>

        <section aria-labelledby="metrics-h" className="overview-metrics">
          <h2 id="metrics-h">Activity</h2>
          <dl className="metric-rows">
            <div className="metric-row">
              <dt>Acknowledgements completed</dt>
              <dd><Link to="/admin/campaigns" aria-label="Acknowledgement completion: see campaigns"><strong>{ack.completion_rate === null ? '—' : `${Math.round(ack.completion_rate * 100)}%`}</strong></Link><span className="muted">{ack.assigned ? `${ack.completed} of ${ack.assigned} assigned` : 'none assigned in range'}</span></dd>
            </div>
            <div className="metric-row">
              <dt>Open acknowledgements</dt>
              <dd><Link to="/admin/campaigns?status=active" aria-label={`${ack.pending} open acknowledgements: see active campaigns`}><strong>{ack.pending}</strong></Link><span className="muted">across active campaigns</span></dd>
            </div>
            <div className="metric-row">
              <dt>Versions published</dt>
              <dd><Link to={`/admin/audit?action=version.published&from=${data.range.from.slice(0, 10)}`} aria-label={`${pub.total} versions published: see them in the audit log`}><strong>{pub.total}</strong></Link><Sparkline points={pub.series.map((p) => p.count)} label={`Versions published per ${pub.bucket}`} /></dd>
            </div>
            <div className="metric-row">
              <dt>Announcements published</dt>
              <dd><Link to="/admin/announcements?status=published" aria-label={`${data.metrics.announcements_published} announcements published: see them`}><strong>{data.metrics.announcements_published}</strong></Link></dd>
            </div>
            <div className="metric-row metric-row-guidance">
              <dt>Requests and SLAs</dt>
              <dd><span className="muted">No data yet. Request volume, overdue work and SLA risk appear once processes are running.</span></dd>
            </div>
          </dl>
          {data.departments.length > 0 && (
            <>
              <h2 className="overview-sub">By department</h2>
              <table className="table table-compact" aria-label="Departments">
                <thead><tr><th>Department</th><th className="num">Members</th><th className="num">Overdue acks</th></tr></thead>
                <tbody>{data.departments.map((d) => (
                  <tr key={d.id}><td><Link to={`/admin/overview?department=${d.id}`}>{d.name}</Link></td><td className="num">{d.members}</td><td className="num">{d.overdue_acknowledgements ? <Tag color="red">{d.overdue_acknowledgements}</Tag> : '0'}</td></tr>
                ))}</tbody>
              </table>
            </>
          )}
        </section>
      </div>

      <section aria-labelledby="security-h" className="overview-strip">
        <h2 id="security-h" className="visually-hidden">Security and storage</h2>
        <Link to="/admin/people?filter=no_mfa" className="strip-item"><span className="strip-value">{data.security.members_without_mfa}</span><span>without two-step verification</span></Link>
        <Link to="/admin/people" className="strip-item"><span className="strip-value">{data.security.failed_logins}</span><span>failed sign-ins</span></Link>
        <Link to="/admin/people?filter=privileged" className="strip-item"><span className="strip-value">{data.security.new_admins}</span><span>new admin grants</span></Link>
        <span className="strip-item"><span className="strip-value">{data.security.break_glass_sessions}</span><span>break-glass sessions</span></span>
        <span className="strip-item strip-guidance"><span>Storage and quota appear once files are enabled.</span></span>
      </section>
    </>
  );
}

/** A small inline trend line (data visualisation, not an icon). */
function Sparkline({ points, label }: { points: number[]; label: string }) {
  if (points.length < 2) return null;
  const w = 96, h = 24, max = Math.max(1, ...points);
  const d = points.map((p, i) => `${i === 0 ? 'M' : 'L'}${((i / (points.length - 1)) * w).toFixed(1)},${(h - 2 - (p / max) * (h - 4)).toFixed(1)}`).join(' ');
  return <svg className="sparkline" style={{ width: '6rem', height: '1.5rem' }} viewBox={`0 0 ${w} ${h}`} role="img" aria-label={`${label}: ${points.join(', ')}`}><path d={d} fill="none" strokeWidth={1.5} /></svg>;
}
