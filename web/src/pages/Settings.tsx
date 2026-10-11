import { Archive, Bell, Check, ChevronDown, ChevronRight, FileText, Lock, Megaphone, Palette, Search, ShieldCheck, Users, type LucideIcon } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Input, SearchField } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { useItems } from '../lib/data';
import { Link, navigate } from '../lib/router';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { StepUpDialog } from '../ui/StepUp';
import { TextField } from '../ui/components/TextField';
import { StartingPoint } from './SettingsScopes';
import { LoadError, Skeleton } from '../ui/States';
import { CATEGORIES, type CategoryId, LAYER, SYNONYMS, anchorOf, categoryById, durationText, isHidden, labelOf, placeOf, redirectFor, sizeText, valueName } from './settingsMeta';
import { contrast, textShade } from '../lib/contrast';
import { uploadFile } from '../lib/upload';
import { type Label, TERMS, TERM_DEFAULTS, setTerms } from '../lib/terms';

export type Kind =
  | { type: 'bool' } | { type: 'int'; min: number; max: number } | { type: 'duration'; min: number; max: number } | { type: 'size'; min: number; max: number }
  | { type: 'enum'; values: string[] } | { type: 'set'; values: string[]; allow_all: boolean } | { type: 'list'; max_items: number; max_len: number }
  | { type: 'text'; max_len: number } | { type: 'colour' } | { type: 'image' } | { type: 'duration_list'; min: number; max: number; max_items: number }
  | { type: 'terms'; terms: string[]; max_len: number };
export interface Setting { key: string; value: unknown; source: string; clamped: boolean; layers: { layer: string; value: unknown }[]; editable_with: string | null; immediate: boolean; step_up: boolean; kind: Kind; revision: number; department_scopable?: boolean }

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
    case 'image': return v ? 'Uploaded image' : 'Not set';
    case 'terms': { const e = Object.entries((v as Record<string, Label>) ?? {}); return e.length ? e.map(([t, l]) => `${TERM_DEFAULTS[t as keyof typeof TERM_DEFAULTS]?.singular ?? t} → ${l.singular}`).join(', ') : 'Product defaults'; }
    default: return String(v ?? '');
  }
}

/** Changed by an admin since the workspace was created (a workspace layer replaced the starting value). */
export const isChanged = (s: Setting) => s.layers.some((l) => l.layer === 'workspace');

