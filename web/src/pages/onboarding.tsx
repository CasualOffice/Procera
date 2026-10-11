import { Check, Circle, Rocket, X } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';
import { Skeleton } from '../ui/States';
import { useStepUp } from '../ui/StepUp';
import type { Pack } from './Templates';

/*
 * Workspace onboarding (docs/05 §3.1, approved 2026-10-12): a checklist on Home that ticks itself
 * from real data, and a short guided setup at /welcome. Nothing is forced: deep links still work.
 */

interface SettingRow { key: string; value: unknown; layers: { layer: string }[]; revision: number }
interface MemberRow { user_id: string; roles: string[]; status: string }
export interface SetupStep { id: string; label: string; hint: string; done: boolean; to: string }

const items = <T,>(path: string) => api<{ items: T[] }>(path).then((r) => r.items).catch(() => [] as T[]);

/** Each step's state, read from the workspace (never from what was clicked). */
export function useSetup(): { steps: SetupStep[] | null; reload: () => void } {
  const { can, user } = useSession();
  const [steps, setSteps] = useState<SetupStep[] | null>(null);
  const load = useCallback(() => {
    if (!can('workspace:settings.manage')) { setSteps([]); return; }
    void Promise.all([
      items<SettingRow>('/api/v1/settings'),
      items<{ id: string }>('/api/v1/departments'),
      items<MemberRow>('/api/v1/members'),
      items<{ id: string }>('/api/v1/members/invitations'),
      items<{ state: string }>('/api/v1/artifacts'),
      can('campaign:view') ? items<{ id: string }>('/api/v1/campaigns') : Promise.resolve([]),
    ]).then(([settings, depts, members, invites, docs, campaigns]) => {
      const set = (k: string) => settings.find((s) => s.key === k);
      const changed = (k: string) => !!set(k)?.layers.some((l) => l.layer === 'workspace');
      const others = members.filter((m) => m.user_id !== user?.id && m.status === 'active');
      const mfa = (set('auth.mfa.required_roles')?.value as string[] | undefined) ?? [];
      setSteps([
        { id: 'brand', label: 'Name and brand your workspace', hint: 'Logo, colour and time zone', done: changed('branding.accent') || changed('branding.logo'), to: '/welcome' },
        { id: 'departments', label: 'Add your departments', hint: 'Used to target documents and campaigns', done: depts.length > 0, to: '/welcome?step=3' },
        { id: 'invite', label: 'Invite your team', hint: 'They join by email', done: others.length > 0 || invites.length > 0, to: '/welcome?step=4' },
        { id: 'roles', label: 'Choose who reviews and who manages', hint: 'Reviewers, compliance, admins', done: others.some((m) => m.roles.some((r) => r !== 'member')), to: '/admin/roles' },
        { id: 'publish', label: 'Publish your first policy', hint: 'Start from the starter pack', done: docs.some((d) => d.state === 'published'), to: '/library' },
        { id: 'campaign', label: 'Ask everyone to acknowledge it', hint: 'Track who has read it', done: campaigns.length > 0, to: '/admin/campaigns?new=1' },
        { id: 'mfa', label: 'Require two-step verification for admins', hint: 'Protects the accounts that can change things', done: mfa.includes('workspace.admin'), to: '/admin/settings' },
      ]);
    });
  }, [can, user?.id]);
  useEffect(load, [load]);
  return { steps, reload: load };
}

const HIDE_KEY = (ws: string) => `procera.setup.hidden.${ws}`;

