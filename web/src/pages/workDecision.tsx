import { Check, CornerUpLeft, RotateCw, X } from 'lucide-react';
import { useId, useRef, useState } from 'react';
import { FieldError, Form, Label, Text, TextArea, TextField as AriaTextField } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { type RequestDetail, type WorkItem, dueInfo, fieldErrors, needsComment, outcomeTone, outcomeVerb, workChanged } from '../lib/requests';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { Tag } from '../ui/Tag';
import { RequestFields, type Values, requiredErrors, toInput, valuesFrom } from './requestFields';
import { AttachmentList, FileDrop, type PickedFile, fileProblem } from './requestFiles';
import { PolicyLine } from './requestPolicy';

const ICON = { primary: Check, secondary: CornerUpLeft, reject: X } as const;

/**
 * The decision panel (BPMS-105): one button per outcome the step allows, a comment (required to send
 * back or reject), the fields this step may edit, and the work item's revision as If-Match so a
 * decision made elsewhere first is refused (409/412) instead of overwritten. Used on My work and on
 * the request page. Approve is the primary action; Reject stays an outlined neutral (docs/05 §7.1.1).
 */
export function DecisionPanel({ item, detail, onDecided }: { item: WorkItem; detail?: RequestDetail | null; onDecided: (outcome: string) => void }) {
  const editable = item.fields ?? [];
  const [values, setValues] = useState<Values>(() => valuesFrom(editable, detail?.input));
  const [comment, setComment] = useState('');
  const [files, setFiles] = useState<PickedFile[]>([]);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [stale, setStale] = useState(false);
  const [members] = useItems<Member>(editable.some((f) => f.type === 'principal') ? '/api/v1/members' : null);
  const commentRef = useRef<HTMLTextAreaElement>(null);
  const headingId = useId();
  const due = dueInfo(item.due_at);
  const commentWhy = item.outcomes.filter(needsComment).map((o) => outcomeVerb(o).toLowerCase());

  const decide = async (outcome: string) => {
    setProblem(null);
    const errs: Record<string, string> = outcome.startsWith('reject') || outcome.startsWith('cancel') ? {} : requiredErrors(editable, values);
    if (needsComment(outcome) && !comment.trim()) errs.comment = `Say why you ${outcome.startsWith('reject') ? 'are rejecting it' : 'are sending it back'}, so ${item.request.initiator.name} knows what to do.`;
    const fp = fileProblem(files);
    if (fp) errs.files = fp;
    setErrors(errs);
    if (Object.keys(errs).length) { if (errs.comment) commentRef.current?.focus(); return; }
    setBusy(outcome);
    try {
      await api(`/api/v1/work/${item.id}/decisions`, {
        method: 'POST',
        headers: { 'if-match': `"${item.revision}"`, 'idempotency-key': crypto.randomUUID() },
        body: { outcome, comment: comment.trim() || undefined, fields: editable.length ? toInput(editable, values) : undefined, files: files.flatMap((f) => (f.id ? [f.id] : [])), policy_version: item.policy?.version_id },
      });
      workChanged();
      onDecided(outcome);
    } catch (e) {
      if (e instanceof ApiError && (e.status === 409 || e.status === 412)) setStale(true);
      else if (e instanceof ApiError && e.status === 422 && Object.keys(fieldErrors(e)).length) setErrors(fieldErrors(e));
      else setProblem(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'The decision could not be saved. Check your connection and try again.');
    } finally { setBusy(null); }
  };

  return (
    <section className="decide" aria-labelledby={headingId}>
      <div className="decide-head">
        <h2 id={headingId}>Your decision</h2>
        <span className="decide-step">{item.step}</span>
        {due && <Tag color={due.color} dot={due.overdue}>{due.label}</Tag>}
      </div>
      {item.why_you && <p className="decide-why"><span className="decide-why-label">Why you:</span> {item.why_you}.</p>}
      {item.policy && <PolicyLine policy={item.policy} />}
      {!detail && <AttachmentList items={item.attachments ?? []} heading="Files on this request" headingLevel={3} />}
      {stale ? (
        <Notice tone="warning" title="Someone already decided this — refresh">
          <p className="decide-note">The request moved on while you had it open. Refresh to see where it is now.</p>
          <Button variant="secondary" className="btn-sm" onPress={() => window.location.reload()}><RotateCw aria-hidden="true" size="0.875rem" /> Refresh</Button>
        </Notice>
      ) : (
        <Form className="stack decide-form" validationBehavior="aria" validationErrors={errors} onSubmit={(e) => e.preventDefault()}>
          {problem && <Notice tone="danger" title={problem} />}
          {editable.length > 0 && (
            <fieldset className="decide-fields">
              <legend>You can change these before deciding</legend>
              <RequestFields fields={editable} values={values} onChange={(n, v) => setValues((s) => ({ ...s, [n]: v }))} members={members} errors={errors} idPrefix={`w${item.id}`} />
            </fieldset>
          )}
          <AriaTextField name="comment" className="field" value={comment} onChange={(v) => { setComment(v); if (errors.comment) setErrors(({ comment: _, ...rest }) => rest); }}>
            <Label className="field-label">Comment</Label>
            <TextArea ref={commentRef} className="field-input req-textarea" rows={3} />
            {commentWhy.length > 0 && <Text slot="description" className="field-description">Optional to {outcomeVerb(item.outcomes.find((o) => !needsComment(o)) ?? 'approve').toLowerCase()}; required to {commentWhy.join(' or ')}.</Text>}
            <FieldError className="field-error" />
          </AriaTextField>
          {!editable.some((f) => f.type === 'file') && <FileDrop label="Attach files to your decision" description="Optional, for example a signed quote. Everyone who can see the request can open them." value={files}
            onChange={(x) => { setFiles(x); if (errors.files) setErrors(({ files: _, ...rest }) => rest); }} error={errors.files} />}
          <div className="decide-actions">
            {[...item.outcomes].sort((a, b) => order(a) - order(b)).map((o) => {
              const tone = outcomeTone(o);
              const Icon = ICON[tone];
              return (
                <Button key={o} variant={tone === 'primary' ? 'primary' : 'secondary'} className={tone === 'reject' ? 'btn-reject' : undefined} isDisabled={busy !== null} onPress={() => void decide(o)}>
                  <Icon aria-hidden="true" size="0.9375rem" /> {busy === o ? `${outcomeVerb(o)}…` : outcomeVerb(o)}
                </Button>
              );
            })}
          </div>
        </Form>
      )}
    </section>
  );
}

/** Primary first, then neutral, then reject (reading order matches importance). */
const order = (o: string) => ({ primary: 0, secondary: 1, reject: 2 })[outcomeTone(o)];
