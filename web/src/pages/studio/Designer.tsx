import { Check, CircleAlert, CloudOff, LoaderCircle, Monitor, Redo2, RotateCw, ShieldCheck, TriangleAlert, Undo2, X } from 'lucide-react';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ApiError, api } from '../../lib/api';
import { type LoadFailure, type Member, toFailure, useItems } from '../../lib/data';
import { type Group, problemText } from '../../lib/directory';
import type { Role } from '../../lib/roles';
import { setCrumb } from '../../ui/AppShell';
import { Button } from '../../ui/components/Button';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { LoadError, Skeleton } from '../../ui/States';
import { Tag } from '../../ui/Tag';
import { stateTag } from '../Processes';
import { type ProcessDetail, type Problem, problemsOf, publish, saveDraft, validate } from './api';
import {
  type FieldKind, type IrDocument, type Model, type Step, addField, addStep, fromContent, moveField, moveStep, problemTarget, removeField, removeStep,
  setRequest, toContent, updateField, updateStep,
} from './model';
import { RequestPanel } from './RequestPanel';
import { type StepAction, StepBuilder } from './StepBuilder';
import { StepProperties } from './StepProperties';
import './studio.css';

type SaveState = 'saved' | 'dirty' | 'saving' | 'error' | 'conflict';
const WIDE = '(min-width: 64rem)';

function useWide(): boolean {
  const [wide, setWide] = useState(() => typeof window === 'undefined' || window.matchMedia(WIDE).matches);
  useEffect(() => {
    const mq = window.matchMedia(WIDE);
    const on = () => setWide(mq.matches);
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  }, []);
  return wide;
}

const editingTarget = (t: EventTarget | null) => t instanceof HTMLElement && (t.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(t.tagName));

/**
 * The Guided Studio (BPMS-100, docs/05 §3.5) at `/admin/processes/{id}`: the request card and form
 * fields on the left, the steps in the centre, the selected step's settings on the right. Edits
 * autosave to the draft (If-Match on its revision; a stale one shows a reload banner), Validate
 * lists problems linked to their steps, and Publish makes it a request type, or sends it for review.
 */
