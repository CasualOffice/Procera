import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type Member, useResource } from '../lib/data';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { Skeleton } from '../ui/States';

/** `GET /members/{id}/holdings`: what someone holds when they leave (docs/08 "Directory"). */
interface Holdings {
  reassign_strategy: string;
  documents: { id: string; title: string; kind: string }[];
  work_in_progress: { artifact_id: string; title: string; version_no: number; lifecycle: string }[];
  campaigns: { id: string; title: string; state: string }[];
  groups: { id: string; name: string }[];
  direct_reports: { id: string; display_name: string }[];
  grants: { id: string; role: string; scope_kind: string }[];
  pending_acknowledgements: number;
}

export interface LeaveResult { title: string; details: string[]; tone: 'success' | 'danger' }

const STEPS = ['Review', 'Hand over', 'Confirm'] as const;
const count = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** "A, B, C and 2 more", or "None". */
function names(items: string[]) {
  if (!items.length) return <span className="muted">None</span>;
  const shown = items.slice(0, 4).join(', ');
  return <>{shown}{items.length > 4 ? ` and ${items.length - 4} more` : ''}</>;
}

/**
 * Leaving the organisation (docs/design/ia-competitive-analysis.md §8.3, docs/22 A-44): review what
 * they hold, hand their documents and direct reports to someone else, then deactivate. Every
 * hand-over goes through its own audited endpoint; their open acknowledgements close as "left".
 */
