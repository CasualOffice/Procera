import { Check, ChevronDown } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { useItems, useResource } from '../lib/data';
import { useSession } from '../lib/session';
import { Tag } from '../ui/Tag';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { Drawer } from '../ui/Drawer';
import { StepUpDialog } from '../ui/StepUp';
import { LoadError, Skeleton } from '../ui/States';
import { Control, type Kind, type Setting, show } from './Settings';
import { labelOf } from './settingsMeta';

interface Department { id: string; name: string; parent_id: string | null; path: string[] }
interface DeptSetting extends Setting { department_id: string; override_value: unknown | null; override_revision: number; inherited_from: string | null }

const errorText = (e: unknown) => {
  if (e instanceof ApiError && e.code === 'precondition_failed') return 'Someone else changed this. Reload the page to see the new value.';
  return e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not save. Try again.';
};

export interface DepartmentScope { departments: Department[]; views: Map<string, DeptSetting[]> | null; reload: () => void }

/**
 * Every department's view of the department-overridable keys (docs/20 §1 layer 4), one request per
 * department, so each setting can say whether it differs anywhere. Only loaded when a page has such keys.
 */
export function useDepartmentScope(enabled: boolean): DepartmentScope {
  const [departments] = useItems<Department>(enabled ? '/api/v1/departments' : null);
  const [views, setViews] = useState<Map<string, DeptSetting[]> | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (departments === null) return;
    if (!departments.length) { setViews(new Map()); return; }
    let live = true;
    void Promise.all(departments.map((d) => api<{ items: DeptSetting[] }>(`/api/v1/settings/departments/${d.id}`)
      .then((r) => [d.id, r.items] as const)
      .catch(() => [d.id, [] as DeptSetting[]] as const)))
      .then((rows) => { if (live) setViews(new Map(rows)); });
    return () => { live = false; };
  }, [departments, tick]);
  return { departments: departments ?? [], views, reload: () => setTick((t) => t + 1) };
}

/**
 * Department overrides presented as Google's Inherited / Overridden model
 * (docs/design/ia-competitive-analysis.md §6.4): a chip under the setting, and a side panel listing
 * each department with its value, "Override" and "Use workspace value".
 */
export function DepartmentChip({ s, scope }: { s: Setting; scope: DepartmentScope }) {
  const [open, setOpen] = useState(false);
  const { departments, views } = scope;
  if (!departments.length || !views) return null;
  const [label] = labelOf(s.key);
  const n = departments.filter((d) => (views.get(d.id)?.find((x) => x.key === s.key)?.override_revision ?? 0) > 0).length;
  const text = n ? `Different in ${n} department${n === 1 ? '' : 's'}` : 'Same for all departments';
  return (
    <>
      <button type="button" className={`setting-why dept-chip${n ? ' dept-chip-on' : ''}`} aria-label={`${text}: ${label}`} onClick={() => setOpen(true)}>{text}<ChevronDown aria-hidden="true" size="0.75rem" /></button>
      {open && <DepartmentPanel s={s} scope={scope} onClose={() => setOpen(false)} />}
    </>
  );
}

function DepartmentPanel({ s, scope, onClose }: { s: Setting; scope: DepartmentScope; onClose: () => void }) {
  const [label] = labelOf(s.key);
  const { departments, views, reload } = scope;
  const byId = useMemo(() => new Map(departments.map((d) => [d.id, d])), [departments]);
  const entry = (deptId: string | null) => (deptId ? views?.get(deptId)?.find((x) => x.key === s.key) : undefined);
  return (
    <Drawer title={`${label} by department`} onClose={onClose}>
      <p className="scope-intro">Departments use the workspace settings unless you change them here. Sub-departments follow their parent.</p>
      <p className="scope-intro">Workspace value: <strong>{show(s.kind, s.value)}</strong></p>
      {!views ? <Skeleton variant="list" rows={3} label="Loading departments" /> : (
        <ul className="settings-list" aria-label={`${label} in each department`}>
          {departments.map((d) => {
            const e = entry(d.id);
            if (!e) return null;
            const parent = d.parent_id ? byId.get(d.parent_id) : undefined;
            const parentEntry = entry(d.parent_id);
            // Clearing an override falls back to the nearest ancestor that sets one, else the workspace.
            const fallback = parentEntry && parentEntry.override_revision > 0 ? parent?.name : parentEntry?.inherited_from ? byId.get(parentEntry.inherited_from)?.name : undefined;
            return <OverrideRow key={d.id} s={e} label={label} dept={d} depth={Math.max(0, d.path.length - 1)} inheritedName={e.inherited_from ? byId.get(e.inherited_from)?.name ?? 'a parent department' : ''} fallback={fallback} onSaved={reload} />;
          })}
        </ul>
      )}
    </Drawer>
  );
}

function sourceTag(s: DeptSetting, deptName: string, inheritedName: string) {
  if (s.override_revision > 0) return <Tag color="purple">Set for {deptName}</Tag>;
  if (s.inherited_from) return <Tag color="blue">Inherited from {inheritedName}</Tag>;
  if (s.source === 'platform_floor') return <Tag color="orange">Platform minimum</Tag>;
  return <Tag>Inherited from workspace</Tag>;
}

