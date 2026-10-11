import { Check } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { useItems, useResource } from '../lib/data';
import { useSession } from '../lib/session';
import { Tag } from '../ui/Tag';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { StepUpDialog } from '../ui/StepUp';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Control, type Kind, type Setting, show } from './Settings';
import { AREAS, LABELS, areaOf, humanise } from './settingsMeta';

interface Department { id: string; name: string; parent_id: string | null; path: string[] }
interface DeptSetting extends Setting { department_id: string; override_value: unknown | null; override_revision: number; inherited_from: string | null }

const errorText = (e: unknown) => {
  if (e instanceof ApiError && e.code === 'precondition_failed') return 'Someone else changed this. Reload the page to see the new value.';
  return e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not save. Try again.';
};

/** Department overrides (docs/20 §1 layer 4): pick a department, see what applies there, override or clear. */
export function DepartmentOverrides() {
  const [departments, reloadDepts, deptError] = useItems<Department>('/api/v1/departments');
  const [chosen, setChosen] = useState<string>('');
  useEffect(() => { if (!chosen && departments?.length) setChosen(departments[0]!.id); }, [departments, chosen]);
  const byId = useMemo(() => new Map((departments ?? []).map((d) => [d.id, d])), [departments]);
  const [view, reload, error] = useResource<{ items: DeptSetting[] }>(chosen ? `/api/v1/settings/departments/${chosen}` : null);
  const groups = useMemo(() => AREAS.map((a) => ({ ...a, items: (view?.items ?? []).filter((s) => areaOf(s.key) === a.id) })).filter((g) => g.items.length), [view]);

  if (deptError) return <LoadError error={deptError} onRetry={reloadDepts} what="departments" />;
  if (departments === null) return <Skeleton variant="table" rows={4} label="Loading departments" />;
  if (!departments.length) {
    return <Empty title="No departments yet">Add departments in the directory first. Then some settings, such as time zone and review rules, can differ per department.</Empty>;
  }
  const name = (id: string | null) => (id ? byId.get(id)?.name ?? 'a parent department' : '');

  return (
    <div className="scope-panel">
      <p className="scope-intro">Some settings can differ per department. A department uses its parent department’s value unless it has its own, and never goes below the platform minimum.</p>
      <div className="field scope-pick">
        <label className="field-label" htmlFor="scope-dept">Department</label>
        <select id="scope-dept" className="field-input" value={chosen} onChange={(e) => setChosen(e.target.value)}>
          {departments.map((d) => <option key={d.id} value={d.id}>{`${'  '.repeat(Math.max(0, d.path.length - 1))}${d.name}`}</option>)}
        </select>
      </div>
      {error ? <LoadError error={error} onRetry={reload} what="department settings" /> : !view ? <Skeleton variant="table" rows={6} label="Loading department settings" /> : (
        <div className="settings-main">
          {groups.map((g) => (
            <section key={g.id} className="settings-group" aria-labelledby={`dset-${g.id}-h`}>
              <h2 id={`dset-${g.id}-h`}>{g.label}</h2>
              <ul className="settings-list">
                {g.items.map((s) => <OverrideRow key={`${chosen}-${s.key}`} s={s} deptName={byId.get(chosen)?.name ?? ''} inheritedName={name(s.inherited_from)} onSaved={reload} />)}
              </ul>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

function sourceTag(s: DeptSetting, deptName: string, inheritedName: string) {
  if (s.override_revision > 0) return <Tag color="purple">Set for {deptName}</Tag>;
  if (s.inherited_from) return <Tag color="blue">From {inheritedName}</Tag>;
  if (s.source === 'platform_floor') return <Tag color="orange">Platform minimum</Tag>;
  return <Tag>Workspace value</Tag>;
}

function OverrideRow({ s, deptName, inheritedName, onSaved }: { s: DeptSetting; deptName: string; inheritedName: string; onSaved: () => void }) {
  const { can } = useSession();
  const [label, hint] = LABELS[s.key] ?? [humanise(s.key)];
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<unknown>(s.value);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const canEdit = !!s.editable_with && can(s.editable_with);
  const own = s.override_revision > 0;
  const id = `dset-${s.key.replace(/\./g, '-')}`;
  useEffect(() => { if (!editing) setDraft(s.value); }, [s.value, editing]);

  const call = async (method: 'PUT' | 'DELETE') => {
    setBusy(true); setError(null);
    try {
      await api(`/api/v1/settings/departments/${s.department_id}/${s.key}`, { method, headers: { 'if-match': `"${s.override_revision}"` }, ...(method === 'PUT' ? { body: { value: draft } } : {}) });
      setEditing(false); setDone(method === 'PUT' ? 'Saved' : 'Override removed'); onSaved();
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };

  return (
    <li className={`setting${editing ? ' setting-editing' : ''}`}>
      <div className="setting-head">
        <div className="setting-text">
          <p className="setting-label" id={`${id}-l`}>{label}</p>
          {hint && <p className="setting-hint" id={`${id}-h`}>{hint}</p>}
          <p className="setting-meta">{sourceTag(s, deptName, inheritedName)}</p>
        </div>
        {!editing && (
          <div className="setting-value">
            <span className="setting-current">{show(s.kind as Kind, s.value)}</span>
            {done && <span className="setting-saved" role="status"><Check aria-hidden="true" size="0.8125rem" /> {done}</span>}
            {canEdit && <Button variant="secondary" className="btn-sm" onPress={() => { setEditing(true); setError(null); setDone(null); }} aria-label={`${own ? 'Change' : 'Override'} ${label} for ${deptName}`}>{own ? 'Change' : 'Override'}</Button>}
            {canEdit && own && <Button variant="ghost" className="btn-sm" onPress={() => void call('DELETE')} isDisabled={busy} aria-label={`Clear ${label} for ${deptName}`}>Clear</Button>}
          </div>
        )}
      </div>
      {editing && (
        <div className="setting-edit">
          {error && <Notice tone="danger" title={error} />}
          <Control kind={s.kind as Kind} value={draft} onChange={setDraft} labelledBy={`${id}-l`} describedBy={hint ? `${id}-h` : undefined} />
          <div className="actions">
            <Button onPress={() => void call('PUT')} isDisabled={busy || (own && JSON.stringify(draft) === JSON.stringify(s.value))}>{busy ? 'Saving…' : 'Save for this department'}</Button>
            <Button variant="ghost" onPress={() => { setEditing(false); setError(null); }}>Cancel</Button>
          </div>
        </div>
      )}
      {!editing && error && <Notice tone="danger" title={error} />}
    </li>
  );
}

interface PlanItem { key: string; kind: Kind; current: unknown; current_source: string; proposed: unknown; after: unknown; floor: unknown; clamped: boolean; customized: boolean }
interface Plan { from: string; to: string; items: PlanItem[]; basis: string }

const PROFILES: { id: string; name: string; about: string }[] = [
  { id: 'starter', name: 'Starter', about: 'Small teams. Few approvals, longer sessions, the owner publishes.' },
  { id: 'standard', name: 'Standard', about: 'Most companies. One reviewer, two-step verification for admins.' },
  { id: 'enterprise', name: 'Enterprise', about: 'Many departments. Two-step verification for everyone.' },
  { id: 'regulated', name: 'Regulated', about: 'Pharma, finance, public sector. Two reviewers, short sessions, re-acknowledge every version.' },
];

/** Profile switch (docs/20 §0, CFG-017): preview every change, then apply with a reason and step-up. */
export function ProfileSwitch({ onApplied }: { onApplied: () => void }) {
  const { can } = useSession();
  const [current, reloadCurrent, currentError] = useResource<Plan>('/api/v1/settings/profile/plan');
  const [target, setTarget] = useState<string | null>(null);
  const [plan, setPlan] = useState<Plan | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [reason, setReason] = useState('');
  const [busy, setBusy] = useState(false);
  const [stepUp, setStepUp] = useState(false);
  const [applied, setApplied] = useState<string | null>(null);

  const preview = async (to: string) => {
    setTarget(to); setPlan(null); setPlanError(null); setApplied(null);
    try { setPlan(await api<Plan>(`/api/v1/settings/profile/plan?profile=${to}`)); } catch (e) { setPlanError(errorText(e)); }
  };
  const apply = async () => {
    if (!plan) return;
    setBusy(true); setPlanError(null);
    try {
      await api('/api/v1/settings/profile/apply', { method: 'POST', headers: { 'if-match': `"${plan.basis}"` }, body: { profile: plan.to, reason: reason.trim() } });
      const name = PROFILES.find((p) => p.id === plan.to)?.name ?? plan.to;
      setApplied(`This workspace now uses the ${name} profile.`); setPlan(null); setTarget(null); setReason('');
      reloadCurrent(); onApplied();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(true);
      else if (e instanceof ApiError && e.code === 'precondition_failed') setPlanError('Settings changed since this preview. Preview again to see the latest changes.');
      else setPlanError(errorText(e));
    } finally { setBusy(false); }
  };

  if (currentError) return <LoadError error={currentError} onRetry={reloadCurrent} what="the configuration profile" />;
  if (!current) return <Skeleton variant="list" rows={4} label="Loading profile" />;
  const currentName = PROFILES.find((p) => p.id === current.from)?.name ?? current.from;
  const manage = can('workspace:settings.manage');
  const notes = !!plan?.items.some((i) => i.clamped || i.customized);

  return (
    <div className="scope-panel">
      <p className="scope-intro">
        This workspace uses the <strong>{currentName}</strong> profile.{' '}
        {current.items.length ? `${current.items.length} setting${current.items.length === 1 ? ' differs' : 's differ'} from it today.` : 'Every profile setting still matches it.'}{' '}
        Switching shows each change first. Documents, people and history are never touched.
      </p>
      {applied && <Notice tone="success" title={applied} />}
      <fieldset className="profile-fieldset">
        <legend className="field-label">Preview a switch to</legend>
        <div className="profile-choices">
        {PROFILES.map((p) => (
          <label key={p.id} className={`profile-choice${target === p.id ? ' profile-choice-on' : ''}`}>
            <input type="radio" name="profile" value={p.id} checked={target === p.id} onChange={() => void preview(p.id)} />
            <span className="profile-choice-text">
              <span className="profile-choice-name">{p.name}{p.id === current.from && <Tag>Current</Tag>}</span>
              <span className="profile-choice-about">{p.about}</span>
            </span>
          </label>
        ))}
        </div>
      </fieldset>
      {planError && <Notice tone="danger" title={planError} />}
      {target && !plan && !planError && <Skeleton variant="table" rows={4} label="Preparing the preview" />}
      {plan && (
        <section className="settings-group" aria-labelledby="plan-h">
          <h2 id="plan-h">{plan.items.length ? `${plan.items.length} change${plan.items.length === 1 ? '' : 's'} if you switch` : 'Nothing would change'}</h2>
          {plan.items.length > 0 && (
            <div className="table-wrap">
              <table className="table" aria-label="Changes in this switch">
                <thead><tr><th scope="col">Setting</th><th scope="col">Now</th><th scope="col">After the switch</th>{notes && <th scope="col">Note</th>}</tr></thead>
                <tbody>
                  {plan.items.map((i) => (
                    <tr key={i.key}>
                      <td>{(LABELS[i.key] ?? [humanise(i.key)])[0]}</td>
                      <td>{show(i.kind, i.current)}</td>
                      <td>{show(i.kind, i.after)}</td>
                      {notes && (
                        <td className="plan-note">
                          {i.clamped && <Tag color="orange">Raised to the platform minimum</Tag>}
                          {i.customized && <Tag color="yellow">Replaces a value an admin set</Tag>}
                        </td>
                      )}
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {manage && plan.items.length > 0 && (
            <div className="setting-edit">
              <div className="field setting-reason">
                <label className="field-label" htmlFor="plan-reason">Reason for the switch</label>
                <input id="plan-reason" className="field-input" value={reason} onChange={(e) => setReason(e.target.value)} aria-describedby="plan-reason-h" />
                <span className="field-description" id="plan-reason-h">Kept in the audit log. You’ll be asked to confirm it’s you.</span>
              </div>
              <div className="actions">
                <Button onPress={() => void apply()} isDisabled={busy || !reason.trim()}>{busy ? 'Switching…' : `Switch to ${PROFILES.find((p) => p.id === plan.to)?.name ?? plan.to}`}</Button>
                <Button variant="ghost" onPress={() => { setPlan(null); setTarget(null); }}>Cancel</Button>
              </div>
            </div>
          )}
        </section>
      )}
      <StepUpDialog isOpen={stepUp} onClose={() => setStepUp(false)} onDone={() => { setStepUp(false); void apply(); }} />
    </div>
  );
}

