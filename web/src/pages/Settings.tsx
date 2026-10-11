import { Check, ChevronDown, Lock, ShieldCheck } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type LoadFailure, useItems } from '../lib/data';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { StepUpDialog } from '../ui/StepUp';
import { Tabs } from '../ui/components/Tabs';
import { DepartmentOverrides, ProfileSwitch } from './SettingsScopes';
import { LoadError, Skeleton } from '../ui/States';
import { AREAS, LABELS, LAYER, areaOf, durationText, humanise, sizeText, valueName } from './settingsMeta';

export type Kind =
  | { type: 'bool' } | { type: 'int'; min: number; max: number } | { type: 'duration'; min: number; max: number } | { type: 'size'; min: number; max: number }
  | { type: 'enum'; values: string[] } | { type: 'set'; values: string[]; allow_all: boolean } | { type: 'list'; max_items: number; max_len: number }
  | { type: 'text'; max_len: number } | { type: 'colour' } | { type: 'duration_list'; min: number; max: number; max_items: number };
export interface Setting { key: string; value: unknown; source: string; clamped: boolean; layers: { layer: string; value: unknown }[]; editable_with: string | null; immediate: boolean; step_up: boolean; kind: Kind; revision: number }

/** A value as a short sentence fragment, for read-only display and the explainer. */
export function show(kind: Kind, v: unknown): string {
  switch (kind.type) {
    case 'bool': return v ? 'On' : 'Off';
    case 'duration': return durationText(Number(v));
    case 'size': return sizeText(Number(v));
    case 'enum': return valueName(String(v));
    case 'set': { const a = (v as string[]) ?? []; return a.length ? a.map(valueName).join(', ') : 'None'; }
    case 'list': { const a = (v as string[]) ?? []; return a.length ? a.join(', ') : 'None'; }
    case 'duration_list': { const a = (v as number[]) ?? []; return a.length ? a.map(durationText).join(', ') : 'None'; }
    case 'text': return String(v ?? '') || 'Not set';
    default: return String(v ?? '');
  }
}

/** Admin · Settings (docs/05 §3.16, docs/20): grouped by area, edited one setting at a time. */
export function Settings() {
  const { can } = useSession();
  const [items, reload, error] = useItems<Setting>('/api/v1/settings');
  const groups = useMemo(() => AREAS.map((a) => ({ ...a, items: (items ?? []).filter((s) => areaOf(s.key) === a.id) })).filter((g) => g.items.length), [items]);
  const editable = (s: Setting) => !!s.editable_with && can(s.editable_with);
  const workspaceTab = <WorkspaceSettings items={items} reload={reload} error={error} groups={groups} editable={editable} />;

  return (
    <>
      <div className="page-head">
        <div><h1>Settings</h1><p>How this workspace works. Changes apply to everyone and are recorded in the audit log.</p></div>
      </div>
      <Tabs label="Settings scope" tabs={[
        { id: 'workspace', label: 'Workspace', content: workspaceTab },
        { id: 'departments', label: 'Departments', content: <DepartmentOverrides /> },
        { id: 'profile', label: 'Configuration profile', content: <ProfileSwitch onApplied={reload} /> },
      ]} />
    </>
  );
}

function WorkspaceSettings({ items, reload, error, groups, editable }: { items: Setting[] | null; reload: () => void; error: LoadFailure | null; groups: { id: string; label: string; items: Setting[] }[]; editable: (s: Setting) => boolean }) {
  return (
    <>
      {error ? <LoadError error={error} onRetry={reload} what="settings" /> : items === null ? <Skeleton variant="table" rows={8} label="Loading settings" /> : (
        <div className="settings">
          <nav className="settings-index" aria-label="Settings sections">
            <ul>{groups.map((g) => <li key={g.id}><a href={`#set-${g.id}`}>{g.label}</a></li>)}</ul>
          </nav>
          <div className="settings-main">
            {groups.map((g) => (
              <section key={g.id} id={`set-${g.id}`} className="settings-group" aria-labelledby={`set-${g.id}-h`}>
                <h2 id={`set-${g.id}-h`}>{g.label}</h2>
                <ul className="settings-list">
                  {g.items.map((s) => <SettingRow key={s.key} s={s} canEdit={editable(s)} onSaved={reload} />)}
                </ul>
              </section>
            ))}
          </div>
        </div>
      )}
    </>
  );
}

