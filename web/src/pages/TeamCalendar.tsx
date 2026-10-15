import { ChevronLeft, ChevronRight } from 'lucide-react';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useResource } from '../lib/data';
import { Link } from '../lib/router';
import { useSession } from '../lib/session';
import { Avatar } from '../ui/Avatar';
import { Button } from '../ui/components/Button';
import { Select } from '../ui/components/Select';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { TimeOffTabs } from './areaTabs';
import './teamcalendar.css';

interface Person { id: string; name: string; title: string | null; department: string | null; location_id: string | null; me: boolean }
interface Absence { membership_id: string; start_date: string; start_half: string; end_date: string; end_half: string; state: 'pending' | 'approved'; label: string; colour: string; request_id: string | null; days: number | null }
interface Holiday { location_id: string; date: string; name: string; optional: boolean }
interface Cal { from: string; to: string; people: Person[]; absences: Absence[]; holidays: Holiday[]; work_weeks: [string, number[]][] }

const pad = (n: number) => String(n).padStart(2, '0');
const isoOf = (y: number, m: number, d: number) => `${y}-${pad(m + 1)}-${pad(d)}`;
const isoDay = (d: string) => { const [y, m, dd] = d.split('-').map(Number); return ((new Date(Date.UTC(y!, m! - 1, dd!)).getUTCDay() + 6) % 7) + 1; };
const todayIso = () => { const t = new Date(); return isoOf(t.getFullYear(), t.getMonth(), t.getDate()); };

/** What a cell shows: an absence (with which half), a holiday, a weekly day off. */
function cellOf(p: Person, d: string, abs: Absence[], hols: Map<string, Holiday>, weeks: Map<string, number[]>) {
  const a = abs.find((x) => x.membership_id === p.id && x.start_date <= d && x.end_date >= d);
  const h = p.location_id ? hols.get(`${p.location_id}:${d}`) : undefined;
  const week = (p.location_id && weeks.get(p.location_id)) || [1, 2, 3, 4, 5];
  const off = !week.includes(isoDay(d));
  let half: 'am' | 'pm' | null = null;
  if (a && a.start_date === d && a.start_half === 'second_half') half = 'pm';
  if (a && a.end_date === d && a.end_half === 'first_half') half = half === 'pm' ? null : 'am';
  return { a, h: h && !h.optional ? h : undefined, off, half };
}

/**
 * Team calendar (BPMS-225, docs/25 §6, §8): a month of who's out, by team, department or location,
 * with each person's holidays and days off. Colleagues' leave shows as "Out"; the person, their
 * managers and HR see the type and pending requests.
 */
