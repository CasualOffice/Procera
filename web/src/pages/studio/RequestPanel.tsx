import { AlignLeft, ArrowDown, ArrowUp, Banknote, Calendar, ChevronRight, CircleAlert, Hash, List, Paperclip, Plus, ToggleLeft, Trash2, Type, UserRound, X, type LucideIcon } from 'lucide-react';
import { useState } from 'react';
import { Form } from 'react-aria-components';
import { Button } from '../../ui/components/Button';
import { Checkbox } from '../../ui/components/Checkbox';
import { FormDialog } from '../../ui/components/FormDialog';
import { Select } from '../../ui/components/Select';
import { TextField } from '../../ui/components/TextField';
import type { Problem } from './api';
import { CATEGORIES, CURRENCIES, FIELD_KINDS, type Category, type Field, type FieldKind, type Model, type StudioRequest } from './model';
import { LongText } from './parts';

const KIND_ICON: Record<FieldKind, LucideIcon> = { string: Type, text: AlignLeft, number: Hash, money: Banknote, date: Calendar, boolean: ToggleLeft, enum: List, principal: UserRound, files: Paperclip, other: Hash };
const kindLabel = (k: FieldKind) => FIELD_KINDS.find((x) => x.id === k)?.label ?? 'Other';
export const PREFIX_RE = /^[A-Z]{2,4}$/;

/**
 * The left column (docs/05 §3.5): what the request is called and where it is listed (the Request
 * card), then the questions people answer (Form fields) — add, edit, reorder and remove.
 */
export function RequestPanel({ model, readOnly, onRequest, onAddField, onField, onMoveField, onRemoveField, open, setOpen, problems }: {
  model: Model; readOnly: boolean;
  onRequest: (patch: Partial<StudioRequest>) => void;
  onAddField: (label: string, kind: Exclude<FieldKind, 'other'>, options: string[], required: boolean) => void;
  onField: (name: string, patch: Partial<Omit<Field, 'name' | 'base'>>) => void;
  onMoveField: (name: string, by: -1 | 1) => void;
  onRemoveField: (name: string) => void;
  open: string | null; setOpen: (name: string | null) => void;
  problems: Map<string, Problem[]>;
}) {
  const [adding, setAdding] = useState(false);
  const r = model.request;
  return (
    <>
      <section className="studio-card" aria-labelledby="studio-request-h">
        <h2 id="studio-request-h" className="studio-card-title">Request</h2>
        <p className="studio-card-sub">How it appears under New request.</p>
        <div className="studio-card-body">
          <TextField label="Request name" value={r.name} onChange={(name) => onRequest({ name })} isReadOnly={readOnly} maxLength={120} isRequired />
          <LongText label="Description" value={r.description} onChange={(description) => onRequest({ description })} isReadOnly={readOnly} rows={2} description="One line on when to use it." />
          <Select label="Category" selectedKey={r.category} isDisabled={readOnly} onSelectionChange={(k) => k && onRequest({ category: k as Category })} options={CATEGORIES} />
          <TextField label="Reference prefix" value={r.prefix} isReadOnly={readOnly} maxLength={4}
            onChange={(v) => onRequest({ prefix: v.toUpperCase().replace(/[^A-Z]/g, '').slice(0, 4) })}
            description={`2 to 4 capital letters. Requests are numbered ${PREFIX_RE.test(r.prefix) ? r.prefix : 'LAP'}-1, ${PREFIX_RE.test(r.prefix) ? r.prefix : 'LAP'}-2…`}
            isInvalid={!!r.prefix && !PREFIX_RE.test(r.prefix)} errorMessage="Use 2 to 4 capital letters." />
        </div>
      </section>

      <section className="studio-card" aria-labelledby="studio-fields-h">
        <div className="studio-card-head">
          <div>
            <h2 id="studio-fields-h" className="studio-card-title">Form fields</h2>
            <p className="studio-card-sub">What people tell you when they ask.</p>
          </div>
          {!readOnly && <Button variant="secondary" className="btn-sm" onPress={() => setAdding(true)}><Plus aria-hidden="true" size="0.875rem" /> Add field</Button>}
        </div>
        {model.fields.length ? (
          <ol className="studio-fields" aria-label="Form fields">
            {model.fields.map((f, i) => {
              const Icon = KIND_ICON[f.kind];
              const isOpen = open === f.name;
              const errs = problems.get(f.name) ?? [];
              return (
                <li key={f.name} className={`studio-field${isOpen ? ' is-open' : ''}${errs.length ? ' has-error' : ''}`}>
                  <button type="button" className="studio-field-row" aria-expanded={isOpen} aria-controls={`field-${f.name}`} onClick={() => setOpen(isOpen ? null : f.name)}>
                    <span className="studio-field-icon" aria-hidden="true"><Icon size="0.8125rem" /></span>
                    <span className="studio-field-label">{f.label || f.name}{f.required && <><span className="field-required" aria-hidden="true"> *</span><span className="visually-hidden"> (required)</span></>}</span>
                    <span className="studio-field-kind">{kindLabel(f.kind)}</span>
                    <ChevronRight aria-hidden="true" size="0.875rem" className="studio-field-chev" />
                  </button>
                  {errs.map((p, j) => <p key={j} className="step-problem"><CircleAlert aria-hidden="true" size="0.875rem" /><span><span className="visually-hidden">Problem: </span>{p.message}</span></p>)}
                  {isOpen && (
                    <div id={`field-${f.name}`} className="studio-field-edit" role="group" aria-label={`Edit ${f.label || f.name}`}>
                      <FieldEditor field={f} readOnly={readOnly} onChange={(p) => onField(f.name, p)} />
                      {!readOnly && (
                        <div className="studio-field-actions">
                          <button type="button" className="icon-btn" aria-label={`Move ${f.label || f.name} up`} disabled={i === 0} onClick={() => onMoveField(f.name, -1)}><ArrowUp aria-hidden="true" size="0.875rem" /></button>
                          <button type="button" className="icon-btn" aria-label={`Move ${f.label || f.name} down`} disabled={i === model.fields.length - 1} onClick={() => onMoveField(f.name, 1)}><ArrowDown aria-hidden="true" size="0.875rem" /></button>
                          <span className="studio-field-var">Saved as <code>{f.name}</code></span>
                          <Button variant="ghost" className="btn-sm props-delete" onPress={() => onRemoveField(f.name)}><Trash2 aria-hidden="true" size="0.8125rem" /> Remove field</Button>
                        </div>
                      )}
                    </div>
                  )}
                </li>
              );
            })}
          </ol>
        ) : (
          <p className="studio-card-empty">No questions yet. Add the first thing people should tell you, like what they need and why.</p>
        )}
      </section>
      {adding && <AddField onClose={() => setAdding(false)} onAdd={(l, k, o, req) => { onAddField(l, k, o, req); setAdding(false); }} taken={new Set(model.fields.map((f) => f.label.trim().toLowerCase()))} />}
    </>
  );
}