function OverrideRow({ s, label, dept, depth, inheritedName, fallback, onSaved }: { s: DeptSetting; label: string; dept: Department; depth: number; inheritedName: string; fallback?: string; onSaved: () => void }) {
  const { can } = useSession();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<unknown>(s.value);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const canEdit = !!s.editable_with && can(s.editable_with);
  const own = s.override_revision > 0;
  const id = `dset-${dept.id}`;
  useEffect(() => { if (!editing) setDraft(s.value); }, [s.value, editing]);
  const inherit = fallback ? `Use ${fallback} value` : 'Use workspace value';

  const call = async (method: 'PUT' | 'DELETE') => {
    setBusy(true); setError(null);
    try {
      await api(`/api/v1/settings/departments/${s.department_id}/${s.key}`, { method, headers: { 'if-match': `"${s.override_revision}"` }, ...(method === 'PUT' ? { body: { value: draft } } : {}) });
      setEditing(false); setDone(method === 'PUT' ? 'Saved' : 'Override removed'); onSaved();
    } catch (e) { setError(errorText(e)); } finally { setBusy(false); }
  };

  return (
    <li className={`setting${editing ? ' setting-editing' : ''}`} style={depth ? { paddingLeft: `${1 + depth * 1.25}rem` } : undefined}>
      <div className="setting-head">
        <div className="setting-text">
          <p className="setting-label" id={`${id}-l`}>{dept.name}</p>
          <span className="visually-hidden" id={`${id}-c`}>{label} for {dept.name}</span>
          <p className="setting-meta">{sourceTag(s, dept.name, inheritedName)}</p>
        </div>
        {!editing && (
          <div className="setting-value">
            <span className="setting-current">{show(s.kind as Kind, s.value)}</span>
            {done && <span className="setting-saved" role="status"><Check aria-hidden="true" size="0.8125rem" /> {done}</span>}
            {canEdit && <Button variant="secondary" className="btn-sm" onPress={() => { setEditing(true); setError(null); setDone(null); }} aria-label={`${own ? 'Change' : 'Override'} ${label} for ${dept.name}`}>{own ? 'Change' : 'Override'}</Button>}
            {canEdit && own && <Button variant="ghost" className="btn-sm" onPress={() => void call('DELETE')} isDisabled={busy} aria-label={`${inherit} for ${dept.name}`}>{inherit}</Button>}
          </div>
        )}
      </div>
      {editing && (
        <div className="setting-edit">
          {error && <Notice tone="danger" title={error} />}
          <Control kind={s.kind as Kind} value={draft} onChange={setDraft} labelledBy={`${id}-c`} />
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

/**
 * The configuration profile, shown once as "Starting point" on the Settings index (§6.5). The switch
 * keeps its full preview, reason and step-up, in a side panel.
 */
export function StartingPoint({ onApplied }: { onApplied: () => void }) {
  const { can } = useSession();
  const [current, reload, error] = useResource<Plan>('/api/v1/settings/profile/plan');
  const [open, setOpen] = useState(false);
  if (error) return <LoadError error={error} onRetry={reload} what="the starting point" />;
  if (!current) return <Skeleton variant="block" label="Loading the starting point" />;
  const name = PROFILES.find((p) => p.id === current.from)?.name ?? current.from;
  const n = current.items.length;
  const manage = can('workspace:settings.manage');
  return (
    <section className="starting-point" aria-labelledby="sp-h">
      <div className="starting-point-text">
        <h2 id="sp-h">Starting point: {name}</h2>
        <p>This workspace started from the {name} settings. {n ? `${n} setting${n === 1 ? ' differs' : 's differ'} from it today.` : 'Every setting still matches it.'} You can change any setting.</p>
      </div>
      <Button variant="secondary" onPress={() => setOpen(true)}>{manage ? 'Change starting point' : 'Compare starting points'}</Button>
      {open && <Drawer title="Change starting point" onClose={() => setOpen(false)}><ProfileSwitch onApplied={() => { reload(); onApplied(); }} /></Drawer>}
    </section>
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
      setApplied(`This workspace now uses the ${name} starting point.`); setPlan(null); setTarget(null); setReason('');
      reloadCurrent(); onApplied();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(true);
      else if (e instanceof ApiError && e.code === 'precondition_failed') setPlanError('Settings changed since this preview. Preview again to see the latest changes.');
      else setPlanError(errorText(e));
    } finally { setBusy(false); }
  };

  if (currentError) return <LoadError error={currentError} onRetry={reloadCurrent} what="the starting point" />;
  if (!current) return <Skeleton variant="list" rows={4} label="Loading profile" />;
  const currentName = PROFILES.find((p) => p.id === current.from)?.name ?? current.from;
  const manage = can('workspace:settings.manage');
  const notes = !!plan?.items.some((i) => i.clamped || i.customized);

  return (
    <div className="scope-panel">
      <p className="scope-intro">
        This workspace uses the <strong>{currentName}</strong> starting point.{' '}
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
                      <td>{labelOf(i.key)[0]}</td>
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

