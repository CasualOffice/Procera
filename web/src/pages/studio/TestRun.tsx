import { FlaskConical, Play } from 'lucide-react';
import { useMemo, useState } from 'react';
import { ApiError, api } from '../../lib/api';
import { outcomePast } from '../../lib/requests';
import { Button } from '../../ui/components/Button';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { Select } from '../../ui/components/Select';
import { Tag } from '../../ui/Tag';
import type { Content, Field, Step } from './model';

/*
 * Test run (BPMS-100, docs/05 §3.5): the draft as it is now, run for sample answers and the outcome
 * picked at each step, worked out on the server in memory. Nothing is stored and nobody is told.
 */

interface SimStep { node: string; name: string; kind: string; who: string; outcome: string; outcomes: string[] }
interface Simulation { steps: SimStep[]; result: string; ended_at: string | null; note: string | null }

type Plan = 'natural' | 'reject' | 'back' | 'expire';
const PLANS: Record<string, { id: Plan; label: string }[]> = {
  approval: [{ id: 'natural', label: 'Approve as usual' }, { id: 'reject', label: 'Reject' }, { id: 'back', label: 'Send back, then approve' }],
  task: [{ id: 'natural', label: 'Mark done' }],
  acknowledge: [{ id: 'natural', label: 'Acknowledge' }, { id: 'expire', label: 'Let it expire' }],
};
const CHOICE: Record<Plan, string[] | null> = { natural: null, reject: ['rejected'], back: ['changes_requested', 'approved'], expire: ['expired'] };

const inTwoWeeks = () => new Date(Date.now() + 14 * 86_400_000).toISOString().slice(0, 10);

/** A sensible sample answer for each field; people and files are left out. */
function sample(f: Field): unknown {
  switch (f.kind) {
    case 'string': return 'Example';
    case 'text': return 'An example answer.';
    case 'number': return 1;
    case 'money': return { amount: '1000', currency: f.currency || 'EUR' };
    case 'date': return inTwoWeeks();
    case 'boolean': return true;
    case 'enum': return f.options[0] ?? '';
    default: return undefined;
  }
}

export function TestRun({ content, fields, steps, onClose }: { content: Content; fields: Field[]; steps: Step[]; onClose: () => void }) {
  const answerable = fields.filter((f) => sample(f) !== undefined);
  const [answers, setAnswers] = useState<Record<string, unknown>>(() => Object.fromEntries(answerable.map((f) => [f.name, sample(f)])));
  const [plans, setPlans] = useState<Record<string, Plan>>({});
  const [result, setResult] = useState<Simulation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const choices = useMemo(() => Object.fromEntries(Object.entries(plans).flatMap(([id, p]) => (CHOICE[p] ? [[id, CHOICE[p]]] : []))), [plans]);

  const run = async () => {
    setBusy(true); setError(null);
    try {
      setResult(await api<Simulation>('/api/v1/processes/test-run', { method: 'POST', body: { content, input: answers, choices } }));
    } catch (e) {
      setResult(null);
      setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Couldn’t run the test. Try again.');
    } finally { setBusy(false); }
  };

  const set = (name: string, v: unknown) => { setAnswers((a) => ({ ...a, [name]: v })); setResult(null); };
  const ends = result && (result.result === 'approved' ? 'Ends Approved' : result.result === 'rejected' ? 'Ends Rejected' : result.result === 'cancelled' ? 'Ends Cancelled' : 'Doesn’t finish');

  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Test run" width={720}
      description="Try the draft with sample answers and pick what happens at each step. Nothing is saved and nobody is told."
      footer={<><Button variant="secondary" onPress={onClose}>Close</Button><Button onPress={() => void run()} isDisabled={busy}><Play aria-hidden="true" size="0.875rem" /> {busy ? 'Running…' : 'Run test'}</Button></>}>
      <p className="testrun-ribbon"><FlaskConical aria-hidden="true" size="0.875rem" /> TEST — nothing is saved and nobody is told</p>
      <div className="testrun-grid">
        <section aria-labelledby="tr-answers">
          <h3 id="tr-answers" className="testrun-h">Answers</h3>
          {answerable.length === 0 ? <p className="muted">This form has no questions to answer.</p> : (
            <div className="stack">
              {answerable.map((f) => {
                const id = `tr-${f.name}`;
                if (f.kind === 'enum') {
                  return <Select key={f.name} label={f.label} options={f.options.map((o) => ({ id: o, label: o }))} selectedKey={String(answers[f.name] ?? '') || null} onSelectionChange={(k) => set(f.name, String(k ?? ''))} />;
                }
                if (f.kind === 'boolean') {
                  return <label key={f.name} className="check-row"><input type="checkbox" checked={answers[f.name] === true} onChange={(e) => set(f.name, e.target.checked)} /> {f.label}</label>;
                }
                const value = f.kind === 'money' ? String((answers[f.name] as { amount: string } | undefined)?.amount ?? '') : String(answers[f.name] ?? '');
                return (
                  <div key={f.name} className="field">
                    <label className="field-label" htmlFor={id}>{f.label}{f.kind === 'money' ? ` (${f.currency})` : ''}</label>
                    <input id={id} type={f.kind === 'date' ? 'date' : f.kind === 'number' || f.kind === 'money' ? 'number' : 'text'} value={value}
                      onChange={(e) => set(f.name, f.kind === 'money' ? { amount: e.target.value, currency: f.currency || 'EUR' } : f.kind === 'number' ? Number(e.target.value) : e.target.value)} />
                  </div>
                );
              })}
            </div>
          )}
        </section>
        <section aria-labelledby="tr-plan">
          <h3 id="tr-plan" className="testrun-h">At each step</h3>
          {steps.length === 0 ? <p className="muted">There are no steps yet.</p> : (
            <div className="stack">
              {steps.map((s) => (
                <Select key={s.id} label={s.name || 'Untitled step'} options={(PLANS[s.kind] ?? PLANS.task ?? []).map((p) => ({ id: p.id, label: p.label }))}
                  selectedKey={plans[s.id] ?? 'natural'} onSelectionChange={(k) => { setPlans((p) => ({ ...p, [s.id]: (k ?? 'natural') as Plan })); setResult(null); }} />
              ))}
            </div>
          )}
        </section>
      </div>
      {error && <Notice tone="danger" title={error} />}
      {result && (
        <section aria-labelledby="tr-result" className="testrun-result">
          <h3 id="tr-result" className="testrun-h">What happens</h3>
          <ol className="testrun-trace" aria-label="Test run trace">
            <li><span className="testrun-dot" aria-hidden="true" /><span><strong>Request submitted</strong></span></li>
            {result.steps.map((s, i) => (
              <li key={`${s.node}-${i}`}>
                <span className="testrun-dot" aria-hidden="true" />
                <span className="testrun-step"><strong>{s.name}</strong><span className="muted">goes to {s.who}</span></span>
                <Tag color={s.outcome === 'rejected' ? 'red' : s.outcome === 'changes_requested' ? 'orange' : 'green'}>{outcomePast(s.outcome)}</Tag>
              </li>
            ))}
          </ol>
          <p className={`testrun-end is-${result.result}`} role="status">{ends}{result.ended_at && result.ended_at.toLowerCase() !== result.result ? ` (${result.ended_at})` : ''}.{result.note ? ` ${result.note}` : ''}</p>
        </section>
      )}
    </FormDialog>
  );
}