function FieldEditor({ field: f, readOnly, onChange }: { field: Field; readOnly: boolean; onChange: (p: Partial<Omit<Field, 'name' | 'base'>>) => void }) {
  return (
    <>
      <TextField label="Label" value={f.label} onChange={(label) => onChange({ label })} isReadOnly={readOnly} maxLength={120} isRequired />
      <TextField label="Help text (optional)" value={f.help} onChange={(help) => onChange({ help })} isReadOnly={readOnly} maxLength={300} />
      {f.kind === 'other' ? <p className="props-note">This field’s type comes from the template and can’t be changed here.</p> : (
        <Select label="Type" selectedKey={f.kind} isDisabled={readOnly} onSelectionChange={(k) => k && onChange({ kind: k as FieldKind, ...(k === 'enum' && !f.options.length ? { options: ['', ''] } : {}) })} options={FIELD_KINDS.map((k) => ({ id: k.id, label: k.label }))} />
      )}
      {f.kind === 'money' && <Select label="Currency" selectedKey={f.currency} isDisabled={readOnly} onSelectionChange={(k) => k && onChange({ currency: String(k) })} options={CURRENCIES.map((c) => ({ id: c, label: c }))} />}
      {f.kind === 'enum' && (
        <div className="studio-options" role="group" aria-label="Options">
          <p className="field-label">Options</p>
          {f.options.map((o, i) => (
            <div key={i} className="studio-option">
              <TextField label={`Option ${i + 1}`} value={o} isReadOnly={readOnly} onChange={(v) => onChange({ options: f.options.map((x, j) => (j === i ? v : x)) })} />
              {!readOnly && f.options.length > 1 && <button type="button" className="icon-btn" aria-label={`Remove option ${i + 1}`} onClick={() => onChange({ options: f.options.filter((_, j) => j !== i) })}><X aria-hidden="true" size="0.875rem" /></button>}
            </div>
          ))}
          {!readOnly && <Button variant="ghost" className="props-add" onPress={() => onChange({ options: [...f.options, ''] })}><Plus aria-hidden="true" size="0.875rem" /> Add option</Button>}
        </div>
      )}
      <Checkbox isSelected={f.required} isDisabled={readOnly} onChange={(required) => onChange({ required })}>Required</Checkbox>
    </>
  );
}

function AddField({ onClose, onAdd, taken }: { onClose: () => void; onAdd: (label: string, kind: Exclude<FieldKind, 'other'>, options: string[], required: boolean) => void; taken: Set<string> }) {
  const [label, setLabel] = useState('');
  const [kind, setKind] = useState<Exclude<FieldKind, 'other'>>('string');
  const [options, setOptions] = useState('');
  const [required, setRequired] = useState(false);
  const [touched, setTouched] = useState(false);
  const opts = options.split('\n').map((s) => s.trim()).filter(Boolean);
  const labelProblem = !label.trim() ? 'Give the field a label.' : taken.has(label.trim().toLowerCase()) ? 'The form already has a field with this label.' : null;
  const optsProblem = kind === 'enum' && opts.length < 2 ? 'Write at least two options, one per line.' : kind === 'enum' && new Set(opts).size !== opts.length ? 'Each option must be different.' : null;
  const submit = () => {
    setTouched(true);
    if (labelProblem || optsProblem) return;
    onAdd(label.trim(), kind, opts, required);
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Add a field" description="A question on the request form."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={submit}>Add field</Button></>}>
      <Form validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); submit(); }} style={{ display: 'contents' }}>
        <TextField label="Label" value={label} onChange={setLabel} autoFocus isRequired maxLength={120} description="What the form asks, e.g. “Model”."
          isInvalid={touched && !!labelProblem} errorMessage={touched ? labelProblem ?? undefined : undefined} />
        <Select label="Type" selectedKey={kind} onSelectionChange={(k) => k && setKind(k as Exclude<FieldKind, 'other'>)}
          description={FIELD_KINDS.find((k) => k.id === kind)?.hint} options={FIELD_KINDS.map((k) => ({ id: k.id, label: k.label }))} />
        {kind === 'enum' && (
          <div className="field">
            <LongText label="Options" value={options} onChange={setOptions} description="One per line." rows={4} />
            {touched && optsProblem && <p className="field-error" role="alert">{optsProblem}</p>}
          </div>
        )}
        <Checkbox isSelected={required} onChange={setRequired}>Required</Checkbox>
      </Form>
    </FormDialog>
  );
}