export const CATEGORY_ICON: Record<CategoryId, LucideIcon> = {
  general: Palette, security: ShieldCheck, people: Users, documents: FileText, acknowledgements: Megaphone, notifications: Bell, data: Archive,
};

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** Search over label, help, key and synonyms (docs/design/ia-competitive-analysis.md §6.3 rule 5). */
function searchSettings(items: Setting[], q: string): Setting[] {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  return items.filter((s) => {
    const [label, hint] = labelOf(s.key);
    const hay = `${label} ${hint ?? ''} ${s.key} ${SYNONYMS[s.key] ?? ''} ${categoryById(placeOf(s.key).cat)?.label ?? ''}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}
const resultLink = (s: Setting) => `/admin/settings/${placeOf(s.key).cat}#${anchorOf(s.key)}`;

/**
 * Admin · Settings (docs/05 §3.16): the category index. Each category is its own page; search finds
 * any setting across them. The starting point (configuration profile) shows once, as a banner.
 */
export function Settings() {
  const [items, reload, error] = useItems<Setting>('/api/v1/settings');
  const [q, setQ] = useState('');
  // Links to the old one-page layout (`#set-<area>` or `#set-<key>`) open the category page now holding it.
  useEffect(() => { const to = redirectFor(window.location.hash); if (to) navigate(to, { replace: true }); }, []);
  const visible = useMemo(() => (items ?? []).filter((s) => !isHidden(s.key)), [items]);
  const results = useMemo(() => searchSettings(visible, q), [visible, q]);
  const searching = q.trim().length > 0;

  return (
    <>
      <div className="page-head">
        <div><h1>Settings</h1><p>How this workspace works. Changes apply to everyone and are recorded in the audit log.</p></div>
      </div>
      {error ? <LoadError error={error} onRetry={reload} what="settings" /> : items === null ? <Skeleton variant="table" rows={7} label="Loading settings" /> : (
        <div className="settings-home">
          <StartingPoint onApplied={reload} />
          <SearchField className="search-input settings-search" aria-label="Search settings" value={q} onChange={setQ}
            onSubmit={() => { const first = results[0]; if (first) navigate(resultLink(first)); }}>
            <Search aria-hidden="true" size="0.9375rem" /><Input placeholder="Search settings, for example password or reminders" />
          </SearchField>
          {searching ? (
            <section aria-label="Search results" className="settings-results">
              <p className="settings-results-count" role="status">{results.length ? `${plural(results.length, 'setting')} match` : 'No settings match. Try another word, such as password, reminders or retention.'}</p>
              {results.length > 0 && (
                <ul className="settings-cats">
                  {results.map((s) => {
                    const [label, hint] = labelOf(s.key);
                    const cat = categoryById(placeOf(s.key).cat)!;
                    return (
                      <li key={s.key} className="settings-cat">
                        <span className="settings-cat-text">
                          <Link to={resultLink(s)} className="settings-cat-link">{label}</Link>
                          <span className="settings-cat-about">{cat.label}{placeOf(s.key).advanced ? ' › Advanced' : ''}{hint ? ` · ${hint}` : ''}</span>
                        </span>
                        <span className="settings-cat-meta">{show(s.kind, s.value)}</span>
                      </li>
                    );
                  })}
                </ul>
              )}
            </section>
          ) : (
            <nav aria-label="Settings categories">
              <ul className="settings-cats">
                {CATEGORIES.map((c) => {
                  const mine = visible.filter((s) => placeOf(s.key).cat === c.id);
                  const changed = mine.filter(isChanged).length;
                  const Icon = CATEGORY_ICON[c.id];
                  return (
                    <li key={c.id} className="settings-cat">
                      <span className="settings-cat-icon" aria-hidden="true"><Icon size="1rem" /></span>
                      <span className="settings-cat-text">
                        <Link to={`/admin/settings/${c.id}`} className="settings-cat-link">{c.label}</Link>
                        <span className="settings-cat-about">{c.about}</span>
                      </span>
                      <span className="settings-cat-meta">{changed ? `${changed} changed` : 'Starting values'}</span>
                      <ChevronRight aria-hidden="true" size="1rem" className="settings-cat-chev" />
                    </li>
                  );
                })}
              </ul>
            </nav>
          )}
        </div>
      )}
    </>
  );
}

/** The workspace's name, shown in the switcher, emails and the browser tab. Owners and admins rename it. */
export function WorkspaceName() {
  const { workspace, can, reload } = useSession();
  const [name, setName] = useState(workspace?.name ?? '');
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  useEffect(() => setName(workspace?.name ?? ''), [workspace?.name]);
  if (!workspace) return null;
  const editable = can('workspace:settings.manage');
  const dirty = name.trim() !== workspace.name;
  const save = async () => {
    setBusy(true); setErr(null); setSaved(false);
    try {
      await api('/api/v1/workspace', { method: 'PATCH', body: { name: name.trim() } });
      await reload();
      setSaved(true);
    } catch (e) {
      setErr(e instanceof ApiError ? (e.problem.detail ?? e.message) : 'Could not rename the workspace');
    } finally {
      setBusy(false);
    }
  };
  return (
    <section className="workspace-name" aria-labelledby="ws-name-h">
      <h2 id="ws-name-h" className="visually-hidden">Workspace name</h2>
      <form className="workspace-name-form" onSubmit={(e) => { e.preventDefault(); if (dirty && name.trim()) void save(); }}>
        <TextField label="Workspace name" value={name} onChange={(v) => { setName(v); setSaved(false); }} isReadOnly={!editable} isRequired maxLength={120}
          description={editable ? 'Shown in the workspace switcher, emails and the browser tab.' : 'Only owners and admins can rename the workspace.'} />
        {editable && <Button type="submit" variant="secondary" isDisabled={!dirty || busy || !name.trim()}>{busy ? 'Saving…' : saved && !dirty ? <><Check aria-hidden="true" size="0.875rem" /> Saved</> : 'Rename'}</Button>}
      </form>
      {err && <Notice tone="danger" title="Couldn’t rename the workspace">{err}</Notice>}
    </section>
  );
}

/**
 * One setting: label, help, value and source; edited in place. `hide` drops choices that don't exist yet
 * (they stay in the stored value), `extra` adds context such as the department chip.
 */
export function SettingRow({ s, canEdit, onSaved, hide, extra }: { s: Setting; canEdit: boolean; onSaved: () => void; hide?: (choice: string) => boolean; extra?: React.ReactNode }) {
  const [label, hint] = labelOf(s.key);
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
  const id = anchorOf(s.key);
  const shown = hide && s.kind.type === 'set' ? ((s.value as string[]) ?? []).filter((v) => !hide(v)) : s.value;
  const changed = JSON.stringify(draft) !== JSON.stringify(s.value);

  const save = async () => {
    setBusy(true); setError(null);
    try {
      await api(`/api/v1/settings/${s.key}`, { method: 'PUT', headers: { 'if-match': `"${s.revision}"` }, body: { value: draft, ...(reason.trim() ? { reason: reason.trim() } : {}) } });
      if (s.kind.type === 'terms') setTerms(draft as Record<string, Label>); // every screen picks up the new terms now
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
            {extra}
          </p>
        </div>
        {!editing && (
          <div className="setting-value">
            <span className="setting-current">{s.kind.type === 'colour' && <span className="swatch" style={{ background: String(s.value) }} aria-hidden="true" />}{show(s.kind, shown)}</span>
            {saved && <span className="setting-saved" role="status"><Check aria-hidden="true" size="0.8125rem" /> Saved</span>}
            {canEdit && <Button variant="secondary" className="btn-sm" onPress={() => { setEditing(true); setError(null); }} aria-label={`Change ${label}`}>Change</Button>}
          </div>
        )}
      </div>
      {why && <Why s={s} id={`${id}-why`} />}
      {editing && (
        <div className="setting-edit">
          {error && <Notice tone="danger" title={error} />}
          <Control kind={s.kind} value={draft} onChange={setDraft} labelledBy={`${id}-l`} describedBy={hint ? `${id}-h` : undefined} hide={hide} />
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

export function Control({ kind, value, onChange, labelledBy, describedBy, hide }: { kind: Kind; value: unknown; onChange: (v: unknown) => void; labelledBy: string; describedBy?: string; hide?: (choice: string) => boolean }) {
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
          {kind.values.filter((v) => !hide?.(v)).map((v) => { const on = !all && cur.includes(v); return <button key={v} type="button" className="aud-chip" aria-pressed={on} disabled={all} onClick={() => toggle(v)}>{on && <Check aria-hidden="true" size="0.75rem" />}{valueName(v)}</button>; })}
        </div>
      );
    }
    case 'list':
      return <textarea className="textarea textarea-xs" value={((value as string[]) ?? []).join('\n')} onChange={(e) => onChange(e.target.value.split(/[\n,]/).map((x) => x.trim()).filter(Boolean))} {...a11y} />;
    case 'duration_list':
      return <input className="field-input" value={((value as number[]) ?? []).map((x) => x / 86_400).join(', ')} onChange={(e) => onChange(e.target.value.split(',').map((x) => Math.round(Number(x.trim()) * 86_400)).filter((x) => x > 0))} {...a11y} />;
    case 'image':
      return <ImageControl value={String(value ?? '')} onChange={onChange} />;
    case 'terms':
      return <TermsControl value={(value as Record<string, Label>) ?? {}} maxLen={kind.max_len} onChange={onChange} labelledBy={labelledBy} />;
    case 'colour':
      return (
        <span className="setting-inline">
          <input type="color" className="setting-colour" value={String(value)} onChange={(e) => onChange(e.target.value.toUpperCase())} {...a11y} />
          <input className="field-input setting-hex" value={String(value)} onChange={(e) => onChange(e.target.value)} aria-label="Hex colour" maxLength={7} />
          <ContrastNote colour={String(value)} />
        </span>
      );
    default:
      return <input className="field-input" maxLength={'max_len' in kind ? kind.max_len : undefined} value={String(value ?? '')} onChange={(e) => onChange(e.target.value)} {...a11y} />;
  }
}

/** Live contrast check for the accent (docs/05 §7): what links will actually use, and why. */
function ContrastNote({ colour }: { colour: string }) {
  const t = textShade(colour);
  if (!t.ratio) return null;
  return (
    <span className="contrast-note" role="status">
      <span className="swatch" style={{ background: t.shade }} aria-hidden="true" />
      {t.adjusted
        ? <>Too light for text on its own ({contrast(colour, '#FFFFFF').toFixed(1)}:1). Links and labels will use <code>{t.shade}</code> ({t.ratio.toFixed(1)}:1) so they stay readable.</>
        : <>Readable as text ({t.ratio.toFixed(1)}:1 on white).</>}
    </span>
  );
}

/** Upload a logo (scanned like any file, then set as the value), or remove it. */
function ImageControl({ value, onChange }: { value: string; onChange: (v: unknown) => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState<string | null>(null);
  const pick = async (f?: File) => {
    if (!f) return;
    setBusy(true); setError(null);
    try {
      if (f.size > 512 * 1024) throw new Error('The image must be 512 KB or smaller.');
      const done = await uploadFile(f);
      if (done.state !== 'clean' || !/^image\/(png|jpeg|gif|webp)$/.test(done.sniffed_type ?? '')) throw new Error('Use a clean PNG, JPEG, GIF or WebP image.');
      setName(f.name); onChange(done.id);
    } catch (e) { setError(e instanceof Error ? e.message : 'The upload failed.'); } finally { setBusy(false); }
  };
  return (
    <span className="setting-inline">
      <input id="logo-input" type="file" accept="image/png,image/jpeg,image/gif,image/webp" className="visually-hidden" onChange={(e) => void pick(e.target.files?.[0])} />
      <label htmlFor="logo-input" className="btn btn-secondary btn-md" role="button" tabIndex={0}>{busy ? 'Uploading…' : value ? 'Replace image' : 'Upload image'}</label>
      {value && <Button variant="ghost" onPress={() => { setName(null); onChange(''); }}>Remove</Button>}
      {name && <span className="muted">{name}</span>}
      {error && <span className="field-error" role="alert">{error}</span>}
    </span>
  );
}

/**
 * Terminology overrides (CFG-008): one row per product term. Empty means the product default; a term
 * needs both forms. Reset clears one row, Clear all goes back to every default.
 */
function TermsControl({ value, maxLen, onChange, labelledBy }: { value: Record<string, Label>; maxLen: number; onChange: (v: unknown) => void; labelledBy: string }) {
  const set = (t: string, field: keyof Label, text: string) => {
    const row = { singular: value[t]?.singular ?? '', plural: value[t]?.plural ?? '', [field]: text };
    const next = { ...value };
    if (!row.singular.trim() && !row.plural.trim()) delete next[t]; else next[t] = row;
    onChange(next);
  };
  const reset = (t: string) => { const next = { ...value }; delete next[t]; onChange(next); };
  return (
    <div className="terms-editor">
      <table className="table terms-table" aria-labelledby={labelledBy}>
        <thead><tr><th scope="col">Term</th><th scope="col">Singular</th><th scope="col">Plural</th><th scope="col"><span className="visually-hidden">Actions</span></th></tr></thead>
        <tbody>
          {TERMS.map((t) => {
            const d = TERM_DEFAULTS[t];
            const row = value[t];
            const partial = !!row && (!row.singular.trim() || !row.plural.trim());
            return (
              <tr key={t}>
                <th scope="row">{d.singular}</th>
                <td><input className="field-input" aria-label={`Singular for ${d.singular}`} placeholder={d.singular} maxLength={maxLen} value={row?.singular ?? ''} onChange={(e) => set(t, 'singular', e.target.value)} aria-invalid={partial && !row?.singular.trim() ? true : undefined} /></td>
                <td><input className="field-input" aria-label={`Plural for ${d.singular}`} placeholder={d.plural} maxLength={maxLen} value={row?.plural ?? ''} onChange={(e) => set(t, 'plural', e.target.value)} aria-invalid={partial && !row?.plural.trim() ? true : undefined} /></td>
                <td>{row && <Button variant="ghost" className="btn-sm" onPress={() => reset(t)} aria-label={`Reset ${d.singular}`}>Reset</Button>}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <div className="setting-inline">
        {Object.values(value).some((r) => !r.singular.trim() || !r.plural.trim()) && <span className="field-error" role="alert">Give each renamed term both a singular and a plural.</span>}
        {Object.keys(value).length > 0 && <Button variant="secondary" className="btn-sm" onPress={() => onChange({})}>Clear all</Button>}
        <span className="setting-note">Empty fields use the product term. Up to {maxLen} characters.</span>
      </div>
    </div>
  );
}
