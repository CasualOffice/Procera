import { FieldError, Input, Label, NumberField, Text, TextArea, TextField as AriaTextField } from 'react-aria-components';
import type { Member } from '../lib/data';
import type { FormField } from '../lib/requests';
import { Checkbox } from '../ui/components/Checkbox';
import { RadioCards } from '../ui/components/RadioCards';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';
import { FileField, type PickedFile, fileProblem, pickedFrom } from './requestFiles';

/*
 * A request form rendered from its schema (BPMS-103): one React Aria control per field type. Errors
 * come from the surrounding <Form validationErrors> by field name (client required checks and the
 * server's 422 field errors use the same map), so every message sits on its field.
 */

export type Values = Record<string, unknown>;

const Required = ({ on }: { on?: boolean }) => (on ? <span aria-hidden="true" className="field-required"> *</span> : null);

/** Form state from submitted input (prefill on resubmit): money objects become plain amounts. */
export function valuesFrom(fields: FormField[], input: Record<string, unknown> = {}): Values {
  const out: Values = {};
  for (const f of fields) {
    const v = input[f.name];
    if (f.type === 'file') out[f.name] = pickedFrom(v);
    else if (f.type === 'money') out[f.name] = v && typeof v === 'object' ? Number((v as { amount?: string }).amount) : v === undefined || v === null || v === '' ? NaN : Number(v);
    else if (f.type === 'number') out[f.name] = v === undefined || v === null || v === '' ? NaN : Number(v);
    else if (f.type === 'boolean') out[f.name] = v === true;
    else out[f.name] = v === undefined || v === null ? '' : String(v);
  }
  return out;
}

const empty = (f: FormField, v: unknown) => (f.type === 'file' ? !Array.isArray(v) || v.length === 0 : f.type === 'number' || f.type === 'money' ? typeof v !== 'number' || Number.isNaN(v) : f.type === 'boolean' ? v !== true : v === undefined || v === null || String(v).trim() === '');

/** Client-side required checks: `{field: message}` for each required field left empty. */
export function requiredErrors(fields: FormField[], values: Values): Record<string, string> {
  const out: Record<string, string> = {};
  for (const f of fields) if (f.required && empty(f, values[f.name])) out[f.name] = f.type === 'boolean' ? 'Tick this to continue.' : f.type === 'file' ? 'Attach a file.' : `Enter ${f.label.toLowerCase()}.`;
  // Files still scanning (or blocked) hold the form back, whether the field is required or not.
  for (const f of fields) {
    const p = f.type === 'file' && Array.isArray(values[f.name]) ? fileProblem(values[f.name] as PickedFile[]) : null;
    if (p && !out[f.name]) out[f.name] = p;
  }
  return out;
}

/** The API's `input`: money as `{amount, currency}` (docs/08 §1) when the currency is known, empty values left out. */
export function toInput(fields: FormField[], values: Values): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const f of fields) {
    const v = values[f.name];
    // File fields always go (an empty list clears them on a resubmit).
    if (f.type === 'file') { const ids = (Array.isArray(v) ? (v as PickedFile[]) : []).flatMap((x) => (x.id ? [x.id] : [])); out[f.name] = f.multiple === false ? (ids[0] ?? null) : ids; continue; }
    if (f.type !== 'boolean' && empty(f, v)) continue;
    if (f.type === 'money') out[f.name] = f.currency ? { amount: String(v), currency: f.currency } : String(v);
    else if (f.type === 'string' || f.type === 'text') out[f.name] = String(v).trim();
    else out[f.name] = v;
  }
  return out;
}