function SettingRow({ s, canEdit, onSaved }: { s: Setting; canEdit: boolean; onSaved: () => void }) {
  const [label, hint] = LABELS[s.key] ?? [humanise(s.key)];
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState<unknown>(s.value);
  const [reason, setReason] = useState('');
  const [askedReason, setNeedReason] = useState(false);
  const needReason = askedReason || s.immediate; // the server requires a reason for immediate (security) keys
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [why, setWhy] = useState(false);
  const [stepUp, setStepUp] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (!editing) setDraft(s.value); }, [s.value, editing]);
  useEffect(() => { if (!saved) return; const t = setTimeout(() => setSaved(false), 2500); return () => clearTimeout(t); }, [saved]);
  const id = `set-${s.key.replace(/\./g, '-')}`;
  const changed = JSON.stringify(draft) !== JSON.stringify(s.value);

  const save = async () => {
    setBusy(true); setError(null);
    try {
      await api(`/api/v1/settings/${s.key}`, { method: 'PUT', headers: { 'if-match': `"${s.revision}"` }, body: { value: draft, ...(reason.trim() ? { reason: reason.trim() } : {}) } });
      setEditing(false); setReason(''); setNeedReason(false); setSaved(true); onSaved();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(true);
      else if (e instanceof ApiError && e.code === 'reason_required') { setNeedReason(true); setError('Add a reason for this change. It’s kept in the audit log.'); }
      else if (e instanceof ApiError && e.code === 'precondition_failed') setError('Someone else changed this setting. Reload the page to see the new value.');
      else if (e instanceof ApiError && e.code === 'setting_below_floor') setError('That’s looser than the minimum the platform allows.');
      else setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not save. Try again.');
    } finally { setBusy(false); }
  };

  return (
    <li className={`setting${editing ? ' setting-editing' : ''}`} id={id}>
      <div className="setting-head">
        <div className="setting-text">
          <p className="setting-label" id={`${id}-l`}>{label}</p>
          {hint && <p className="setting-hint" id={`${id}-h`}>{hint}</p>}
          <p className="setting-meta">
            {s.source !== 'default' && <span className="setting-source">{LAYER[s.source] ?? s.source}</span>}
            {s.step_up && <span className="setting-flag"><ShieldCheck aria-hidden="true" size="0.75rem" /> Asks you to confirm it’s you</span>}
            {!s.editable_with && <span className="setting-flag"><Lock aria-hidden="true" size="0.75rem" /> Set by the platform</span>}
            <button type="button" className="setting-why" aria-expanded={why} aria-controls={`${id}-why`} onClick={() => setWhy(!why)}>Why this value<ChevronDown aria-hidden="true" size="0.75rem" /></button>
          </p>
        </div>
        {!editing && (
          <div className="setting-value">
            <span className="setting-current">{s.kind.type === 'colour' && <span className="swatch" style={{ background: String(s.value) }} aria-hidden="true" />}{show(s.kind, s.value)}</span>
            {saved && <span className="setting-saved" role="status"><Check aria-hidden="true" size="0.8125rem" /> Saved</span>}
            {canEdit && <Button variant="secondary" className="btn-sm" onPress={() => { setEditing(true); setError(null); }} aria-label={`Change ${label}`}>Change</Button>}
          </div>
        )}
      </div>
      {why && <Why s={s} id={`${id}-why`} />}
      {editing && (
        <div className="setting-edit">
          {error && <Notice tone="danger" title={error} />}
          <Control kind={s.kind} value={draft} onChange={setDraft} labelledBy={`${id}-l`} describedBy={hint ? `${id}-h` : undefined} />
          {needReason && (
            <div className="field setting-reason">
              <label className="field-label" htmlFor={`${id}-r`}>Reason for the change</label>
              <input id={`${id}-r`} className="field-input" value={reason} onChange={(e) => setReason(e.target.value)} aria-describedby={`${id}-rh`} />
              <span className="field-description" id={`${id}-rh`}>Required for security settings. Kept in the audit log.</span>
            </div>
          )}
          <div className="actions">
            <Button onPress={() => void save()} isDisabled={!changed || busy || (needReason && !reason.trim())}>{busy ? 'Saving…' : 'Save'}</Button>
            <Button variant="ghost" onPress={() => { setEditing(false); setDraft(s.value); setNeedReason(false); setError(null); }}>Cancel</Button>
            {s.immediate && <span className="setting-note">Applies right away, including to people who are signed in.</span>}
          </div>
        </div>
      )}
      <StepUpDialog isOpen={stepUp} onClose={() => setStepUp(false)} onDone={() => { setStepUp(false); void save(); }} />
    </li>
  );
}

/** The explainer: every layer that contributed, from `GET /api/v1/settings/effective?key=`. */
function Why({ s, id }: { s: Setting; id: string }) {
  const [e, setE] = useState<Setting | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => { api<Setting>(`/api/v1/settings/effective?key=${encodeURIComponent(s.key)}`).then(setE).catch(() => setFailed(true)); }, [s.key, s.revision]);
  const x = e ?? s;
  return (
    <div className="setting-why-panel" id={id}>
      {failed && <p className="muted">Couldn’t load the explanation. Showing what this page already knows.</p>}
      <ol className="layers">
        {x.layers.map((l, i) => {
          const winner = l.layer === x.source || (x.source === 'platform_floor' && l.layer === 'platform_floor');
          return (
            <li key={`${l.layer}-${i}`} className={winner ? 'layer-win' : ''}>
              <span className="layer-name">{LAYER[l.layer] ?? l.layer}</span>
              <span className="layer-value">{show(x.kind, l.value)}</span>
              {winner && <span className="layer-tag">In effect</span>}
            </li>
          );
        })}
      </ol>
      {x.clamped && <p className="setting-note">The value saved for this workspace is looser than the platform minimum, so the minimum applies.</p>}
      <p className="setting-key">Key <code>{x.key}</code></p>
    </div>
  );
}

