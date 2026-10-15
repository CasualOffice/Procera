import { CalendarDays, Palmtree, Plus } from 'lucide-react';
import { useState } from 'react';
import { useResource } from '../lib/data';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { TimeOffTabs } from './areaTabs';
import { type LeaveRequest, MyRequests, RequestTimeOff } from './timeOffRequest';
import { type Balance, BalanceCard, MONTHS, type PolicyOf } from './timeOffShared';

interface Holiday { date: string; name: string; optional: boolean }

const dayOf = (d: string) => new Date(`${d}T00:00:00Z`);
const fmt = (d: string, o: Intl.DateTimeFormatOptions) => dayOf(d).toLocaleDateString(undefined, { ...o, timeZone: 'UTC' });

/**
 * Time off (BPMS-223, docs/25 §8): my balances by type, each explained by its ledger, and my
 * location's upcoming holidays. Requesting time off arrives with BPMS-224.
 */
export function TimeOff() {
  const [data, reload, err] = useResource<{ policy: PolicyOf | null; items: Balance[]; requests: LeaveRequest[] }>('/api/v1/me/time-off');
  const [asking, setAsking] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const [hols] = useResource<{ location: { name: string } | null; items: Holiday[] }>('/api/v1/me/holidays');
  if (err) return <LoadError error={err} onRetry={reload} what="your time off" />;
  if (!data) return <Skeleton variant="tiles" label="Loading your time off" />;
  const upcoming = (hols?.items ?? []).slice(0, 6);
  const first = data.items[0];
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Time off</h1>
          <p>
            {data.policy
              ? <>{data.policy.name} policy · leave year {first ? `${fmt(first.year_start, { month: 'short', year: 'numeric' })} – ${fmt(first.year_end, { month: 'short', year: 'numeric' })}` : `from ${MONTHS[data.policy.year_start_month - 1]}`}</>
              : 'Your balances appear here once HR sets up a leave policy that includes you.'}
          </p>
        </div>
        {data.items.length > 0 && <Button onPress={() => setAsking(true)}><Plus aria-hidden="true" size="1rem" /> Request time off</Button>}
      </div>
      <TimeOffTabs current="mine" />
      {done && <Notice tone="success" title={done} />}
      <div className="to-layout">
        <div>
          {data.items.length === 0 ? (
            <Empty icon={Palmtree} title="No time off set up yet">
              When HR adds leave types and a policy, you’ll see how many days you have and how each number is worked out.
            </Empty>
          ) : (
            <div className="bal-grid" role="list" aria-label="Balances">
              {data.items.map((b) => <div role="listitem" key={b.leave_type.id}><BalanceCard b={b} /></div>)}
            </div>
          )}
          <MyRequests items={data.requests ?? []} />
        </div>
        <aside className="card to-hols" aria-labelledby="to-hols-h">
          <div className="card-head"><h2 id="to-hols-h">Upcoming holidays</h2>{hols?.location && <span className="muted to-loc">{hols.location.name}</span>}</div>
          {upcoming.length === 0 ? (
            <p className="card-empty">{hols?.location ? 'No holidays in the next year.' : 'Your location isn’t set yet, so no holidays show.'}</p>
          ) : (
            <ul className="to-hol-list">
              {upcoming.map((h) => (
                <li key={h.date}>
                  <span className="to-hol-date" aria-hidden="true"><CalendarDays size="0.875rem" /></span>
                  <span className="to-hol-main"><span className="to-hol-name">{h.name}</span><span className="to-hol-when">{fmt(h.date, { weekday: 'long', day: 'numeric', month: 'long' })}</span></span>
                  {h.optional && <span className="tag tag-purple">Optional</span>}
                </li>
              ))}
            </ul>
          )}
        </aside>
      </div>
      <RequestTimeOff isOpen={asking} onClose={() => setAsking(false)} balances={data.items} onDone={(m) => { setDone(m); reload(); }} />
    </>
  );
}
