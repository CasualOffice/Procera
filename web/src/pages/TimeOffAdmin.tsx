import { Pencil, Plus, Scale, Sparkles, Tags, Users } from 'lucide-react';
import { useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { useResource } from '../lib/data';
import { useSession } from '../lib/session';
import { type Audience, AudiencePicker, EVERYONE, audienceOf, audienceSummary, isEmptyAudience, useDirectory } from '../ui/Audience';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { Tabs } from '../ui/components/Tabs';
import { TextField } from '../ui/components/TextField';
import { Drawer } from '../ui/Drawer';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { useStepUp } from '../ui/StepUp';
import { type Balance, BalanceCard, type LeaveType, MONTHS, type Policy, type PolicyOf, type Rule, daysText } from './timeOffShared';

const errText = (e: unknown, fallback: string) => (e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : fallback);
const COLOURS = ['blue', 'green', 'pink', 'purple', 'orange', 'yellow', 'brown', 'red', 'gray'];
const cap = (s: string) => s.charAt(0).toUpperCase() + s.slice(1);

/**
 * Admin › Time off (BPMS-223, docs/25 §8): everyone's balances (adjust with a reason, see the
 * ledger), leave policies (who they apply to, allowance, accrual, carry-over) and leave types.
 */
export function TimeOffAdmin() {
  const { can } = useSession();
  const manage = can('time_off:manage_policies');
  const [types, reloadTypes, err] = useResource<{ items: LeaveType[] }>('/api/v1/leave-types?archived=true');
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(null);
  const [tick, setTick] = useState(0);
  const reload = () => { reloadTypes(); setTick((t) => t + 1); };
  if (err) return <LoadError error={err} onRetry={reloadTypes} what="leave types" />;
  if (!types) return <Skeleton variant="list" label="Loading time off" />;
  const head = (
    <div className="page-head">
      <div>
        <h1>Time off</h1>
        <p>Leave types, the policies that give people their allowance, and everyone’s balances. Every balance is a list of entries you can explain.</p>
      </div>
    </div>
  );
  if (types.items.length === 0) {
    const defaults = async () => {
      try { await api('/api/v1/leave-types/defaults', { method: 'POST' }); setNotice({ tone: 'success', title: 'Leave types and a Standard policy are ready. Adjust them to match your handbook.' }); reload(); }
      catch (e) { setNotice({ tone: 'danger', title: errText(e, 'Couldn’t set them up.') }); }
    };
    return (
      <>
        {head}
        {notice && <Notice tone={notice.tone} title={notice.title} />}
        <Empty icon={Sparkles} title="Set up time off" action={manage ? <Button onPress={() => void defaults()}><Sparkles aria-hidden="true" size="1rem" /> Start with common leave types</Button> : undefined}>
          Annual (18 days, given monthly, 5 carry over for 3 months), sick (12, private), casual (6), two optional holidays and unpaid leave, in one policy for everyone. You can change all of it.
        </Empty>
      </>
    );
  }
  return (
    <>
      {head}
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      <Tabs label="Time off" tabs={[
        { id: 'balances', label: 'Balances', content: <BalancesTab key={tick} types={types.items} onNotice={setNotice} /> },
        { id: 'policies', label: 'Policies', content: <PoliciesTab key={tick} types={types.items} manage={manage} onNotice={setNotice} /> },
        { id: 'types', label: 'Leave types', content: <TypesTab types={types.items} manage={manage} onChanged={reload} onNotice={setNotice} /> },
      ]} />
    </>
  );
}

type NoticeFn = (n: { tone: 'success' | 'danger'; title: string } | null) => void;

// ------------------------------------------------------------------ balances

interface Row { id: string; name: string; email: string; policy: PolicyOf | null; balances: Record<string, { available: number; taken: number; pending: number }> }

function BalancesTab({ types, onNotice }: { types: LeaveType[]; onNotice: NoticeFn }) {
  const { can } = useSession();
  const [data, reload, err] = useResource<{ types: LeaveType[]; items: Row[] }>('/api/v1/time-off/balances');
  const [q, setQ] = useState('');
  const [person, setPerson] = useState<Row | null>(null);
  const [adjusting, setAdjusting] = useState<Row | null>(null);
  const active = types.filter((t) => !t.archived);
  if (err) return <LoadError error={err} onRetry={reload} what="balances" />;
  if (!data) return <Skeleton variant="table" label="Loading balances" />;
  const rows = data.items.filter((r) => !q || `${r.name} ${r.email}`.toLowerCase().includes(q.toLowerCase()));
  return (
    <div className="to-tab">
      <div className="to-bar">
        <TextField label="Find a person" value={q} onChange={setQ} placeholder="Name or email" />
        <span className="muted to-count">{data.items.length} {data.items.length === 1 ? 'person' : 'people'} · current leave year</span>
      </div>
      <div className="table-wrap to-table">
        <table className="table" aria-label="Balances">
          <thead>
            <tr>
              <th scope="col">Person</th>
              <th scope="col">Policy</th>
              {active.map((t) => <th key={t.id} scope="col" className="num"><span className={`to-th-dot dot-${t.colour}`} aria-hidden="true" />{t.name}</th>)}
              <th scope="col"><span className="visually-hidden">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.id}>
                <td><button type="button" className="link-button to-person" onClick={() => setPerson(r)}>{r.name}</button><span className="to-email">{r.email}</span></td>
                <td>{r.policy ? <>{r.policy.name}{r.policy.via === 'assigned' && <span className="muted"> · assigned</span>}</> : <span className="muted">None</span>}</td>
                {active.map((t) => {
                  const b = r.balances[t.id];
                  return <td key={t.id} className="num">{b ? (t.limited ? b.available : <span className="muted">{b.taken} taken</span>) : <span className="muted">—</span>}{b?.pending ? <span className="to-pending" title="Pending"> ({b.pending} pending)</span> : null}</td>;
                })}
                <td className="to-actions">{can('time_off:adjust_balances') && <Button variant="ghost" onPress={() => setAdjusting(r)} aria-label={`Adjust ${r.name}`}><Scale aria-hidden="true" size="0.9375rem" /> Adjust</Button>}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {person && <PersonDrawer row={person} onClose={() => setPerson(null)} />}
      <AdjustDialog row={adjusting} types={active.filter((t) => t.limited)} onClose={() => setAdjusting(null)} onDone={(t) => { onNotice({ tone: 'success', title: t }); reload(); }} />
    </div>
  );
}