const DUR_UNITS = [[86_400, 'days'], [3_600, 'hours'], [60, 'minutes']] as const;

export function Control({ kind, value, onChange, labelledBy, describedBy }: { kind: Kind; value: unknown; onChange: (v: unknown) => void; labelledBy: string; describedBy?: string }) {
  const a11y = { 'aria-labelledby': labelledBy, 'aria-describedby': describedBy };
  switch (kind.type) {
    case 'bool':
      return (
        <label className="switch-row">
          <input type="checkbox" role="switch" className="switch" checked={!!value} onChange={(e) => onChange(e.target.checked)} {...a11y} />
          <span>{value ? 'On' : 'Off'}</span>
        </label>
      );
    case 'int':
      return <input type="number" className="field-input setting-num" min={kind.min} max={kind.max} value={Number(value)} onChange={(e) => onChange(Number(e.target.value))} {...a11y} />;
    case 'size':
      return <span className="setting-inline"><input type="number" className="field-input setting-num" min={Math.ceil(kind.min / 1_048_576)} max={kind.max / 1_048_576} value={Math.round(Number(value) / 1_048_576)} onChange={(e) => onChange(Number(e.target.value) * 1_048_576)} {...a11y} /><span>MB</span></span>;
    case 'duration': {
      const secs = Number(value);
      const unit = DUR_UNITS.find(([n]) => secs % n === 0 && secs >= n)?.[0] ?? 60;
      return (
        <span className="setting-inline">
          <input type="number" className="field-input setting-num" min={0} value={secs / unit} onChange={(e) => onChange(Math.round(Number(e.target.value) * unit))} {...a11y} />
          <select className="field-input setting-unit" aria-label="Unit" value={unit} onChange={(e) => onChange(Math.round((secs / unit) * Number(e.target.value)))}>
            {DUR_UNITS.map(([n, l]) => <option key={n} value={n}>{l}</option>)}
          </select>
          <span className="setting-range">Between {durationText(kind.min)} and {durationText(kind.max)}</span>
        </span>
      );
    }
    case 'enum':
      return kind.values.length <= 4 ? (
        <div className="segmented" role="group" {...a11y}>
          {kind.values.map((v) => <button key={v} type="button" aria-pressed={value === v} onClick={() => onChange(v)}>{valueName(v)}</button>)}
        </div>
      ) : (
        <select className="field-input setting-select" value={String(value)} onChange={(e) => onChange(e.target.value)} {...a11y}>
          {kind.values.map((v) => <option key={v} value={v}>{valueName(v)}</option>)}
        </select>
      );
    case 'set': {
      const cur = (value as string[]) ?? [];
      const all = cur.includes('*');
      const toggle = (v: string) => onChange(cur.includes(v) ? cur.filter((x) => x !== v) : [...cur.filter((x) => x !== '*'), v]);
      return (
        <div className="aud-chips" role="group" {...a11y}>
          {kind.allow_all && <button type="button" className="aud-chip" aria-pressed={all} onClick={() => onChange(all ? [] : ['*'])}>{all && <Check aria-hidden="true" size="0.75rem" />}Everyone</button>}
          {kind.values.map((v) => { const on = !all && cur.includes(v); return <button key={v} type="button" className="aud-chip" aria-pressed={on} disabled={all} onClick={() => toggle(v)}>{on && <Check aria-hidden="true" size="0.75rem" />}{valueName(v)}</button>; })}
        </div>
      );
    }
    case 'list':
      return <textarea className="textarea textarea-xs" value={((value as string[]) ?? []).join('\n')} onChange={(e) => onChange(e.target.value.split(/[\n,]/).map((x) => x.trim()).filter(Boolean))} {...a11y} />;
    case 'duration_list':
      return <input className="field-input" value={((value as number[]) ?? []).map((x) => x / 86_400).join(', ')} onChange={(e) => onChange(e.target.value.split(',').map((x) => Math.round(Number(x.trim()) * 86_400)).filter((x) => x > 0))} {...a11y} />;
    case 'colour':
      return (
        <span className="setting-inline">
          <input type="color" className="setting-colour" value={String(value)} onChange={(e) => onChange(e.target.value.toUpperCase())} {...a11y} />
          <input className="field-input setting-hex" value={String(value)} onChange={(e) => onChange(e.target.value)} aria-label="Hex colour" maxLength={7} />
        </span>
      );
    default:
      return <input className="field-input" maxLength={'max_len' in kind ? kind.max_len : undefined} value={String(value ?? '')} onChange={(e) => onChange(e.target.value)} {...a11y} />;
  }
}
