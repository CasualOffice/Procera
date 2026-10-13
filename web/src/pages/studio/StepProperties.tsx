import { CornerDownRight, Info, Plus, Trash2, Users, X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { type Artifact, type Member, useItems } from '../../lib/data';
import type { Group } from '../../lib/directory';
import type { Role } from '../../lib/roles';
import { Button } from '../../ui/components/Button';
import { Checkbox } from '../../ui/components/Checkbox';
import { Select } from '../../ui/components/Select';
import { TextField } from '../../ui/components/TextField';
import type { Problem } from './api';
import {
  type Condition, type Model, type Step, type Who, type WhoType, STEP_KINDS, WHO_TYPES, candidateOf, conditionFields, emptyWho, routes,
} from './model';
import { KIND_LOOK, Suggest, daysValue } from './parts';

export interface Directory { groups: Group[]; roles: Role[]; members: Member[] }

/**
 * The right column (docs/05 §3.5): the selected step's Basics, Who, Only when, Deadline &
 * reminders and Outcomes. Changes apply as you type and autosave with the draft.
 */
export function StepProperties({ model, step, onChange, onDelete, directory, problems, readOnly }: {
  model: Model; step: Step | null; onChange: (patch: Partial<Omit<Step, 'id' | 'base'>>) => void; onDelete: () => void;
  directory: Directory; problems: Problem[]; readOnly: boolean;
}) {
  if (!step) {
    return (
      <div className="props-empty">
        <h2 id="props-h" className="props-title">Step settings</h2>
        <p>Select a step to choose who does it, its deadline and where each outcome goes.</p>
        {!readOnly && <p className="muted">Tip: with a step focused, Alt+↑ and Alt+↓ move it.</p>}
      </div>
    );
  }
  const look = KIND_LOOK[step.kind];
  return (
    <div className="step-props" key={step.uid}>
      <div className="props-head">
        <span className={`step-kind tag tag-${look.color}`}><look.Icon aria-hidden="true" size="0.75rem" />{look.label}</span>
        <h2 id="props-h" className="props-title">{step.name.trim() || 'Untitled step'}</h2>
      </div>
      {problems.filter((p) => p.severity === 'error').length > 0 && (
        <ul className="props-problems" aria-label="Problems with this step">
          {problems.map((p, i) => <li key={i}>{p.message}</li>)}
        </ul>
      )}
      <fieldset className="props-section" disabled={readOnly}>
        <legend>Basics</legend>
        <TextField label="Step name" value={step.name} onChange={(name) => onChange({ name })} isReadOnly={readOnly} maxLength={120} isRequired />
        <Select label="Kind of step" selectedKey={step.kind} isDisabled={readOnly}
          onSelectionChange={(k) => k && onChange({ kind: k as Step['kind'] })} options={STEP_KINDS.map((k) => ({ id: k.id, label: k.label }))} />
      </fieldset>
      <WhoSection step={step} onChange={onChange} directory={directory} readOnly={readOnly} />
      {step.kind === 'acknowledge' && <SubjectSection step={step} onChange={onChange} readOnly={readOnly} />}
      <WhenSection model={model} step={step} onChange={onChange} readOnly={readOnly} />
      <DeadlineSection step={step} onChange={onChange} readOnly={readOnly} />
      <section className="props-section" aria-labelledby="props-outcomes">
        <h3 id="props-outcomes" className="props-legend">Outcomes</h3>
        <ul className="props-routes">
          {routes(model, step).map((r) => (
            <li key={r.outcome}><span className="step-route-label">{r.label}</span><CornerDownRight aria-hidden="true" size="0.75rem" /><span>{r.to}</span></li>
          ))}
        </ul>
        <p className="props-note"><Info aria-hidden="true" size="0.8125rem" />
          {step.kind === 'approval' ? 'Approvals always offer approve, reject and send back. Rejecting and sending back ask for a reason.' : step.kind === 'task' ? 'The person marks the task done, and the request moves on.' : 'The person confirms, and the request moves on. If the deadline passes first, it moves on too.'}
        </p>
      </section>
      {!readOnly && (
        <div className="props-foot">
          <Button variant="ghost" className="props-delete" onPress={onDelete}><Trash2 aria-hidden="true" size="0.875rem" /> Delete step</Button>
        </div>
      )}
    </div>
  );
}

const ROW_LABEL = (i: number) => (i === 0 ? 'Who' : i === 1 ? 'Or also' : `Or also (${i + 1})`);

interface Reach { count: number | null; people: string[]; note: string | null }

/** Who each row reaches in the directory today (BPMS-100 “who will this go to”), refreshed as it is edited. */
function useReach(who: Who[]): (Reach | null)[] {
  const asks = who.map(candidateOf);
  const key = JSON.stringify(asks);
  const [reach, setReach] = useState<{ key: string; items: (Reach | null)[] }>({ key: '', items: [] });
  useEffect(() => {
    const filled = asks.filter((c) => c.ref.trim());
    if (!filled.length) return;
    let live = true;
    const t = window.setTimeout(() => {
      api<{ items: Reach[] }>('/api/v1/processes/who', { method: 'POST', body: { candidates: filled } })
        .then((r) => {
          if (!live) return;
          let i = 0;
          setReach({ key, items: asks.map((c) => (c.ref.trim() ? (r.items[i++] ?? null) : null)) });
        })
        .catch(() => live && setReach({ key, items: [] }));
    }, 350);
    return () => { live = false; window.clearTimeout(t); };
  }, [key]);
  return reach.key === key ? reach.items : [];
}

function reachText(r: Reach): string {
  if (r.count === null) return r.note ?? '';
  if (r.count === 0) return r.note ?? 'Nobody yet.';
  const names = r.people.join(', ');
  if (r.count === 1) return `Goes to ${names}.`;
  const more = r.count - r.people.length;
  return `Goes to ${r.count} people: ${names}${more > 0 ? ` and ${more} more` : ''}.`;
}

function WhoSection({ step, onChange, directory, readOnly }: { step: Step; onChange: (p: Partial<Step>) => void; directory: Directory; readOnly: boolean }) {
  const set = (i: number, w: Who) => onChange({ who: step.who.map((x, j) => (j === i ? w : x)) });
  const rows = step.who.length ? step.who : [];
  const reach = useReach(rows);
  return (
    <fieldset className="props-section" disabled={readOnly}>
      <legend>Who</legend>
      {rows.map((w, i) => (
        <div key={i} className="who-row">
          <div className="who-row-main">
            <Select label={ROW_LABEL(i)} selectedKey={w.type} isDisabled={readOnly}
              onSelectionChange={(k) => k && k !== w.type && set(i, emptyWho(k as WhoType))}
              options={[...WHO_TYPES.map((t) => ({ id: t.id, label: t.label })), ...(w.type === 'custom' ? [{ id: 'custom', label: 'A custom rule (from the template)' }] : [])]} />
            {w.type === 'group' && (
              <Suggest label="Group" value={w.name} onChange={(name) => set(i, { type: 'group', name })} placeholder="e.g. IT support"
                items={directory.groups.map((g) => ({ id: g.name, label: g.name, detail: `${g.member_count} ${g.member_count === 1 ? 'person' : 'people'}` }))}
                description="Anyone in the group can pick it up." />
            )}
            {w.type === 'role' && (
              <Select label="Role" selectedKey={w.key || null} placeholder="Choose a role" isDisabled={readOnly}
                onSelectionChange={(k) => k && set(i, { type: 'role', key: String(k) })} options={directory.roles.map((r) => ({ id: r.key, label: r.name }))} />
            )}
            {w.type === 'person' && (
              <Suggest label="Email" value={w.email} onChange={(email) => set(i, { type: 'person', email })} placeholder="name@company.com"
                items={directory.members.filter((m) => m.status === 'active').map((m) => ({ id: m.email, label: m.email, detail: m.display_name }))} />
            )}
            {w.type === 'position' && <TextField label="Position" value={w.title} onChange={(title) => set(i, { type: 'position', title })} placeholder="e.g. CFO" isReadOnly={readOnly} />}
            {w.type === 'custom' && <p className="props-code"><code>{w.candidate.type}: {w.candidate.ref}</code></p>}
            {reach[i] && (
              <p className={`who-reach${reach[i]?.count === 0 ? ' is-empty' : ''}`} aria-live="polite" data-testid="who-reach">
                <Users aria-hidden="true" size="0.8125rem" /> {reachText(reach[i]!)}
              </p>
            )}
          </div>
          {rows.length > 1 && !readOnly && (
            <button type="button" className="icon-btn" aria-label={`Remove “${ROW_LABEL(i)}”`} onClick={() => onChange({ who: step.who.filter((_, j) => j !== i) })}><X aria-hidden="true" size="0.875rem" /></button>
          )}
        </div>
      ))}
      {!readOnly && (
        <Button variant="ghost" className="props-add" onPress={() => onChange({ who: [...step.who, { type: 'group', name: '' }] })}><Plus aria-hidden="true" size="0.875rem" /> Add someone else</Button>
      )}
      <p className="props-note"><Info aria-hidden="true" size="0.8125rem" />
        {`If nobody is found, it goes to the process owner.${step.kind === 'approval' ? ' The requester never decides their own request.' : ''}`}
      </p>
    </fieldset>
  );
}

function WhenSection({ model, step, onChange, readOnly }: { model: Model; step: Step; onChange: (p: Partial<Step>) => void; readOnly: boolean }) {
  const choices = conditionFields(model);
  const c = step.when;
  if (c && 'raw' in c) {
    return (
      <section className="props-section" aria-labelledby="props-when">
        <h3 id="props-when" className="props-legend">Only when</h3>
        <p className="props-note">This step runs only when a custom condition from the template holds:</p>
        <p className="props-code"><code>{c.raw.replace(/^expr:cel:\s*/, '')}</code></p>
        {!readOnly && <Button variant="secondary" onPress={() => onChange({ when: null })}>Remove condition</Button>}
      </section>
    );
  }
  const current = c ? choices.find((x) => x.field.name === c.field) : undefined;
  const set = (patch: Partial<Extract<Condition, { field: string }>>) => c && onChange({ when: { ...c, ...patch } });
  return (
    <fieldset className="props-section" disabled={readOnly}>
      <legend>Only when</legend>
      <Checkbox isSelected={!!c} isDisabled={readOnly || (!c && !choices.length)}
        onChange={(on) => onChange({ when: on && choices[0] ? { field: choices[0].field.name, op: choices[0].op, value: choices[0].field.kind === 'boolean' ? 'yes' : '' } : null })}>
        Only when a condition is met
      </Checkbox>
      {!c && !choices.length && <p className="props-note">Add a number, money, choice, yes/no or short text field to the form to make a step conditional.</p>}
      {c && (
        <>
          <Select label="When" selectedKey={c.field} isDisabled={readOnly}
            onSelectionChange={(k) => { const hit = choices.find((x) => x.field.name === k); if (hit) onChange({ when: { field: hit.field.name, op: hit.op, value: hit.field.kind === 'boolean' ? 'yes' : '' } }); }}
            options={choices.map((x) => ({ id: x.field.name, label: x.field.label || x.field.name }))} />
          {current?.op === 'over' && (
            <TextField label="Is over" value={c.value} onChange={(value) => set({ value: value.replace(/[^\d.]/g, '') })} inputMode="decimal" isReadOnly={readOnly}
              description={current.field.kind === 'money' ? `In ${current.field.currency}. Otherwise the step is skipped.` : 'Otherwise the step is skipped.'}
              isInvalid={!!c.value && !/^\d+(\.\d+)?$/.test(c.value)} errorMessage="Enter a number, like 1500." />
          )}
          {current?.op === 'equals' && current.field.kind === 'enum' && (
            <Select label="Is" selectedKey={c.value || null} placeholder="Choose an option" isDisabled={readOnly} onSelectionChange={(k) => k && set({ value: String(k) })}
              options={current.field.options.filter(Boolean).map((o) => ({ id: o, label: o }))} />
          )}
          {current?.op === 'equals' && current.field.kind === 'boolean' && (
            <Select label="Is" selectedKey={c.value || 'yes'} isDisabled={readOnly} onSelectionChange={(k) => k && set({ value: String(k) })} options={[{ id: 'yes', label: 'Yes' }, { id: 'no', label: 'No' }]} />
          )}
          {current?.op === 'equals' && current.field.kind === 'string' && (
            <TextField label="Is" value={c.value} onChange={(value) => set({ value })} isReadOnly={readOnly} description="Exactly this text. Otherwise the step is skipped." />
          )}
          {!current && <p className="props-note">The field this condition read is gone. Choose another.</p>}
        </>
      )}
    </fieldset>
  );
}

/** A whole number of days in a text box that keeps what you type while it isn't valid yet. */
function DaysField({ label, value, onChange, description, readOnly, max = 60 }: { label: string; value: number; onChange: (n: number) => void; description?: string; readOnly: boolean; max?: number }) {
  const [text, setText] = useState(String(value));
  useEffect(() => { if (daysValue(text, 1, max) !== value) setText(String(value)); }, [value]); // eslint-disable-line react-hooks/exhaustive-deps
  const n = daysValue(text, 1, max);
  return (
    <TextField label={label} value={text} inputMode="numeric" isReadOnly={readOnly} description={description}
      onChange={(t) => { setText(t); const v = daysValue(t, 1, max); if (v !== null) onChange(v); }}
      isInvalid={n === null} errorMessage={`Enter a whole number from 1 to ${max}.`} />
  );
}

const range = (n: number) => Array.from({ length: Math.max(1, n) }, (_, i) => i + 1);
const same = (a: number[], b: number[]) => a.length === b.length && a.every((x, i) => x === b[i]);

function DeadlineSection({ step, onChange, readOnly }: { step: Step; onChange: (p: Partial<Step>) => void; readOnly: boolean }) {
  const due = step.dueDays;
  const daily = range(due ?? 3);
  const mode = !step.reminders.length ? 'none' : same(step.reminders, [1]) ? 'one' : same(step.reminders, daily) ? 'daily' : 'custom';
  const setDue = (d: number | null) => onChange({ dueDays: d, dueRaw: null, ...(mode === 'daily' ? { reminders: range(d ?? 3) } : {}) });
  return (
    <fieldset className="props-section" disabled={readOnly}>
      <legend>Deadline &amp; reminders</legend>
      {step.dueRaw && <p className="props-note">The template sets a deadline of <code>{step.dueRaw.after}</code>. Set one in days to replace it.</p>}
      <Checkbox isSelected={due !== null} isDisabled={readOnly} onChange={(on) => setDue(on ? 2 : null)}>Set a deadline</Checkbox>
      {due !== null && <DaysField label="Business days to finish" value={due} onChange={(d) => setDue(d)} readOnly={readOnly} description="Counts working days on the workspace calendar." />}
      <Select label="Reminders" selectedKey={mode} isDisabled={readOnly}
        onSelectionChange={(k) => { if (k === 'none') onChange({ reminders: [] }); else if (k === 'one') onChange({ reminders: [1] }); else if (k === 'daily') onChange({ reminders: daily }); }}
        options={[
          { id: 'none', label: 'No reminders' },
          { id: 'one', label: 'One reminder after a day' },
          { id: 'daily', label: due !== null ? 'Every day until it’s due' : 'Every day for three days' },
          ...(mode === 'custom' ? [{ id: 'custom', label: `After ${step.reminders.join(', ')} days (from the template)` }] : []),
        ]} />
      <Checkbox isSelected={!!step.escalate} isDisabled={readOnly} onChange={(on) => onChange({ escalate: on ? { days: (due ?? 2) + 1, action: 'reassign' } : null })}>
        If it’s still open, go one level up
      </Checkbox>
      {step.escalate && (
        <>
          <DaysField label="After how many days" value={step.escalate.days} onChange={(days) => onChange({ escalate: { ...step.escalate!, days } })} readOnly={readOnly} />
          <Select label="Then" selectedKey={step.escalate.action} isDisabled={readOnly} onSelectionChange={(k) => k && onChange({ escalate: { ...step.escalate!, action: k as 'notify' | 'reassign' } })}
            options={[{ id: 'reassign', label: 'Move it to their manager’s manager' }, { id: 'notify', label: 'Tell their manager’s manager' }]} />
        </>
      )}
    </fieldset>
  );
}

function SubjectSection({ step, onChange, readOnly }: { step: Step; onChange: (p: Partial<Step>) => void; readOnly: boolean }) {
  const [docs] = useItems<Artifact>('/api/v1/artifacts?state=published');
  const [error, setError] = useState<string | null>(null);
  const published = (docs ?? []).filter((d) => (d.kind === 'policy' || d.kind === 'sop') && d.current_published_version_id);
  const chosen = step.subject ? published.find((d) => step.subject!.ref.startsWith(`${d.kind}:${d.key}@`)) : undefined;
  const pick = async (id: string) => {
    const d = published.find((x) => x.id === id);
    if (!d) return;
    try {
      const r = await api<{ versions: { id: string; version_no: number }[] }>(`/api/v1/artifacts/${d.id}`);
      const v = r.versions.find((x) => x.id === d.current_published_version_id);
      if (!v) throw new Error('no published version');
      setError(null);
      onChange({ subject: { ref: `${d.kind}:${d.key}@${v.version_no}`, title: d.title } });
    } catch { setError('Couldn’t read that document’s published version. Try again.'); }
  };
  return (
    <fieldset className="props-section" disabled={readOnly}>
      <legend>What they confirm</legend>
      <Select label="Document" placeholder={docs === null ? 'Loading…' : published.length ? 'Choose a published document' : 'No published documents yet'} isDisabled={readOnly || !published.length}
        selectedKey={chosen?.id ?? null} onSelectionChange={(k) => k && void pick(String(k))}
        options={published.map((d) => ({ id: d.id, label: d.title }))} errorMessage={error ?? undefined} isInvalid={!!error} />
      {step.subject && !chosen && <p className="props-note">From the template: <code>{step.subject.ref}</code></p>}
      <p className="props-note"><Info aria-hidden="true" size="0.8125rem" />They confirm the version published now; a newer one needs a new process version.</p>
    </fieldset>
  );
}