function PersonDrawer({ row, onClose }: { row: Row; onClose: () => void }) {
  const [data, , err] = useResource<{ policy: PolicyOf | null; items: Balance[] }>(`/api/v1/members/${row.id}/time-off`);
  return (
    <Drawer title={row.name} onClose={onClose}>
      {err ? <LoadError error={err} what="their balances" /> : !data ? <Skeleton variant="list" label="Loading" /> : (
        <div className="stack">
          <p className="muted to-drawer-meta">{data.policy ? `${data.policy.name} policy (${data.policy.via === 'assigned' ? 'assigned to them' : 'from its audience'})` : 'No policy applies.'}</p>
          {data.items.map((b) => <BalanceCard key={b.leave_type.id} b={b} />)}
        </div>
      )}
    </Drawer>
  );
}

function AdjustDialog({ row, types, onClose, onDone }: { row: Row | null; types: LeaveType[]; onClose: () => void; onDone: (title: string) => void }) {
  const [type, setType] = useState(types[0]?.id ?? '');
  const [dir, setDir] = useState<'add' | 'remove'>('add');
  const [days, setDays] = useState('1');
  const [reason, setReason] = useState('');
  const [err, setErr] = useState<string | null>(null);
  const { guard, dialog } = useStepUp((e) => setErr(errText(e, 'Couldn’t adjust.')));
  const n = Number(days);
  const valid = n > 0 && Number.isInteger(n * 2) && reason.trim().length > 0 && type;
  const go = () => guard(async () => {
    if (!row) return;
    const d = dir === 'add' ? n : -n;
    await api(`/api/v1/members/${row.id}/time-off/adjustments`, { method: 'POST', body: { leave_type_id: type, days: d, reason } });
    const t = types.find((x) => x.id === type)?.name ?? 'Leave';
    onDone(`${t}: ${d > 0 ? '+' : ''}${daysText(d)} for ${row.name}.`);
    setReason(''); setDays('1'); onClose();
  });
  return (
    <>
      <FormDialog isOpen={row !== null} onOpenChange={(o) => { if (!o) onClose(); }} title={`Adjust ${row?.name ?? ''}’s balance`} description="Adds an entry to their balance with your reason. Nothing is ever overwritten."
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void go()} isDisabled={!valid}>Adjust balance</Button></>}>
        <Form className="stack" onSubmit={(e) => { e.preventDefault(); void go(); }}>
          {err && <Notice tone="danger" title={err} />}
          <Select label="Leave type" selectedKey={type} onSelectionChange={(k) => setType(String(k))} options={types.map((t) => ({ id: t.id, label: t.name }))} />
          <div className="to-adjust-row">
            <Select label="Change" selectedKey={dir} onSelectionChange={(k) => setDir(k as 'add' | 'remove')} options={[{ id: 'add', label: 'Add days' }, { id: 'remove', label: 'Remove days' }]} />
            <TextField label="Days" type="number" value={days} onChange={setDays} inputMode="decimal" description="Whole or half days" />
          </div>
          <TextField label="Reason" value={reason} onChange={setReason} isRequired placeholder="Worked on the 15 August holiday" description="They see this next to the entry." />
        </Form>
      </FormDialog>
      {dialog}
    </>
  );
}