export function TeamCalendar() {
  const { can } = useSession();
  const now = new Date();
  const [ym, setYm] = useState({ y: now.getFullYear(), m: now.getMonth() });
  const [scope, setScope] = useState('team');
  const last = new Date(ym.y, ym.m + 1, 0).getDate();
  const from = isoOf(ym.y, ym.m, 1);
  const to = isoOf(ym.y, ym.m, last);
  const [cal, reload, err] = useResource<Cal>(`/api/v1/team-calendar?from=${from}&to=${to}&scope=${scope}`);
  const today = todayIso();
  const days = useMemo(() => Array.from({ length: last }, (_, i) => isoOf(ym.y, ym.m, i + 1)), [ym, last]);
  const hols = useMemo(() => new Map((cal?.holidays ?? []).map((h) => [`${h.location_id}:${h.date}`, h])), [cal]);
  const weeks = useMemo(() => new Map(cal?.work_weeks ?? []), [cal]);
  const wrap = useRef<HTMLDivElement>(null);
  // Open the month at today (a few days before it in view), so the coming days are visible.
  useEffect(() => {
    const w = wrap.current;
    const th = w?.querySelector<HTMLElement>('.tc-day-h.is-today');
    const first = w?.querySelector<HTMLElement>('.tc-person-h');
    if (w && th && first) w.scrollLeft = Math.max(0, th.offsetLeft - first.offsetWidth - th.offsetWidth * 3);
  }, [cal]);
  const move = (n: number) => setYm(({ y, m }) => { const d = new Date(y, m + n, 1); return { y: d.getFullYear(), m: d.getMonth() }; });
  const title = new Date(ym.y, ym.m, 1).toLocaleDateString(undefined, { month: 'long', year: 'numeric' });
  const outToday = cal ? cal.people.filter((p) => cal.absences.some((a) => a.membership_id === p.id && a.state === 'approved' && a.start_date <= today && a.end_date >= today)) : [];
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Team calendar</h1>
          <p>Who’s out, holidays and days off. Colleagues see time off as “Out”; managers and HR see the type.</p>
        </div>
      </div>
      <TimeOffTabs current="calendar" />
      <div className="tc-bar">
        <Select label="Show" selectedKey={scope} onSelectionChange={(k) => setScope(String(k))} options={[
          { id: 'team', label: 'My team' }, { id: 'department', label: 'My department' }, { id: 'location', label: 'My location' },
          ...(can('time_off:view_all') ? [{ id: 'everyone', label: 'Everyone' }] : []),
        ]} />
        <div className="tc-month" role="group" aria-label="Month">
          <Button variant="ghost" aria-label="Previous month" onPress={() => move(-1)}><ChevronLeft aria-hidden="true" size="1rem" /></Button>
          <h2 className="tc-month-name" aria-live="polite">{title}</h2>
          <Button variant="ghost" aria-label="Next month" onPress={() => move(1)}><ChevronRight aria-hidden="true" size="1rem" /></Button>
          <Button variant="secondary" onPress={() => setYm({ y: now.getFullYear(), m: now.getMonth() })}>Today</Button>
        </div>
      </div>
      {err ? <LoadError error={err} onRetry={reload} what="the calendar" /> : !cal ? <Skeleton variant="table" label="Loading the calendar" /> : cal.people.length <= 1 && cal.absences.length === 0 && scope === 'team' ? (
        <Empty title="Your team is just you so far">Set managers in People to see your team here, or show your department or location.</Empty>
      ) : (
        <>
          {from <= today && today <= to && (
            <p className="tc-today"><strong>Out today:</strong> {outToday.length ? outToday.map((p) => p.me ? 'You' : p.name).join(', ') : 'nobody'}</p>
          )}
          <div className="tc-wrap" ref={wrap}>
            <table className="tc" aria-label={`Team calendar, ${title}`}>
              <thead>
                <tr>
                  <th scope="col" className="tc-person-h">Person</th>
                  {days.map((d) => {
                    const dt = new Date(`${d}T00:00:00Z`);
                    return (
                      <th key={d} scope="col" className={`tc-day-h${d === today ? ' is-today' : ''}${isoDay(d) >= 6 ? ' is-wkend' : ''}`} aria-label={dt.toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long', timeZone: 'UTC' })}>
                        <span className="tc-wd" aria-hidden="true">{dt.toLocaleDateString(undefined, { weekday: 'narrow', timeZone: 'UTC' })}</span>
                        <span className="tc-dn" aria-hidden="true">{dt.getUTCDate()}</span>
                      </th>
                    );
                  })}
                </tr>
              </thead>
              <tbody>
                {cal.people.map((p) => (
                  <tr key={p.id} className={p.me ? 'is-me' : undefined}>
                    <th scope="row" className="tc-person">
                      <Avatar name={p.name} size="1.5rem" />
                      <span className="tc-pname"><span>{p.me ? `${p.name} (you)` : p.name}</span>{p.title && <span className="tc-ptitle">{p.title}</span>}</span>
                    </th>
                    {days.map((d) => {
                      const c = cellOf(p, d, cal.absences, hols, weeks);
                      const what = c.a ? `${c.a.label}${c.a.state === 'pending' ? ' (pending)' : ''}${c.half ? (c.half === 'am' ? ', morning' : ', afternoon') : ''}` : c.h ? c.h.name : null;
                      const cls = ['tc-cell', c.off && 'is-off', c.h && 'is-hol', d === today && 'is-today'].filter(Boolean).join(' ');
                      return (
                        <td key={d} className={cls} title={what ?? undefined}>
                          {c.a && (
                            c.a.request_id
                              ? <Link to={`/requests/${c.a.request_id}`} className={`tc-ab ab-${c.a.colour}${c.a.state === 'pending' ? ' is-pending' : ''}${c.half ? ` half-${c.half}` : ''}`}><span className="visually-hidden">{`${p.name}, ${d}: ${what}`}</span></Link>
                              : <span role="img" className={`tc-ab ab-${c.a.colour}${c.half ? ` half-${c.half}` : ''}`} aria-label={`${p.name}, ${d}: ${what}`} />
                          )}
                          {!c.a && c.h && <span role="img" className="tc-hol" aria-label={`${d}: ${c.h.name}`} />}
                        </td>
                      );
                    })}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <ul className="tc-legend" aria-label="Legend">
            <li><span className="tc-ab ab-blue" aria-hidden="true" /> Time off</li>
            <li><span className="tc-ab ab-blue is-pending" aria-hidden="true" /> Waiting for approval</li>
            <li><span className="tc-ab ab-gray" aria-hidden="true" /> Out (colleague)</li>
            <li><span className="tc-hol tc-hol-key" aria-hidden="true" /> Holiday</li>
            <li><span className="tc-off-key" aria-hidden="true" /> Day off</li>
          </ul>
        </>
      )}
    </>
  );
}