/** Home card for owners and admins until every step is done; can be hidden per browser. */
export function SetupChecklist() {
  const { workspace } = useSession();
  const { steps } = useSetup();
  const [hidden, setHidden] = useState(() => {
    try { return !!workspace && localStorage.getItem(HIDE_KEY(workspace.workspace_id)) === '1'; } catch { return false; }
  });
  if (!steps || steps.length === 0 || hidden || !workspace) return null;
  const done = steps.filter((s) => s.done).length;
  if (done === steps.length) return null;
  const hide = () => {
    try { localStorage.setItem(HIDE_KEY(workspace.workspace_id), '1'); } catch { /* storage unavailable: hidden for this page only */ }
    setHidden(true);
  };
  return (
    <section className="setup card" aria-labelledby="setup-title">
      <div className="setup-head">
        <span className="quick-icon tag-purple"><Rocket aria-hidden="true" size="1rem" /></span>
        <div className="setup-heading">
          <h2 id="setup-title">Set up {workspace.name}</h2>
          <p>{done} of {steps.length} done. Each step ticks itself when it’s in place.</p>
        </div>
        <Link className="btn btn-primary btn-md link-btn" to="/welcome">{done === 0 ? 'Start setup' : 'Continue setup'}</Link>
        <Button variant="ghost" className="btn-sm" aria-label="Hide the setup checklist" onPress={hide}><X aria-hidden="true" size="1rem" /></Button>
      </div>
      <div className="setup-bar" role="progressbar" aria-label="Setup progress" aria-valuemin={0} aria-valuemax={steps.length} aria-valuenow={done}><span style={{ width: `${(done / steps.length) * 100}%` }} /></div>
      <ol className="setup-steps">
        {steps.map((s) => (
          <li key={s.id} className={s.done ? 'is-done' : undefined}>
            {s.done ? <Check aria-hidden="true" size="1rem" className="setup-tick" /> : <Circle aria-hidden="true" size="1rem" className="setup-dot" />}
            {s.done ? <span>{s.label}<span className="visually-hidden"> (done)</span></span> : <Link to={s.to}>{s.label}</Link>}
            <span className="setup-hint">{s.hint}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}

const STEPS = ['Your workspace', 'Starting point', 'Departments', 'Invite people'];
const INVITE_ROLES = [{ id: 'member', label: 'Member' }, { id: 'reviewer', label: 'Reviewer' }, { id: 'compliance.officer', label: 'Compliance' }, { id: 'workspace.admin', label: 'Admin' }];
const message = (e: unknown, fallback: string) => (e instanceof ApiError ? (e.problem.detail ?? e.message) : fallback);

/** /welcome: four short, skippable steps; each saves as it goes and the last one lands on Home. */
export function Welcome() {
  const { workspace, can, reload } = useSession();
  const first = Number(new URLSearchParams(window.location.search).get('step') ?? '1');
  const [step, setStep] = useState(first >= 1 && first <= 4 ? first : 1);
  const next = () => (step < 4 ? setStep(step + 1) : navigate('/'));
  if (!workspace) return <Skeleton rows={3} />;
  if (!can('workspace:settings.manage')) {
    return <Notice tone="info" title="Only owners and admins set up a workspace">Ask a workspace admin if something is missing.</Notice>;
  }
  return (
    <div className="welcome">
      <div className="page-head"><div><h1>Set up {workspace.name}</h1><p>Four short steps. Skip anything and come back to it from Home.</p></div></div>
      <ol className="welcome-steps" aria-label="Setup steps">
        {STEPS.map((label, i) => (
          <li key={label} aria-current={step === i + 1 ? 'step' : undefined} className={step > i + 1 ? 'is-done' : undefined}>
            <button type="button" onClick={() => setStep(i + 1)}><span className="welcome-num">{step > i + 1 ? <Check aria-hidden="true" size="0.875rem" /> : i + 1}</span>{label}</button>
          </li>
        ))}
      </ol>
      <div className="card welcome-card">
        {step === 1 && <BrandStep onDone={async () => { await reload(); next(); }} onSkip={next} />}
        {step === 2 && <PackStep onDone={next} />}
        {step === 3 && <DepartmentsStep onDone={next} />}
        {step === 4 && <InviteStep onDone={next} />}
      </div>
    </div>
  );
}

function StepActions({ busy, label, onSkip }: { busy: boolean; label: string; onSkip: () => void }) {
  return (
    <div className="form-actions">
      <Button variant="ghost" onPress={onSkip}>Skip for now</Button>
      <Button type="submit" isDisabled={busy}>{busy ? 'Saving…' : label}</Button>
    </div>
  );
}

function BrandStep({ onDone, onSkip }: { onDone: () => Promise<void>; onSkip: () => void }) {
  const { workspace } = useSession();
  const [name, setName] = useState(workspace?.name ?? '');
  const [accent, setAccent] = useState('#EE4136');
  const [tz, setTz] = useState(Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC');
  const [settings, setSettings] = useState<SettingRow[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { guard, dialog } = useStepUp((e) => { setBusy(false); setError(message(e, 'Could not save')); });
  useEffect(() => {
    items<SettingRow>('/api/v1/settings').then((s) => {
      setSettings(s);
      const a = s.find((x) => x.key === 'branding.accent')?.value;
      if (typeof a === 'string') setAccent(a);
    });
  }, []);
  const put = async (key: string, value: unknown) => {
    const s = settings?.find((x) => x.key === key);
    if (!s || JSON.stringify(s.value) === JSON.stringify(value)) return;
    await api(`/api/v1/settings/${key}`, { method: 'PUT', headers: { 'if-match': `"${s.revision}"` }, body: { value } });
  };
  const save = () => guard(async () => {
    setBusy(true); setError(null);
    if (name.trim() && name.trim() !== workspace?.name) await api('/api/v1/workspace', { method: 'PATCH', body: { name: name.trim() } });
    await put('branding.accent', accent.toUpperCase());
    await put('locale.timezone', tz);
    setBusy(false);
    await onDone();
  });
  const zones = (typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : ['UTC']).map((z) => ({ id: z, label: z.replaceAll('_', ' ') }));
  if (!zones.some((z) => z.id === tz)) zones.unshift({ id: tz, label: tz });
  return (
    <Form onSubmit={(e) => { e.preventDefault(); void save(); }} className="welcome-form">
      <h2>Your workspace</h2>
      <p className="welcome-lede">How it appears to everyone, in the app and in emails. A logo can be added later in Settings.</p>
      {error && <Notice tone="danger" title="Couldn’t save">{error}</Notice>}
      <TextField label="Workspace name" value={name} onChange={setName} isRequired maxLength={120} />
      <div className="welcome-accent">
        <label htmlFor="welcome-accent">Accent colour</label>
        <input id="welcome-accent" type="color" value={accent} onChange={(e) => setAccent(e.target.value)} />
        <span className="welcome-accent-hint">Used for links and highlights. Procera adjusts it so text stays readable.</span>
      </div>
      <Select label="Time zone" options={zones} selectedKey={tz} onSelectionChange={(k) => k && setTz(String(k))} description="Due dates, reminders and digests follow it." />
      <StepActions busy={busy || settings === null} label="Save and continue" onSkip={onSkip} />
      {dialog}
    </Form>
  );
}

function PackStep({ onDone }: { onDone: () => void }) {
  const [packs, setPacks] = useState<Pack[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string[]>([]);
  useEffect(() => { items<Pack>('/api/v1/templates').then(setPacks); }, []);
  const installable = (packs ?? []).filter((p) => p.can_install);
  const install = async () => {
    setBusy(true); setError(null);
    try {
      for (const p of installable) {
        await api(`/api/v1/templates/${p.key}/install`, { method: 'POST', body: { documents: p.documents.map((d) => d.key), campaign: false, on_conflict: 'skip' } });
        setDone((d) => [...d, p.key]);
      }
      onDone();
    } catch (e) {
      setError(message(e, 'Could not install the starter packs'));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Form onSubmit={(e) => { e.preventDefault(); void install(); }} className="welcome-form">
      <h2>Starting point</h2>
      <p className="welcome-lede">Start from reviewed starter documents instead of a blank page. They install as drafts for you to adapt, review and publish.</p>
      {error && <Notice tone="danger" title="Couldn’t install">{error}</Notice>}
      {packs === null ? <Skeleton rows={2} /> : installable.length === 0 ? <p>No starter packs are available to you.</p> : (
        <ul className="welcome-packs">
          {installable.map((p) => (
            <li key={p.key}>
              {done.includes(p.key) ? <Check aria-hidden="true" size="1rem" className="setup-tick" /> : <Circle aria-hidden="true" size="1rem" className="setup-dot" />}
              <span><strong>{p.name}</strong> · {p.documents.length} documents<span className="setup-hint">{p.description}</span></span>
            </li>
          ))}
        </ul>
      )}
      <StepActions busy={busy || packs === null} label={installable.length ? 'Install and continue' : 'Continue'} onSkip={onDone} />
    </Form>
  );
}

function DepartmentsStep({ onDone }: { onDone: () => void }) {
  const [text, setText] = useState('');
  const [existing, setExisting] = useState<{ name: string }[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { items<{ name: string }>('/api/v1/departments').then(setExisting); }, []);
  const names = [...new Set(text.split('\n').map((l) => l.trim()).filter(Boolean))].filter((n) => !(existing ?? []).some((d) => d.name.toLowerCase() === n.toLowerCase()));
  const save = async () => {
    setBusy(true); setError(null);
    try {
      for (const name of names) await api('/api/v1/departments', { method: 'POST', body: { name } });
      onDone();
    } catch (e) {
      setError(message(e, 'Could not add the departments'));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Form onSubmit={(e) => { e.preventDefault(); if (names.length) void save(); else onDone(); }} className="welcome-form">
      <h2>Departments</h2>
      <p className="welcome-lede">Each person belongs to one. They’re used to target documents, campaigns and announcements, and to scope roles. You can nest and rearrange them later.</p>
      {error && <Notice tone="danger" title="Couldn’t add departments">{error}</Notice>}
      {existing && existing.length > 0 && <p className="welcome-existing">Already here: {existing.map((d) => d.name).join(', ')}</p>}
      <div className="field">
        <label className="field-label" htmlFor="welcome-depts">Departments, one per line</label>
        <textarea id="welcome-depts" className="field-input welcome-textarea" rows={6} value={text} onChange={(e) => setText(e.target.value)} placeholder={'People\nFinance\nEngineering\nSales'} />
      </div>
      <StepActions busy={busy} label={names.length ? `Add ${names.length} and continue` : 'Continue'} onSkip={onDone} />
    </Form>
  );
}

function InviteStep({ onDone }: { onDone: () => void }) {
  const [rows, setRows] = useState([{ email: '', role: 'member' }, { email: '', role: 'member' }, { email: '', role: 'member' }]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const filled = rows.filter((r) => r.email.trim());
  const update = (i: number, patch: Partial<{ email: string; role: string }>) => setRows(rows.map((r, j) => (j === i ? { ...r, ...patch } : r)));
  const send = async () => {
    setBusy(true); setError(null);
    try {
      for (const r of filled) {
        await api('/api/v1/members/invitations', { method: 'POST', body: { email: r.email.trim(), roles: r.role === 'member' || r.role === 'workspace.admin' ? [r.role] : ['member', r.role] } });
      }
      onDone();
    } catch (e) {
      setError(message(e, 'Could not send the invitations'));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Form onSubmit={(e) => { e.preventDefault(); if (filled.length) void send(); else onDone(); }} className="welcome-form">
      <h2>Invite people</h2>
      <p className="welcome-lede">They get an email to join. Everyone can read and acknowledge; pick a role for people who review or manage.</p>
      {error && <Notice tone="danger" title="Couldn’t invite everyone">{error}</Notice>}
      <ul className="welcome-invites">
        {rows.map((r, i) => (
          <li key={i}>
            <TextField label={`Email ${i + 1}`} type="email" value={r.email} onChange={(v) => update(i, { email: v })} />
            <Select label={`Role ${i + 1}`} options={INVITE_ROLES} selectedKey={r.role} onSelectionChange={(k) => k && update(i, { role: String(k) })} />
          </li>
        ))}
      </ul>
      <Button variant="ghost" className="btn-sm" onPress={() => setRows([...rows, { email: '', role: 'member' }])}>Add another</Button>
      <StepActions busy={busy} label={filled.length ? `Invite ${filled.length} and finish` : 'Finish'} onSkip={onDone} />
    </Form>
  );
}
