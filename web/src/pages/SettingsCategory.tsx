import { ChevronDown } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { useItems } from '../lib/data';
import type { Role } from '../lib/roles';
import { useSession } from '../lib/session';
import { LoadError, Skeleton } from '../ui/States';
import { type Setting, SettingRow, WorkspaceName } from './Settings';
import { DepartmentChip, useDepartmentScope } from './SettingsScopes';
import { anchorOf, categoryById, hiddenChoice, isHidden, placeOf } from './settingsMeta';

const slug = (s: string) => s.toLowerCase().replace(/[^a-z]+/g, '-');

/**
 * One Settings category (docs/05 §3.16, docs/design/ia-competitive-analysis.md §6.2): everyday
 * settings in sections, rare ones under a collapsed "Advanced". Settings a department may override
 * carry an Inherited / Overridden chip (§6.4).
 */
export function SettingsCategory({ id }: { id: string }) {
  const cat = categoryById(id)!;
  const { can } = useSession();
  const [items, reload, error] = useItems<Setting>('/api/v1/settings');
  const mine = useMemo(() => (items ?? []).filter((s) => !isHidden(s.key) && placeOf(s.key).cat === cat.id), [items, cat.id]);
  const advanced = mine.filter((s) => placeOf(s.key).advanced);
  const sections = cat.sections.map((name) => ({ name, items: mine.filter((s) => !placeOf(s.key).advanced && placeOf(s.key).section === name) })).filter((g) => g.items.length);
  const [showAdvanced, setShowAdvanced] = useState(false);

  // Two-step verification lists only roles that exist in this workspace (and not those of unshipped modules).
  const [roles] = useItems<Role>(mine.some((s) => s.key === 'auth.mfa.required_roles') ? '/api/v1/roles' : null);
  const hideFor = (key: string) => (choice: string) =>
    hiddenChoice(key, choice) || (key === 'auth.mfa.required_roles' && !!roles?.length && choice !== '*' && !roles.some((r) => r.key === choice));
  const scope = useDepartmentScope(mine.some((s) => s.department_scopable));
  const editable = (s: Setting) => !!s.editable_with && can(s.editable_with);

  // A search result or old link names a row (`#set-<key>`): open Advanced if it's there, then bring it into view.
  useEffect(() => {
    if (!items) return;
    const h = window.location.hash.slice(1);
    const hit = mine.find((s) => anchorOf(s.key) === h);
    if (!hit) return;
    if (placeOf(hit.key).advanced) setShowAdvanced(true);
    requestAnimationFrame(() => {
      const el = document.getElementById(h);
      el?.scrollIntoView({ block: 'center' });
      el?.classList.add('setting-found');
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items === null, cat.id]);

  const row = (s: Setting) => (
    <SettingRow key={s.key} s={s} canEdit={editable(s)} onSaved={reload} hide={s.kind.type === 'set' ? hideFor(s.key) : undefined}
      extra={s.department_scopable ? <DepartmentChip s={s} scope={scope} /> : undefined} />
  );

  return (
    <>
      <div className="page-head">
        <div><h1>{cat.label}</h1><p>{cat.about}. Changes apply to everyone and are recorded in the audit log.</p></div>
      </div>
      {cat.id === 'general' && <WorkspaceName />}
      {error ? <LoadError error={error} onRetry={reload} what="settings" /> : items === null ? <Skeleton variant="table" rows={6} label="Loading settings" /> : (
        <div className="settings-main">
          {sections.map((g) => (
            <section key={g.name} className="settings-group" aria-labelledby={`sec-${slug(g.name)}`}>
              <h2 id={`sec-${slug(g.name)}`}>{g.name}</h2>
              <ul className="settings-list">{g.items.map(row)}</ul>
            </section>
          ))}
          {advanced.length > 0 && (
            <section className="settings-group settings-advanced" aria-labelledby="sec-advanced">
              <h2 id="sec-advanced">
                <button type="button" className="settings-advanced-toggle" aria-expanded={showAdvanced} aria-controls="sec-advanced-list" onClick={() => setShowAdvanced(!showAdvanced)}>
                  <ChevronDown aria-hidden="true" size="1rem" />Advanced<span className="settings-advanced-count">{advanced.length} rarely changed</span>
                </button>
              </h2>
              <ul className="settings-list" id="sec-advanced-list" hidden={!showAdvanced}>{showAdvanced && advanced.map(row)}</ul>
            </section>
          )}
        </div>
      )}
    </>
  );
}