export function RequestFields({ fields, values, onChange, members, errors = {}, idPrefix = 'f' }: {
  fields: FormField[];
  values: Values;
  onChange: (name: string, value: unknown) => void;
  members: Member[] | null;
  /** Errors for controls that can't read the form's validation context (checkboxes). */
  errors?: Record<string, string>;
  idPrefix?: string;
}) {
  return (
    <>
      {fields.map((f) => {
        const v = values[f.name];
        const common = { name: f.name, isRequired: !!f.required };
        switch (f.type) {
          case 'text':
            return (
              <AriaTextField key={f.name} {...common} className="field" value={String(v ?? '')} onChange={(x) => onChange(f.name, x)}>
                <Label className="field-label">{f.label}<Required on={f.required} /></Label>
                <TextArea className="field-input req-textarea" rows={4} />
                {f.help && <Text slot="description" className="field-description">{f.help}</Text>}
                <FieldError className="field-error" />
              </AriaTextField>
            );
          case 'enum': {
            const opts = f.options ?? [];
            if (opts.length > 0 && opts.length <= 4) {
              return (
                <div key={f.name} className="req-radio">
                  <RadioCards {...common} label={f.label} value={String(v ?? '') || null} onChange={(x) => onChange(f.name, x)} options={opts.map((o) => ({ id: o.value, label: o.label }))} aria-describedby={f.help ? `${idPrefix}-${f.name}-help` : undefined} />
                  {f.help && <span id={`${idPrefix}-${f.name}-help`} className="field-description">{f.help}</span>}
                </div>
              );
            }
            return <Select key={f.name} {...common} label={f.label} description={f.help} placeholder="Choose…" options={opts.map((o) => ({ id: o.value, label: o.label }))} selectedKey={String(v ?? '') || null} onSelectionChange={(k) => onChange(f.name, k === null ? '' : String(k))} />;
          }
          case 'principal': {
            const opts = (members ?? []).filter((m) => m.status === 'active').map((m) => ({ id: m.id, label: m.display_name }));
            return <Select key={f.name} {...common} label={f.label} description={f.help} placeholder={members === null ? 'Loading people…' : 'Choose a person…'} isDisabled={members === null} options={opts} selectedKey={String(v ?? '') || null} onSelectionChange={(k) => onChange(f.name, k === null ? '' : String(k))} />;
          }
          case 'money':
          case 'number':
            return (
              <NumberField key={f.name} {...common} className="field" value={typeof v === 'number' ? v : NaN} onChange={(x) => onChange(f.name, x)} minValue={f.type === 'money' ? 0 : undefined}
                formatOptions={f.type === 'money' ? (f.currency ? { style: 'currency', currency: f.currency } : { minimumFractionDigits: 0, maximumFractionDigits: 2 }) : { maximumFractionDigits: 6 }}>
                <Label className="field-label">{f.label}<Required on={f.required} /></Label>
                <Input className="field-input req-number" />
                {f.help && <Text slot="description" className="field-description">{f.help}</Text>}
                <FieldError className="field-error" />
              </NumberField>
            );
          case 'file':
            return <FileField key={f.name} field={f} value={v} onChange={(x) => onChange(f.name, x)} error={errors[f.name]} />;
          case 'date':
            return <TextField key={f.name} {...common} type="date" label={f.label} description={f.help} value={String(v ?? '')} onChange={(x) => onChange(f.name, x)} />;
          case 'boolean':
            return (
              <div key={f.name} className="field">
                <Checkbox name={f.name} isSelected={v === true} onChange={(x) => onChange(f.name, x)} isInvalid={!!errors[f.name]} aria-describedby={errors[f.name] ? `${idPrefix}-${f.name}-err` : undefined}>
                  {f.label}<Required on={f.required} />
                </Checkbox>
                {f.help && <span className="field-description">{f.help}</span>}
                {errors[f.name] && <span id={`${idPrefix}-${f.name}-err`} className="field-error">{errors[f.name]}</span>}
              </div>
            );
          default:
            return <TextField key={f.name} {...common} label={f.label} description={f.help} value={String(v ?? '')} onChange={(x) => onChange(f.name, x)} />;
        }
      })}
    </>
  );
}