export function LeaveDialog({ member, members, managerId, onClose, onDone }: {
  member: Member; members: Member[]; managerId: string | null; onClose: () => void; onDone: (r: LeaveResult) => void;
}) {
  const { membershipId } = useSession();
  const [holdings, , loadError] = useResource<Holdings>(`/api/v1/members/${member.id}/holdings`);
  const [step, setStep] = useState(0);
  const [owner, setOwner] = useState<string | null>(null);
  const [manager, setManager] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const people = members.filter((m) => m.status === 'active' && m.id !== member.id);
  const options = people.map((m) => ({ id: m.id, label: m.id === membershipId ? `${m.display_name} (you)` : m.display_name }));
  const nameOf = (id: string | null) => { const m = people.find((x) => x.id === id); return m ? (m.id === membershipId ? `${m.display_name} (you)` : m.display_name) : 'nobody'; };
  const theirManager = managerId && managerId !== member.id && people.some((m) => m.id === managerId) ? managerId : null;

  // Defaults follow "When someone leaves" (directory.deactivation.reassign_strategy).
  useEffect(() => {
    if (!holdings) return;
    const fallback = membershipId ?? null;
    setOwner((o) => o ?? (holdings.reassign_strategy === 'manager' && theirManager ? theirManager : fallback));
    setManager((m) => m ?? theirManager ?? fallback);
  }, [holdings, theirManager, membershipId]);

  const first = member.display_name.split(' ')[0];
  const docs = holdings?.documents.length ?? 0;
  const reports = holdings?.direct_reports.length ?? 0;
  const acks = holdings?.pending_acknowledgements ?? 0;

  const confirm = async () => {
    if (!holdings) return;
    setBusy(true);
    setError(null);
    try {
      await api(`/api/v1/members/${member.id}/deactivate`, { method: 'POST' });
    } catch (e) {
      setError(e instanceof ApiError ? (e.code === 'last_owner' ? 'This is the only owner. Make someone else an owner first.' : e.problem.detail ?? e.problem.title) : 'That didn’t work. Try again.');
      setBusy(false);
      return;
    }
    const failed: string[] = [];
    if (owner) for (const d of holdings.documents) {
      try { await api(`/api/v1/artifacts/${d.id}/owner`, { method: 'PUT', body: { membership_id: owner } }); } catch { failed.push(d.title); }
    }
    if (manager) for (const r of holdings.direct_reports) {
      try { await api(`/api/v1/members/${r.id}`, { method: 'PATCH', body: { manager_membership_id: manager } }); } catch { failed.push(r.display_name); }
    }
    const details = [
      docs && owner ? `${count(docs - holdings.documents.filter((d) => failed.includes(d.title)).length, 'document')} now owned by ${nameOf(owner)}.` : '',
      reports && manager ? (() => { const n = reports - holdings.direct_reports.filter((r) => failed.includes(r.display_name)).length; return `${count(n, 'person', 'people')} now ${n === 1 ? 'reports' : 'report'} to ${nameOf(manager)}.`; })() : '',
      acks ? `${count(acks, 'pending acknowledgement')} closed as “left organisation”.` : '',
      failed.length ? `Couldn’t hand over: ${failed.join(', ')}. Change these by hand.` : '',
    ].filter(Boolean);
    setBusy(false);
    onDone({ title: `${member.display_name} can no longer sign in to this workspace.`, details, tone: failed.length ? 'danger' : 'success' });
  };

  const footer = (
    <>
      {step === 0 ? <Button variant="secondary" onPress={onClose}>Keep</Button> : <Button variant="secondary" onPress={() => setStep(step - 1)} isDisabled={busy}>Back</Button>}
      {step < 2
        ? <Button onPress={() => setStep(step + 1)} isDisabled={!holdings}>Next</Button>
        : <Button variant="danger" onPress={() => void confirm()} isDisabled={busy}>{busy ? 'Deactivating…' : 'Deactivate'}</Button>}
    </>
  );

  return (
    <FormDialog isOpen onOpenChange={(o) => !o && !busy && onClose()} title={`Deactivate ${member.display_name}?`} width={560}
      description={['Step 1 of 3: what they hold in this workspace.', 'Step 2 of 3: who takes over their work.', 'Step 3 of 3: check and confirm.'][step]}
      footer={footer}>
      <ol className="lifecycle-steps" aria-label="Steps" style={{ marginBottom: 'var(--space-4)' }}>
        {STEPS.map((s, i) => <li key={s} aria-current={i === step ? 'step' : undefined} className={i < step ? 'done' : i === step ? 'current' : ''}><span className="lifecycle-mark" aria-hidden="true" />{s}</li>)}
      </ol>
      {error && <Notice tone="danger" title={error} />}
      {loadError ? <Notice tone="danger" title="Couldn’t load what they hold.">{loadError.message}</Notice> : !holdings ? <Skeleton rows={4} label="Loading what they hold" /> : step === 0 ? (
        <dl className="props" aria-label="What they hold">
          <dt>Documents they own</dt><dd>{names(holdings.documents.map((d) => d.title))}</dd>
          <dt>Drafts and reviews</dt><dd>{names(holdings.work_in_progress.map((w) => `${w.title} v${w.version_no}`))}</dd>
          <dt>Campaigns they run</dt><dd>{names(holdings.campaigns.map((c) => c.title))}</dd>
          <dt>Groups</dt><dd>{names(holdings.groups.map((g) => g.name))}</dd>
          <dt>Direct reports</dt><dd>{names(holdings.direct_reports.map((r) => r.display_name))}</dd>
          <dt>Role grants</dt><dd>{names(holdings.grants.map((g) => g.scope_kind === 'workspace' ? g.role : `${g.role} (${g.scope_kind})`))}</dd>
          <dt>Pending acknowledgements</dt><dd>{acks ? `${acks}: they close as “left organisation”, never as acknowledged` : <span className="muted">None</span>}</dd>
        </dl>
      ) : step === 1 ? (
        <div className="stack">
          {docs > 0 && <Select label="New owner for their documents" options={options} selectedKey={owner} onSelectionChange={(k) => setOwner(String(k))}
            description={`${count(docs, 'document')}. The owner keeps them correct and current.`} />}
          {reports > 0 && <Select label="New manager for their direct reports" options={options} selectedKey={manager} onSelectionChange={(k) => setManager(String(k))}
            description={`${count(reports, 'person', 'people')} report to ${first}.`} />}
          {!docs && !reports && <p style={{ margin: 0 }}>{first} owns no documents and has no direct reports. Nothing to hand over.</p>}
          <p className="muted" style={{ margin: 0 }}>Drafts, campaigns, groups and role grants stay as they are. Their roles come back if you reactivate them.</p>
        </div>
      ) : (
        <ul className="stack" style={{ margin: 0, paddingLeft: 'var(--space-5)' }} aria-label="What happens">
          <li>{first} is signed out of this workspace at once and can’t act in it.</li>
          {docs > 0 && <li>{count(docs, 'document')} {docs === 1 ? 'goes' : 'go'} to {nameOf(owner)}.</li>}
          {reports > 0 && <li>{count(reports, 'person', 'people')} will report to {nameOf(manager)}.</li>}
          {acks > 0 && <li>{count(acks, 'pending acknowledgement')} {acks === 1 ? 'closes' : 'close'} as “left organisation”.</li>}
          <li>Their history and records stay. You can reactivate them later. Everything is recorded in the audit log.</li>
        </ul>
      )}
    </FormDialog>
  );
}