// ------------------------------------------------------------------ policies

function PoliciesTab({ types, manage, onNotice }: { types: LeaveType[]; manage: boolean; onNotice: NoticeFn }) {
  const [data, reload, err] = useResource<{ items: Policy[] }>('/api/v1/leave-policies?archived=true');
  const dir = useDirectory();
  const [editing, setEditing] = useState<Policy | 'new' | null>(null);
  const byId = useMemo(() => new Map(types.map((t) => [t.id, t])), [types]);
  if (err) return <LoadError error={err} onRetry={reload} what="policies" />;
  if (!data) return <Skeleton variant="list" label="Loading policies" />;
  return (
    <div className="to-tab">
      <div className="to-bar">
        <p className="muted to-hint">When several policies include someone, the first one in this list applies to them.</p>
        {manage && <Button onPress={() => setEditing('new')}><Plus aria-hidden="true" size="1rem" /> New policy</Button>}
      </div>
      {data.items.length === 0 ? <Empty compact icon={Users} title="No policies yet">A policy gives a group of people their yearly allowance for each leave type.</Empty> : (
        <ol className="pol-list" aria-label="Leave policies">
          {data.items.map((p, i) => (
            <li key={p.id} className={`card pol${p.archived ? ' is-archived' : ''}`}>
              <div className="pol-head">
                <span className="pol-rank" aria-label={`Order ${i + 1}`}>{i + 1}</span>
                <div className="pol-title">
                  <h3>{p.name}{p.archived && <span className="tag tag-gray">Archived</span>}</h3>
                  <p className="muted">{audienceSummary(audienceOf(p.audience), dir)} · {p.members} {p.members === 1 ? 'person' : 'people'} · leave year starts in {MONTHS[p.year_start_month - 1]}</p>
                </div>
                {manage && <Button variant="ghost" aria-label={`Edit ${p.name}`} onPress={() => setEditing(p)}><Pencil aria-hidden="true" size="0.9375rem" /> Edit</Button>}
              </div>
              <table className="table pol-rules" aria-label={`${p.name} allowances`}>
                <thead><tr><th scope="col">Leave type</th><th scope="col" className="num">Per year</th><th scope="col">Given</th><th scope="col">Carry over</th></tr></thead>
                <tbody>
                  {p.rules.map((r) => {
                    const t = byId.get(r.leave_type_id);
                    return (
                      <tr key={r.leave_type_id}>
                        <td><span className={`to-th-dot dot-${t?.colour ?? 'gray'}`} aria-hidden="true" />{t?.name ?? 'Unknown'}</td>
                        <td className="num">{t && !t.limited ? 'No limit' : daysText(r.yearly_days)}</td>
                        <td>{t && !t.limited ? '—' : r.accrual === 'monthly' ? 'Monthly' : 'At the start of the year'}{r.prorate && t?.limited ? <span className="muted"> · pro-rated</span> : null}{r.probation_waits ? <span className="muted"> · after probation</span> : null}</td>
                        <td>{r.carry_over_max_days ? <>Up to {daysText(r.carry_over_max_days)}{r.carry_over_expiry_months ? <span className="muted">, used within {r.carry_over_expiry_months} {r.carry_over_expiry_months === 1 ? 'month' : 'months'}</span> : null}</> : <span className="muted">None</span>}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </li>
          ))}
        </ol>
      )}
      <PolicyDialog key={editing === 'new' ? 'new' : editing?.id ?? 'none'} policy={editing} types={types.filter((t) => !t.archived)} onClose={() => setEditing(null)} onDone={(t) => { onNotice({ tone: 'success', title: t }); reload(); }} />
    </div>
  );
}

interface Draft { on: boolean; yearly: string; accrual: 'upfront' | 'monthly'; carry: string; expiry: string; prorate: boolean; probation: boolean }

function PolicyDialog({ policy, types, onClose, onDone }: { policy: Policy | 'new' | null; types: LeaveType[]; onClose: () => void; onDone: (t: string) => void }) {
  const edit = policy && policy !== 'new' ? policy : null;
  const [name, setName] = useState(edit?.name ?? '');
  const [aud, setAud] = useState<Audience>(edit ? audienceOf(edit.audience) : EVERYONE);
  const [ysm, setYsm] = useState(String(edit?.year_start_month ?? 1));
  const [rules, setRules] = useState<Record<string, Draft>>(() => Object.fromEntries(types.map((t) => {
    const r = edit?.rules.find((x) => x.leave_type_id === t.id);
    return [t.id, { on: !!r || !edit, yearly: String(r?.yearly_days ?? (t.limited ? 12 : 0)), accrual: r?.accrual ?? 'upfront', carry: String(r?.carry_over_max_days ?? 0), expiry: r?.carry_over_expiry_months ? String(r.carry_over_expiry_months) : '', prorate: r?.prorate ?? true, probation: r?.probation_waits ?? false }];
  })));
  const [err, setErr] = useState<string | null>(null);
  const set = (id: string, patch: Partial<Draft>) => setRules((rs) => ({ ...rs, [id]: { ...rs[id]!, ...patch } }));
  const save = async (archived?: boolean) => {
    setErr(null);
    const body = {
      name, audience: aud, year_start_month: Number(ysm),
      rules: types.filter((t) => rules[t.id]?.on).map((t): Rule => {
        const d = rules[t.id]!;
        return { leave_type_id: t.id, yearly_days: t.limited ? Number(d.yearly) : 0, accrual: d.accrual, prorate: d.prorate, probation_waits: d.probation, carry_over_max_days: t.limited ? Number(d.carry) : 0, carry_over_expiry_months: d.expiry ? Number(d.expiry) : null };
      }),
      ...(archived !== undefined ? { archived } : {}),
    };
    try {
      if (edit) await api(`/api/v1/leave-policies/${edit.id}`, { method: 'PATCH', headers: { 'If-Match': `"${edit.revision}"` }, body });
      else await api('/api/v1/leave-policies', { method: 'POST', body });
      onDone(archived ? `${name.trim()} archived.` : edit ? `${name.trim()} saved. New allowances apply from the next grant.` : `${name.trim()} created.`);
      onClose();
    } catch (e) { setErr(errText(e, 'Couldn’t save.')); }
  };
  return (
    <FormDialog isOpen={policy !== null} onOpenChange={(o) => { if (!o) onClose(); }} title={edit ? `Edit ${edit.name}` : 'New leave policy'} width={760}
      footer={<>
        {edit && !edit.archived && <Button variant="ghost" onPress={() => void save(true)}>Archive</Button>}
        <span className="sheet-foot-gap" />
        <Button variant="secondary" onPress={onClose}>Cancel</Button>
        <Button onPress={() => void save()} isDisabled={!name.trim() || isEmptyAudience(aud)}>{edit ? 'Save policy' : 'Create policy'}</Button>
      </>}>
      <Form className="stack" onSubmit={(e) => { e.preventDefault(); void save(); }}>
        {err && <Notice tone="danger" title={err} />}
        <div className="to-adjust-row">
          <TextField label="Name" value={name} onChange={setName} isRequired placeholder="India full-time" />
          <Select label="Leave year starts in" selectedKey={ysm} onSelectionChange={(k) => setYsm(String(k))} options={MONTHS.map((m, i) => ({ id: String(i + 1), label: m }))} />
        </div>
        <AudiencePicker value={aud} onChange={setAud} legend="Applies to" summary="Applies to" />
        <fieldset className="pol-edit">
          <legend className="field-label">Allowances</legend>
          {types.map((t) => {
            const d = rules[t.id]!;
            return (
              <div key={t.id} className={`pol-edit-row${d.on ? '' : ' is-off'}`}>
                <Checkbox isSelected={d.on} onChange={(on) => set(t.id, { on })}><span className={`to-th-dot dot-${t.colour}`} aria-hidden="true" />{t.name}</Checkbox>
                {d.on && t.limited && (
                  <div className="pol-edit-fields">
                    <TextField label="Days a year" type="number" value={d.yearly} onChange={(v) => set(t.id, { yearly: v })} inputMode="decimal" />
                    <Select label="Given" selectedKey={d.accrual} onSelectionChange={(k) => set(t.id, { accrual: k as Draft['accrual'] })} options={[{ id: 'upfront', label: 'At the start of the year' }, { id: 'monthly', label: 'Monthly' }]} />
                    <TextField label="Carry over up to" type="number" value={d.carry} onChange={(v) => set(t.id, { carry: v })} inputMode="decimal" />
                    <TextField label="Use carried days within (months)" type="number" value={d.expiry} onChange={(v) => set(t.id, { expiry: v })} inputMode="numeric" placeholder="No limit" />
                    <Checkbox isSelected={d.prorate} onChange={(v) => set(t.id, { prorate: v })}>Pro-rate for joiners</Checkbox>
                    <Checkbox isSelected={d.probation} onChange={(v) => set(t.id, { probation: v })}>Starts after probation</Checkbox>
                  </div>
                )}
                {d.on && !t.limited && <p className="muted pol-edit-note">No limit: only what’s taken is counted.</p>}
              </div>
            );
          })}
        </fieldset>
      </Form>
    </FormDialog>
  );
}

// ------------------------------------------------------------------ types

function TypesTab({ types, manage, onChanged, onNotice }: { types: LeaveType[]; manage: boolean; onChanged: () => void; onNotice: NoticeFn }) {
  const [editing, setEditing] = useState<LeaveType | 'new' | null>(null);
  return (
    <div className="to-tab">
      <div className="to-bar">
        <p className="muted to-hint">What people can take time off for. Private types show as “Out” to colleagues.</p>
        {manage && <Button onPress={() => setEditing('new')}><Plus aria-hidden="true" size="1rem" /> New leave type</Button>}
      </div>
      {types.length === 0 ? <Empty compact icon={Tags} title="No leave types" /> : (
        <ul className="lt-list" aria-label="Leave types">
          {types.map((t) => (
            <li key={t.id} className={`lt-row${t.archived ? ' is-archived' : ''}`}>
              <span className={`lt-swatch dot-${t.colour}`} aria-hidden="true" />
              <span className="lt-main">
                <span className="lt-name">{t.name}</span>
                <span className="lt-tags">
                  {!t.paid && <span className="tag tag-gray">Unpaid</span>}
                  {!t.limited && <span className="tag tag-gray">No limit</span>}
                  {t.sensitive && <span className="tag tag-pink">Private</span>}
                  {t.optional_holidays_only && <span className="tag tag-purple">Optional holidays</span>}
                  {t.counts_non_working_days && <span className="tag tag-orange">Counts weekends</span>}
                  {t.archived && <span className="tag tag-gray">Archived</span>}
                </span>
              </span>
              <span className="lt-rules muted">
                {[t.half_days ? 'Half days' : 'Whole days', t.min_notice_days ? `${t.min_notice_days} days’ notice` : null, t.max_consecutive_days ? `Up to ${t.max_consecutive_days} in a row` : null, t.needs_attachment_after_days ? `Note after ${t.needs_attachment_after_days} days` : null, t.negative_limit_days ? `Can go ${daysText(t.negative_limit_days)} below zero` : null].filter(Boolean).join(' · ')}
              </span>
              {manage && <Button variant="ghost" aria-label={`Edit ${t.name}`} onPress={() => setEditing(t)}><Pencil aria-hidden="true" size="0.9375rem" /></Button>}
            </li>
          ))}
        </ul>
      )}
      <TypeDialog key={editing === 'new' ? 'new' : editing?.id ?? 'none'} type={editing} onClose={() => setEditing(null)} onDone={(m) => { onNotice({ tone: 'success', title: m }); onChanged(); }} />
    </div>
  );
}

function TypeDialog({ type, onClose, onDone }: { type: LeaveType | 'new' | null; onClose: () => void; onDone: (t: string) => void }) {
  const e0 = type && type !== 'new' ? type : null;
  const [f, setF] = useState({
    name: e0?.name ?? '', colour: e0?.colour ?? 'blue', paid: e0?.paid ?? true, limited: e0?.limited ?? true, sensitive: e0?.sensitive ?? false,
    half_days: e0?.half_days ?? true, counts_non_working_days: e0?.counts_non_working_days ?? false, optional_holidays_only: e0?.optional_holidays_only ?? false,
    min_notice_days: String(e0?.min_notice_days ?? 0), max_consecutive_days: e0?.max_consecutive_days ? String(e0.max_consecutive_days) : '',
    needs_attachment_after_days: e0?.needs_attachment_after_days ? String(e0.needs_attachment_after_days) : '', negative_limit_days: String(e0?.negative_limit_days ?? 0),
  });
  const [err, setErr] = useState<string | null>(null);
  const set = (p: Partial<typeof f>) => setF((x) => ({ ...x, ...p }));
  const save = async (archived?: boolean) => {
    setErr(null);
    const body = {
      name: f.name, colour: f.colour, paid: f.paid, limited: f.limited, sensitive: f.sensitive, half_days: f.half_days,
      counts_non_working_days: f.counts_non_working_days, optional_holidays_only: f.optional_holidays_only,
      min_notice_days: Number(f.min_notice_days) || 0, max_consecutive_days: f.max_consecutive_days ? Number(f.max_consecutive_days) : null,
      needs_attachment_after_days: f.needs_attachment_after_days ? Number(f.needs_attachment_after_days) : null, negative_limit_days: Number(f.negative_limit_days) || 0,
      ...(archived !== undefined ? { archived } : {}),
    };
    try {
      if (e0) await api(`/api/v1/leave-types/${e0.id}`, { method: 'PATCH', headers: { 'If-Match': `"${e0.revision}"` }, body });
      else await api('/api/v1/leave-types', { method: 'POST', body });
      onDone(archived ? `${f.name.trim()} archived.` : archived === false ? `${f.name.trim()} restored.` : e0 ? `${f.name.trim()} saved.` : `${f.name.trim()} added.`);
      onClose();
    } catch (e) { setErr(errText(e, 'Couldn’t save.')); }
  };
  return (
    <FormDialog isOpen={type !== null} onOpenChange={(o) => { if (!o) onClose(); }} title={e0 ? `Edit ${e0.name}` : 'New leave type'} width={620}
      footer={<>
        {e0 && <Button variant="ghost" onPress={() => void save(!e0.archived)}>{e0.archived ? 'Restore' : 'Archive'}</Button>}
        <span className="sheet-foot-gap" />
        <Button variant="secondary" onPress={onClose}>Cancel</Button>
        <Button onPress={() => void save()} isDisabled={!f.name.trim()}>{e0 ? 'Save' : 'Add leave type'}</Button>
      </>}>
      <Form className="stack" onSubmit={(e) => { e.preventDefault(); void save(); }}>
        {err && <Notice tone="danger" title={err} />}
        <div className="to-adjust-row">
          <TextField label="Name" value={f.name} onChange={(v) => set({ name: v })} isRequired placeholder="Parental leave" />
          <Select label="Colour" selectedKey={f.colour} onSelectionChange={(k) => set({ colour: String(k) })} options={COLOURS.map((c) => ({ id: c, label: cap(c) }))} />
        </div>
        <div className="lt-checks">
          <Checkbox isSelected={f.paid} onChange={(v) => set({ paid: v })}>Paid</Checkbox>
          <Checkbox isSelected={f.limited} onChange={(v) => set({ limited: v })}>Has a yearly allowance</Checkbox>
          <Checkbox isSelected={f.sensitive} onChange={(v) => set({ sensitive: v })}>Private (colleagues see only “Out”)</Checkbox>
          <Checkbox isSelected={f.half_days} onChange={(v) => set({ half_days: v })}>Half days allowed</Checkbox>
          <Checkbox isSelected={f.counts_non_working_days} onChange={(v) => set({ counts_non_working_days: v })}>Count weekends and holidays inside a request</Checkbox>
          <Checkbox isSelected={f.optional_holidays_only} onChange={(v) => set({ optional_holidays_only: v })}>Only on optional holidays</Checkbox>
        </div>
        <div className="lt-nums">
          <TextField label="Notice (days)" type="number" value={f.min_notice_days} onChange={(v) => set({ min_notice_days: v })} inputMode="numeric" />
          <TextField label="Most days in a row" type="number" value={f.max_consecutive_days} onChange={(v) => set({ max_consecutive_days: v })} inputMode="numeric" placeholder="No limit" />
          <TextField label="Note needed after (days)" type="number" value={f.needs_attachment_after_days} onChange={(v) => set({ needs_attachment_after_days: v })} inputMode="numeric" placeholder="Never" />
          <TextField label="Can go below zero by (days)" type="number" value={f.negative_limit_days} onChange={(v) => set({ negative_limit_days: v })} inputMode="decimal" />
        </div>
      </Form>
    </FormDialog>
  );
}