export function Designer({ id }: { id: string }) {
  const wide = useWide();
  const [detail, setDetail] = useState<ProcessDetail | null>(null);
  const [loadError, setLoadError] = useState<LoadFailure | null>(null);
  const [model, setModel] = useState<Model | null>(null);
  const modelRef = useRef<Model | null>(null);
  const history = useRef<{ past: Model[]; future: Model[] }>({ past: [], future: [] });
  const meta = useRef<{ key: string; version: number; revision: number | null }>({ key: '', version: 1, revision: null });
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const inflight = useRef<Promise<boolean> | null>(null);
  const dirty = useRef(false);
  const [save, setSave] = useState<SaveState>('saved');
  const [problems, setProblems] = useState<{ list: Problem[]; checked: IrDocument } | null>(null);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger' | 'info'; title: string } | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [openField, setOpenField] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<Step | null>(null);
  const [busy, setBusy] = useState<'validate' | 'publish' | null>(null);
  const [said, setSaid] = useState('');
  const [groups] = useItems<Group>('/api/v1/groups');
  const [roles] = useItems<Role>('/api/v1/roles');
  const [members] = useItems<Member>('/api/v1/members');

  const load = useCallback(async () => {
    try {
      const d = await api<ProcessDetail>(`/api/v1/processes/${id}`);
      const content = d.draft?.content ?? d.published?.content ?? null;
      const m = fromContent(content, d.title);
      meta.current = { key: d.key, version: d.draft?.version_no ?? (d.published ? d.published.version_no + 1 : 1), revision: d.draft?.revision ?? null };
      history.current = { past: [], future: [] };
      dirty.current = false;
      modelRef.current = m;
      setModel(m);
      setDetail(d);
      setSave('saved');
      setLoadError(null);
      setSelected((s) => (s && m.steps.some((x) => x.id === s) ? s : null));
    } catch (e) { setLoadError(toFailure(e)); }
  }, [id]);
  useEffect(() => { void load(); }, [load]);
  useEffect(() => { if (detail) setCrumb(detail.title); return () => setCrumb(null); }, [detail]);

  const editable = !!detail && !!model && detail.can.edit && !model.readOnly && wide && save !== 'conflict' && (!detail.draft || detail.draft.lifecycle === 'draft');

  /** Saves the draft now (after any save in flight). False when it couldn't (conflict or error). */
  const flush = useCallback(async (): Promise<boolean> => {
    clearTimeout(timer.current);
    if (inflight.current) await inflight.current;
    if (!dirty.current || !modelRef.current) return true;
    dirty.current = false;
    setSave('saving');
    const run = (async () => {
      try {
        const wasNew = meta.current.revision === null;
        const r = await saveDraft(id, toContent(modelRef.current!, meta.current.key, meta.current.version), meta.current.revision);
        meta.current.revision = r.revision;
        if (wasNew) {
          // The save opened a new draft: learn its number (and save again if it isn't the one written).
          const d = await api<ProcessDetail>(`/api/v1/processes/${id}`);
          setDetail(d);
          if (d.draft) {
            meta.current.revision = d.draft.revision;
            if (d.draft.version_no !== meta.current.version) { meta.current.version = d.draft.version_no; dirty.current = true; }
          }
        }
        setSave(dirty.current ? 'dirty' : 'saved');
        return true;
      } catch (e) {
        if (e instanceof ApiError && e.status === 412) { setSave('conflict'); return false; }
        dirty.current = true;
        setSave('error');
        return false;
      } finally { inflight.current = null; }
    })();
    inflight.current = run;
    const ok = await run;
    return ok && dirty.current ? flush() : ok;
  }, [id]);

  const schedule = () => {
    dirty.current = true;
    setSave('dirty');
    clearTimeout(timer.current);
    timer.current = setTimeout(() => void flush(), 900);
  };

  const change = (fn: (m: Model) => Model) => {
    const prev = modelRef.current;
    if (!prev || !editable) return;
    const next = fn(prev);
    if (next === prev) return;
    history.current.past.push(prev);
    if (history.current.past.length > 100) history.current.past.shift();
    history.current.future = [];
    modelRef.current = next;
    setModel(next);
    schedule();
  };

  const travel = (dir: 'undo' | 'redo') => {
    const h = history.current;
    const from = dir === 'undo' ? h.past : h.future;
    const to = dir === 'undo' ? h.future : h.past;
    const cur = modelRef.current;
    if (!from.length || !cur || !editable) return;
    to.push(cur);
    const next = from.pop()!;
    modelRef.current = next;
    setModel(next);
    setSaid(dir === 'undo' ? 'Undone.' : 'Redone.');
    schedule();
  };
  const travelRef = useRef(travel);
  travelRef.current = travel;
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.altKey || editingTarget(e.target)) return;
      const k = e.key.toLowerCase();
      if (k === 'z') { e.preventDefault(); travelRef.current(e.shiftKey ? 'redo' : 'undo'); }
      else if (k === 'y' && e.ctrlKey) { e.preventDefault(); travelRef.current('redo'); }
    };
    window.addEventListener('keydown', on);
    return () => window.removeEventListener('keydown', on);
  }, []);
  // Leaving with unsaved edits: save now, and ask the browser to hold the tab if a save is pending.
  useEffect(() => {
    const before = (e: BeforeUnloadEvent) => { if (dirty.current || inflight.current) { void flush(); e.preventDefault(); } };
    window.addEventListener('beforeunload', before);
    return () => { window.removeEventListener('beforeunload', before); if (dirty.current) void flush(); };
  }, [flush]);

  const onStepAction = (a: StepAction) => {
    const m = modelRef.current;
    if (!m) return;
    if (a.type === 'add') {
      const [next, newId] = addStep(m, a.after, a.kind);
      change(() => next);
      setSelected(newId);
      setSaid(`Added a step at position ${a.after + 2}.`);
      requestAnimationFrame(() => (document.querySelector('.studio-right input') as HTMLInputElement | null)?.focus());
    } else if (a.type === 'move') {
      const i = m.steps.findIndex((s) => s.id === a.id);
      const j = i + a.by;
      if (j < 0 || j >= m.steps.length) return;
      change((x) => moveStep(x, a.id, a.by));
      setSaid(`Moved ${m.steps[i]!.name} to position ${j + 1} of ${m.steps.length}.`);
      requestAnimationFrame(() => document.getElementById(`step-${a.id}`)?.focus());
    } else {
      setDeleting(m.steps.find((s) => s.id === a.id) ?? null);
    }
  };

  const onStepChange = (patch: Partial<Omit<Step, 'id' | 'base'>>) => {
    const m = modelRef.current;
    if (!m || !selected) return;
    const [next, newId] = updateStep(m, selected, patch);
    change(() => next);
    if (newId !== selected) setSelected(newId);
  };

  const target = useMemo(() => {
    const steps = new Map<string, Problem[]>();
    const fields = new Map<string, Problem[]>();
    const general: Problem[] = [];
    if (problems && model) {
      for (const p of problems.list) {
        const t = problemTarget(model, p, problems.checked);
        if (t.step) steps.set(t.step, [...(steps.get(t.step) ?? []), p]);
        else if (t.field) fields.set(t.field, [...(fields.get(t.field) ?? []), p]);
        else general.push(p);
      }
    }
    return { steps, fields, general };
  }, [problems, model]);

  const runValidate = async () => {
    const m = modelRef.current;
    if (!m) return;
    setBusy('validate');
    const content = toContent(m, meta.current.key, meta.current.version);
    try {
      const r = await validate(content);
      setProblems({ list: r.problems, checked: content.ir });
      const errors = r.problems.filter((p) => p.severity === 'error').length;
      setNotice(errors ? null : { tone: 'success', title: r.problems.length ? 'No blocking problems. Have a look at the warnings below.' : 'No problems found. It’s ready to publish.' });
    } catch (e) { setNotice({ tone: 'danger', title: problemText(e, 'Couldn’t check the process. Try again.') }); }
    setBusy(null);
  };

  const runPublish = async () => {
    const m = modelRef.current;
    if (!m) return;
    setBusy('publish');
    if (!(await flush())) { setBusy(null); return; }
    try {
      const r = await publish(id);
      setProblems(null);
      await load();
      setNotice(r.state === 'published'
        ? { tone: 'success', title: `Published v${r.version_no}. People can now request it under New request.` }
        : { tone: 'success', title: `Sent for review. v${r.version_no} goes live once it’s approved.` });
    } catch (e) {
      const ps = problemsOf(e);
      if (ps) {
        setProblems({ list: ps, checked: toContent(m, meta.current.key, meta.current.version).ir });
        setNotice({ tone: 'danger', title: 'Not published. Fix the problems below first.' });
      } else setNotice({ tone: 'danger', title: problemText(e, 'Couldn’t publish. Try again.') });
    }
    setBusy(null);
  };

  const goTo = (p: Problem) => {
    if (!model) return;
    const t = problemTarget(model, p, problems?.checked);
    if (t.step) { setSelected(t.step); requestAnimationFrame(() => document.getElementById(`step-${t.step}`)?.focus()); }
    else if (t.field) { setOpenField(t.field); requestAnimationFrame(() => (document.querySelector(`#field-${CSS.escape(t.field!)} input`) as HTMLInputElement | null)?.focus()); }
  };

  if (loadError) return <LoadError error={loadError} onRetry={() => void load()} what="this process" />;
  if (!detail || !model) return <Skeleton variant="doc" label="Loading process" />;

  const roleName = (k: string) => (roles ?? []).find((r) => r.key === k)?.name;
  const step = model.steps.find((s) => s.id === selected) ?? null;
  const tag = stateTag(detail);
  const inReview = !!detail.draft && detail.draft.lifecycle === 'in_review';
  const approved = !!detail.draft && detail.draft.lifecycle === 'approved';
  const canPublish = detail.can.publish && !inReview && !model.readOnly && (!detail.draft || ['draft', 'approved'].includes(detail.draft.lifecycle)) && (!!detail.draft || save !== 'saved');
  const errorCount = problems?.list.filter((p) => p.severity === 'error').length ?? 0;
  const readOnly = !editable;

  return (
    <div className={`studio${wide ? '' : ' studio-narrow'}`}>
      <header className="studio-bar">
        <div className="studio-bar-main">
          <h1 className="studio-title">{detail.title}</h1>
          <p className="studio-bar-meta">
            <Tag color={tag.color} dot>{tag.label}</Tag>
            <span>{detail.draft ? `Draft v${detail.draft.version_no}` : detail.published ? `Live v${detail.published.version_no}` : 'New'}</span>
            <code className="studio-key">{detail.key}</code>
            {editable && <SaveStatus state={save} onRetry={() => void flush()} />}
          </p>
        </div>
        {wide && (
          <div className="studio-bar-actions">
            {editable && (
              <span className="studio-history">
                <button type="button" className="icon-btn" aria-label="Undo" aria-keyshortcuts="Meta+Z" disabled={!history.current.past.length} onClick={() => travel('undo')}><Undo2 aria-hidden="true" size="1rem" /></button>
                <button type="button" className="icon-btn" aria-label="Redo" aria-keyshortcuts="Meta+Shift+Z" disabled={!history.current.future.length} onClick={() => travel('redo')}><Redo2 aria-hidden="true" size="1rem" /></button>
              </span>
            )}
            {!model.readOnly && <Button variant="secondary" onPress={() => void runValidate()} isDisabled={busy !== null}><ShieldCheck aria-hidden="true" size="0.9375rem" /> {busy === 'validate' ? 'Checking…' : 'Validate'}</Button>}
            {detail.can.publish && <Button onPress={() => void runPublish()} isDisabled={!canPublish || busy !== null || save === 'conflict'}>{busy === 'publish' ? 'Publishing…' : 'Publish'}</Button>}
          </div>
        )}
      </header>
      <p className="visually-hidden" role="status" aria-live="polite">{said}</p>

      {save === 'conflict' && (
        <div className="studio-banner" role="alert">
          <TriangleAlert aria-hidden="true" size="1.125rem" />
          <div><p className="studio-banner-title">Someone else saved this draft</p><p>Your latest changes here weren’t saved. Reload to see theirs, then make your changes again.</p></div>
          <Button variant="secondary" onPress={() => void load()}><RotateCw aria-hidden="true" size="0.875rem" /> Reload</Button>
        </div>
      )}
      {notice && <div className="studio-notice"><Notice tone={notice.tone} title={notice.title} /><button type="button" className="icon-btn" aria-label="Dismiss" onClick={() => setNotice(null)}><X aria-hidden="true" size="0.875rem" /></button></div>}
      {model.readOnly && <Notice tone="warning" title="The studio can’t edit this process yet">{model.readOnly} It keeps working as it is.</Notice>}
      {inReview && <Notice tone="info" title="This version is in review">You can edit again once it’s approved or sent back.</Notice>}
      {approved && <Notice tone="info" title="This version is approved">Publish it to make it live. It can’t be edited now.</Notice>}
      {!detail.can.edit && <Notice tone="info" title="You can look, but not edit">Ask a workspace admin for process design rights if you need them.</Notice>}
      {!wide && <div className="studio-mobile-note"><Monitor aria-hidden="true" size="1.125rem" /><p>Editing needs a larger screen. Here is a summary of the steps.</p></div>}

      {problems && problems.list.length > 0 && (
        <section className="studio-problems" aria-labelledby="studio-problems-h">
          <div className="studio-problems-head">
            <h2 id="studio-problems-h">{errorCount ? `${errorCount} ${errorCount === 1 ? 'problem' : 'problems'} to fix` : 'Warnings'}</h2>
            <span className="muted">As of the last check</span>
            <button type="button" className="icon-btn" aria-label="Hide problems" onClick={() => setProblems(null)}><X aria-hidden="true" size="0.875rem" /></button>
          </div>
          <ul className="studio-problem-list">
            {problems.list.map((p, i) => {
              const t = problemTarget(model, p, problems.checked);
              const where = t.step ? model.steps.find((s) => s.id === t.step)?.name : t.field ? model.fields.find((f) => f.name === t.field)?.label : null;
              return (
                <li key={i} className={`studio-problem is-${p.severity}`}>
                  {p.severity === 'error' ? <CircleAlert aria-hidden="true" size="0.9375rem" /> : <TriangleAlert aria-hidden="true" size="0.9375rem" />}
                  <div className="studio-problem-main">
                    <p><span className="visually-hidden">{p.severity === 'error' ? 'Error: ' : 'Warning: '}</span>{p.message}</p>
                    <details className="studio-problem-details"><summary>Details</summary><code>{p.code} · {p.pointer || '/'}</code></details>
                  </div>
                  {where && <Button variant="ghost" className="btn-sm" onPress={() => goTo(p)}>Go to {where}</Button>}
                </li>
              );
            })}
          </ul>
        </section>
      )}

      <div className="studio-grid">
        <aside className="studio-left" aria-label="Request and form">
          <RequestPanel model={model} readOnly={readOnly} open={openField} setOpen={setOpenField} problems={target.fields}
            onRequest={(p) => change((m) => setRequest(m, p))}
            onAddField={(label, kind: Exclude<FieldKind, 'other'>, options, required) => {
              const [m1, name] = addField(modelRef.current!, label, kind, options);
              change(() => (required ? updateField(m1, name, { required }) : m1));
              setOpenField(name);
              setSaid(`Added the field ${label}.`);
            }}
            onField={(name, p) => change((m) => updateField(m, name, p))}
            onMoveField={(name, by) => change((m) => moveField(m, name, by))}
            onRemoveField={(name) => { change((m) => removeField(m, name)); setOpenField(null); }} />
        </aside>
        <section className="studio-centre" aria-labelledby="studio-steps-h">
          <div className="studio-col-head">
            <h2 id="studio-steps-h">Steps</h2>
            <p className="muted">What happens after someone submits, in order.</p>
          </div>
          <StepBuilder model={model} selected={selected} onSelect={setSelected} onAction={onStepAction} problems={target.steps} readOnly={readOnly} roleName={roleName} />
        </section>
        {wide && (
          <aside className="studio-right" aria-labelledby="props-h">
            <StepProperties model={model} step={step} onChange={onStepChange} onDelete={() => step && setDeleting(step)} readOnly={readOnly}
              directory={{ groups: groups ?? [], roles: roles ?? [], members: members ?? [] }} problems={step ? target.steps.get(step.id) ?? [] : []} />
          </aside>
        )}
      </div>

      {deleting && (
        <FormDialog isOpen onOpenChange={(o) => !o && setDeleting(null)} title={`Delete ${deleting.name.trim() || 'this step'}?`}
          footer={<><Button variant="secondary" autoFocus onPress={() => setDeleting(null)}>Cancel</Button><Button variant="danger" onPress={() => {
            change((m) => removeStep(m, deleting.id));
            if (selected === deleting.id) setSelected(null);
            setSaid(`Deleted ${deleting.name}.`);
            setDeleting(null);
          }}>Delete step</Button></>}>
          <p>The step and its settings leave this draft. Requests already running keep the version they started on. You can undo this.</p>
        </FormDialog>
      )}
    </div>
  );
}

function SaveStatus({ state, onRetry }: { state: SaveState; onRetry: () => void }) {
  if (state === 'error') return <span className="studio-save is-error"><CloudOff aria-hidden="true" size="0.8125rem" /> Couldn’t save <button type="button" className="studio-save-retry" onClick={onRetry}>Retry</button></span>;
  if (state === 'saving') return <span className="studio-save"><LoaderCircle aria-hidden="true" size="0.8125rem" className="studio-spin" /> Saving…</span>;
  if (state === 'dirty') return <span className="studio-save">Unsaved changes</span>;
  if (state === 'conflict') return <span className="studio-save is-error"><CloudOff aria-hidden="true" size="0.8125rem" /> Not saved</span>;
  return <span className="studio-save is-saved"><Check aria-hidden="true" size="0.8125rem" /> All changes saved</span>;
}
