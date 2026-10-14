import { ChevronDown } from 'lucide-react';
import { useState } from 'react';
import './timeoff.css';

/** Time off as the API returns it (docs/08 "Time off", docs/25 §3). */
export interface LeaveType {
  id: string; key: string; name: string; colour: string; paid: boolean; limited: boolean; sensitive: boolean; half_days: boolean;
  needs_attachment_after_days: number | null; min_notice_days: number; max_consecutive_days: number | null; negative_limit_days: number;
  counts_non_working_days: boolean; optional_holidays_only: boolean; archived: boolean; revision: number;
}
export interface Rule { leave_type_id: string; yearly_days: number; accrual: 'upfront' | 'monthly'; prorate: boolean; probation_waits: boolean; carry_over_max_days: number; carry_over_expiry_months: number | null }
export interface Policy { id: string; name: string; audience: unknown; priority: number; year_start_month: number; rules: Rule[]; members: number; archived: boolean; revision: number }
export interface PolicyOf { id: string; name: string; via: 'assigned' | 'audience'; year_start_month: number }
export interface Entry { id: string; kind: string; days: number; reason: string | null; by: string | null; request_id: string | null; at: string }
export interface Balance {
  leave_type: { id: string; key: string; name: string; colour: string; limited: boolean; sensitive: boolean; paid: boolean };
  year_start: string; year_end: string; yearly_days: number | null; accrual: string | null;
  granted: number; carried: number; taken: number; pending: number; adjusted: number; expired: number; available: number; entries: Entry[];
}

export const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

/** "1 day", "2.5 days", "½ day". */
export function daysText(n: number) {
  const a = Math.abs(n);
  const num = a === 0.5 ? '½' : Number.isInteger(a) ? String(a) : a.toFixed(1);
  return `${n < 0 ? '−' : ''}${num} ${a === 1 || a === 0.5 ? 'day' : 'days'}`;
}
const signed = (n: number) => (n > 0 ? `+${daysText(n)}` : daysText(n));
const shortDay = (iso: string) => new Date(iso).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
const monthDay = (d: string) => new Date(`${d}T00:00:00Z`).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' });

const KIND: Record<string, string> = {
  grant: 'Yearly allowance', accrual: 'Monthly accrual', hold: 'Requested (pending)', release: 'Request closed', taken: 'Time off taken',
  cancelled: 'Time off cancelled', adjustment: 'Adjusted by HR', carry_over: 'Carried over', expiry: 'Expired',
};
function entryLabel(e: Entry) {
  if (e.kind === 'carry_over') return e.days > 0 ? 'Carried over from last year' : 'Carried over to next year';
  if (e.kind === 'expiry') return 'Expired (not used in time)';
  return KIND[e.kind] ?? e.kind;
}

/** A balance with its "how it's calculated" ledger. */
export function BalanceCard({ b }: { b: Balance }) {
  const [open, setOpen] = useState(false);
  const t = b.leave_type;
  const id = `bal-${t.id}`;
  const parts: string[] = [];
  if (b.yearly_days != null && t.limited) parts.push(`${daysText(b.yearly_days)} a year${b.accrual === 'monthly' ? ', given monthly' : ''}`);
  if (b.pending) parts.push(`${daysText(b.pending)} pending`);
  if (b.taken) parts.push(`${daysText(b.taken)} taken`);
  return (
    <section className={`bal bal-${t.colour}`} aria-labelledby={`${id}-h`}>
      <div className="bal-top">
        <h3 id={`${id}-h`} className="bal-name"><span className="bal-dot" aria-hidden="true" />{t.name}</h3>
        {!t.paid && <span className="tag tag-gray">Unpaid</span>}
      </div>
      {t.limited ? (
        <p className="bal-num" aria-label={`${daysText(b.available)} available`}>
          <span className="bal-n">{b.available === 0.5 ? '½' : b.available}</span>
          <span className="bal-unit">{Math.abs(b.available) === 1 ? 'day' : 'days'} available</span>
        </p>
      ) : (
        <p className="bal-num"><span className="bal-n">{b.taken}</span><span className="bal-unit">{b.taken === 1 ? 'day' : 'days'} taken · no limit</span></p>
      )}
      <p className="bal-sub">{parts.join(' · ') || `Leave year ${monthDay(b.year_start)} – ${monthDay(b.year_end)}`}</p>
      {b.entries.length > 0 && (
        <>
          <button type="button" className="bal-why" aria-expanded={open} aria-controls={`${id}-l`} onClick={() => setOpen((o) => !o)}>
            How it’s calculated <ChevronDown aria-hidden="true" size="0.875rem" className={open ? 'is-open' : ''} />
          </button>
          {open && (
            <ol id={`${id}-l`} className="bal-ledger" aria-label={`${t.name} entries`}>
              {b.entries.map((e) => (
                <li key={e.id}>
                  <span className="bal-l-what">{entryLabel(e)}{e.reason && <span className="bal-l-reason">“{e.reason}”{e.by ? ` — ${e.by}` : ''}</span>}</span>
                  <span className="bal-l-when">{shortDay(e.at)}</span>
                  <span className={`bal-l-days ${e.days < 0 ? 'neg' : 'pos'}`}>{signed(e.days)}</span>
                </li>
              ))}
              <li className="bal-l-total"><span className="bal-l-what">Available</span><span /><span className="bal-l-days">{daysText(b.available)}</span></li>
            </ol>
          )}
        </>
      )}
    </section>
  );
}
