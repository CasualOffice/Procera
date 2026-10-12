import { ArrowLeft, Bell, CircleCheck, Send, UserCheck } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { Link, navigate } from '../lib/router';
import { type RequestType, fieldErrors } from '../lib/requests';
import { setCrumb } from '../ui/AppShell';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { RequestFields, type Values, requiredErrors, toInput, valuesFrom } from './requestFields';
import { categoryTone, typeIcon } from './Requests';
import { WhoApproves } from './requestPreview';
import './requests.css';

/**
 * Start a request (BPMS-103, `/requests/new/{type}`): the type's form rendered from its schema, required
 * fields checked before sending, the server's field errors shown on their fields, and one
 * Idempotency-Key per submit so a double click or a retry never files two requests.
 */
export function RequestNew({ typeKey }: { typeKey: string }) {
  const [types, reload, error] = useItems<RequestType>('/api/v1/request-types');
  const type = types?.find((t) => t.key === typeKey) ?? null;
  const fields = useMemo(() => type?.form.fields ?? [], [type]);
  const [values, setValues] = useState<Values>({});
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [members] = useItems<Member>(fields.some((f) => f.type === 'principal') ? '/api/v1/members' : null);
  useEffect(() => { setValues(valuesFrom(fields)); setErrors({}); }, [fields]);
  useEffect(() => { if (type) setCrumb(type.name); return () => setCrumb(null); }, [type]);

  if (error) return <LoadError error={error} onRetry={reload} what="this request form" />;
  if (types === null) return <Skeleton variant="doc" label="Loading the form" />;
  if (!type) {
    return <Empty art="no-match" title="This request type isn’t available" action={<Link className="btn btn-secondary btn-md link-btn" to="/requests">See what you can request</Link>}>It may have been retired, or it isn’t offered to you.</Empty>;
  }

  const submit = async () => {
    setProblem(null);
    const errs = requiredErrors(fields, values);
    setErrors(errs);
    if (Object.keys(errs).length) {
      const first = fields.find((f) => errs[f.name]);
      if (first) (document.querySelector(`[name="${CSS.escape(first.name)}"]`) as HTMLElement | null)?.focus();
      return;
    }
    setBusy(true);
    try {
      const r = await api<{ id: string; display_id: string; status: string }>('/api/v1/requests', { method: 'POST', headers: { 'idempotency-key': crypto.randomUUID() }, body: { type: type.key, input: toInput(fields, values) } });
      navigate(`/requests/${r.id}?submitted=1`);
    } catch (e) {
      const fe = fieldErrors(e);
      if (Object.keys(fe).length) { setErrors(fe); setProblem('Some answers need another look.'); }
      else setProblem(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'The request could not be sent. Check your connection and try again.');
    } finally { setBusy(false); }
  };

  const Icon = typeIcon(type);
  return (
    <div className="req-new">
      <Link className="back-link" to="/requests"><ArrowLeft aria-hidden="true" size="0.875rem" /> All request types</Link>
      <header className="req-new-head">
        <span className={`type-icon type-icon-lg tag-${categoryTone(type.category || 'General')}`}><Icon aria-hidden="true" size="1.375rem" /></span>
        <div>
          <p className="req-eyebrow">{type.category || 'Request'}</p>
          <h1>{type.name}</h1>
          {type.description && <p className="req-lede">{type.description}</p>}
        </div>
      </header>
      <div className="req-new-grid">
        <Form className="req-form stack" validationBehavior="aria" validationErrors={errors} onSubmit={(e) => { e.preventDefault(); void submit(); }} aria-label={type.name}>
          {problem && <Notice tone="danger" title={problem} />}
          {fields.length === 0 && <p className="muted">This request has no questions. Send it when you’re ready.</p>}
          <RequestFields fields={fields} values={values} onChange={(n, v) => { setValues((s) => ({ ...s, [n]: v })); if (errors[n]) setErrors(({ [n]: _, ...rest }) => rest); }} members={members} errors={errors} idPrefix="new" />
          <div className="req-form-foot">
            <Button type="submit" isDisabled={busy}><Send aria-hidden="true" size="0.9375rem" /> {busy ? 'Submitting…' : 'Submit request'}</Button>
            <Link className="btn btn-ghost btn-md link-btn" to="/requests">Cancel</Link>
            {fields.some((f) => f.required) && <span className="req-form-hint"><span aria-hidden="true">*</span> Required</span>}
          </div>
        </Form>
        <aside className="req-next" aria-label="Before you send">
          <WhoApproves typeKey={type.key} fields={fields} values={values} />
          <h2 id="req-next-title">What happens next</h2>
          <ol>
            <li><UserCheck aria-hidden="true" size="1rem" /><span>It goes to the right approver for this request, usually your manager first.</span></li>
            <li><Bell aria-hidden="true" size="1rem" /><span>You get a notification when someone decides, or if they send it back with a question.</span></li>
            <li><CircleCheck aria-hidden="true" size="1rem" /><span>Every step is kept on the request page, so you can always see where it is.</span></li>
          </ol>
        </aside>
      </div>
    </div>
  );
}
