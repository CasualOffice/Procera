import { Monitor, Smartphone } from 'lucide-react';
import { useMemo, useState } from 'react';
import type { FormField } from '../../lib/requests';
import { FormDialog } from '../../ui/components/FormDialog';
import { Button } from '../../ui/components/Button';
import { RequestFields, type Values, valuesFrom } from '../requestFields';
import type { Field, StudioRequest } from './model';

/*
 * Form preview (form builder, BPMS-101; docs/05 §3.6): the request form exactly as people will see
 * it under New request, built from the draft — labels, help, required marks, rules and "show only
 * when" working live — at desktop or phone width. Nothing is submitted.
 */

/** The studio's field as the request form renders it. */
export function toFormField(f: Field): FormField | null {
  const type = ({ string: 'string', text: 'text', number: 'number', money: 'money', date: 'date', boolean: 'boolean', enum: 'enum', principal: 'principal', files: 'file' } as const)[f.kind as 'string'];
  if (!type) return null;
  return {
    name: f.name, label: f.label || f.name, type, required: f.required, help: f.help || undefined,
    options: f.kind === 'enum' ? f.options.filter((o) => o.trim()).map((o) => ({ value: o, label: o })) : undefined,
    currency: f.kind === 'money' ? f.currency : undefined, multiple: f.kind === 'files' ? true : undefined,
    rules: f.rules, default: f.default, classification: f.classification,
  };
}

export function FormPreview({ request, fields, onClose }: { request: StudioRequest; fields: Field[]; onClose: () => void }) {
  const form = useMemo(() => fields.map(toFormField).filter((f): f is FormField => f !== null), [fields]);
  const [values, setValues] = useState<Values>(() => valuesFrom(form));
  const [phone, setPhone] = useState(false);
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Preview the form" width={phone ? 460 : 720}
      description="What people see under New request. Try the answers: fields shown only when another answer matches appear as you go. Nothing is sent."
      footer={<Button variant="secondary" onPress={onClose}>Close</Button>}>
      <div className="preview-toggle" role="group" aria-label="Preview width">
        <button type="button" aria-pressed={!phone} onClick={() => setPhone(false)}><Monitor aria-hidden="true" size="0.875rem" /> Desktop</button>
        <button type="button" aria-pressed={phone} onClick={() => setPhone(true)}><Smartphone aria-hidden="true" size="0.875rem" /> Phone</button>
      </div>
      <div className={phone ? 'form-preview is-phone' : 'form-preview'} aria-label={`${request.name || 'Request'} form preview`} role="group">
        <p className="form-preview-title">{request.name || 'Untitled request'}</p>
        {request.description && <p className="form-preview-desc">{request.description}</p>}
        <div className="stack">
          {form.length ? <RequestFields fields={form} values={values} onChange={(n, v) => setValues((x) => ({ ...x, [n]: v }))} members={[]} idPrefix="pv" /> : <p className="muted">No questions yet.</p>}
        </div>
      </div>
    </FormDialog>
  );
}
