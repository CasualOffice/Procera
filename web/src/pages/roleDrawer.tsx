import { Copy, Lock, Search, UserPlus } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { CheckboxGroup, Form, Label } from 'react-aria-components';
import { api } from '../lib/api';
import { type Catalog, type Grant, type Role, type RolePreview, accessErrorText, actionLabel, groupByArea, ownerOnly, permLabel, previewSentences, roleKey } from '../lib/roles';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { Drawer } from '../ui/Drawer';
import { Empty } from '../ui/States';
import { Tag } from '../ui/Tag';
import { useStepUp } from '../ui/StepUp';
import { type AccessDirectory, expiryLabel, granteeLabel, isLive, scopeLabel } from './roleGrants';

const same = (a: string[], b: string[]) => a.length === b.length && [...a].sort().join() === [...b].sort().join();

/**
 * One role (docs/05 §3.15): what it allows, grouped by area, and who holds it. Built-in roles are
 * read-only; custom roles are edited here, with the effect preview before saving.
 */
export function RoleDrawer({ role, roles, grants, catalog, dir, onClose, onChanged, onCreated, onGrant, onRevoke }: {
  role: Role; roles: Role[]; grants: Grant[]; catalog: Catalog | null; dir: AccessDirectory;
  onClose: () => void; onChanged: () => void; onCreated: (r: Role) => void; onGrant: () => void; onRevoke: (g: Grant) => void;
}) {
  const { can } = useSession();
  const canManage = can('role:manage');
  const lacking = role.permissions.filter((p) => !can(p));
  const editable = !role.is_builtin && canManage && lacking.length === 0;
  const locked = useMemo(() => ownerOnly(catalog), [catalog]);
  const [sel, setSel] = useState<string[]>(role.permissions);
  const [show, setShow] = useState<'all' | 'included'>(editable ? 'all' : 'included');
  const [q, setQ] = useState('');
  const [preview, setPreview] = useState<RolePreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [duplicating, setDuplicating] = useState(false);
  useEffect(() => { setSel(role.permissions); setPreview(null); setError(null); }, [role.id, role.revision, role.permissions]);
  useEffect(() => { setShow(editable ? 'all' : 'included'); setSaved(false); }, [role.id, editable]);
  const dirty = !same(sel, role.permissions);
  const { guard, dialog } = useStepUp((e) => { setPreview(null); setError(accessErrorText(e)); });

  const review = async () => {
    setError(null); setSaved(false);
    try { setPreview(await api<RolePreview>(`/api/v1/roles/${role.id}/preview`, { method: 'POST', body: { permissions: sel } })); }
    catch (e) { setError(accessErrorText(e)); }
  };
  const save = () => guard(async () => {
    await api(`/api/v1/roles/${role.id}`, { method: 'PATCH', body: { permissions: sel } });
    setPreview(null); setSaved(true); onChanged();
  });

  const why = (p: string) => (locked.has(p) ? 'Owners only' : !can(p) ? 'You don’t have this' : null);
  const needle = q.trim().toLowerCase();
  const groups = groupByArea(catalog?.permissions ?? role.permissions)
    .map((g) => ({ ...g, perms: g.perms.filter((p) => (show === 'all' || sel.includes(p)) && (!needle || p.includes(needle) || permLabel(p).toLowerCase().includes(needle))) }))
    .filter((g) => g.perms.length);
  const roleGrants = grants.filter((g) => g.role_key === role.key);
  const source = role.builtin_template && !role.is_builtin ? roles.find((r) => r.key === role.builtin_template) : undefined;

  return (
    <Drawer title={role.name} onClose={onClose} footer={<>
      {canManage && <Button variant="secondary" onPress={() => setDuplicating(true)}><Copy aria-hidden="true" size="0.875rem" /> Duplicate</Button>}
      {can('member:manage_roles') && <Button variant="secondary" onPress={onGrant}><UserPlus aria-hidden="true" size="0.875rem" /> Grant role</Button>}
      {editable && dirty && <Button variant="secondary" onPress={() => setSel(role.permissions)}>Discard changes</Button>}
      {editable && <Button onPress={() => void review()} isDisabled={!dirty}>Review changes</Button>}
    </>}>
      <div className="drawer-tags">
        {role.is_builtin ? <Tag color="gray"><Lock aria-hidden="true" size="0.75rem" /> Built-in</Tag> : <Tag color="purple">Custom</Tag>}
        <Tag color="blue">{sel.length} {sel.length === 1 ? 'permission' : 'permissions'}</Tag>
        <code className="mono">{role.key}</code>
      </div>
      {role.description && <p className="drawer-next">{role.description}</p>}
      {source && <p className="drawer-next">Based on {source.name}.</p>}
      {saved && <Notice tone="success" title="Role saved." />}
      {error && <Notice tone="danger" title={error} />}
      {role.is_builtin && <Notice tone="info" title="Built-in roles can’t be changed.">{canManage ? 'Duplicate it to make a custom role you can edit.' : 'Ask an admin to duplicate it if you need a variation.'}</Notice>}
      {!role.is_builtin && canManage && lacking.length > 0 && (
        <Notice tone="warning" title="You can’t edit this role.">It has permissions you don’t hold yourself: {lacking.map(permLabel).join(', ')}.</Notice>
      )}

      <section className="drawer-section" aria-labelledby="role-perms-h">
        <h3 className="drawer-h" id="role-perms-h">Permissions</h3>
        <div className="perm-tools">
          <div className="segmented" role="group" aria-label="Show permissions">
            <button type="button" aria-pressed={show === 'included'} onClick={() => setShow('included')}>Included <span className="seg-count">{sel.length}</span></button>
            <button type="button" aria-pressed={show === 'all'} onClick={() => setShow('all')}>All <span className="seg-count">{catalog?.permissions.length ?? ''}</span></button>
          </div>
          <label className="perm-search"><Search aria-hidden="true" size="0.875rem" /><span className="visually-hidden">Search permissions</span>
            <input type="search" value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search permissions" /></label>
        </div>
        {groups.length === 0 ? <Empty compact title={needle ? 'No permissions match' : 'This role has no permissions'} /> : groups.map((g) => {
          const allowed = g.perms.filter((p) => !why(p));
          const all = allowed.length > 0 && allowed.every((p) => sel.includes(p));
          return (
            <CheckboxGroup key={g.area} value={sel} onChange={setSel} isReadOnly={!editable} className="perm-group">
              <div className="perm-group-head">
                <Label className="perm-area">{g.label}</Label>
                {editable && allowed.length > 1 && (
                  <button type="button" className="link-btn perm-all" onClick={() => setSel(all ? sel.filter((p) => !allowed.includes(p)) : [...new Set([...sel, ...allowed])])}>
                    {all ? `Clear ${g.label}` : `Select all ${g.label}`}
                  </button>
                )}
              </div>
              {g.perms.map((p) => {
                const reason = editable ? why(p) : null;
                return (
                  <Checkbox key={p} value={p} isDisabled={!!reason && !sel.includes(p)}>
                    <span className="perm-label">{actionLabel(p)} <code className="perm-key">{p}</code>{reason && <span className="perm-why">{reason}</span>}</span>
                  </Checkbox>
                );
              })}
            </CheckboxGroup>
          );
        })}
      </section>

      <section className="drawer-section" aria-labelledby="role-holders-h">
        <h3 className="drawer-h" id="role-holders-h">Who has this role</h3>
        {roleGrants.length === 0 ? <p className="muted" style={{ margin: 0 }}>Nobody has this role yet.</p> : (
          <ul className="grant-list">
            {roleGrants.map((g) => (
              <li key={g.id} className="grant-item">
                <span className="grant-main"><strong>{granteeLabel(g, dir)}</strong><span className="muted">{scopeLabel(g.scope, dir)} · {expiryLabel(g)}</span></span>
                {!isLive(g) && <Tag color="gray">Expired</Tag>}
                {can('member:manage_roles') && <Button variant="ghost" onPress={() => onRevoke(g)} aria-label={`Revoke from ${granteeLabel(g, dir)}`}>Revoke</Button>}
              </li>
            ))}
          </ul>
        )}
      </section>

      <FormDialog isOpen={preview !== null} onOpenChange={(o) => !o && setPreview(null)} title={`Review changes to ${role.name}`}
        description="This is what saving does. The change is recorded in the audit log with the permissions added and removed."
        footer={<><Button variant="secondary" onPress={() => setPreview(null)}>Back</Button><Button onPress={() => void save()} isDisabled={!preview || (!preview.added.length && !preview.removed.length)}>Save changes</Button></>}>
        {preview && (
          <div className="stack" aria-live="polite">
            {previewSentences(preview).map((s) => <p key={s} className="preview-line">{s}</p>)}
            {preview.affected_members.length > 0 && (
              <details className="preview-people">
                <summary>Show the {preview.affected_members.length === 1 ? 'person' : `${preview.affected_members.length} people`}</summary>
                <ul>{preview.affected_members.map((id) => <li key={id}>{dir.member(id)?.display_name ?? 'Unknown person'}</li>)}</ul>
              </details>
            )}
          </div>
        )}
      </FormDialog>
      <DuplicateDialog isOpen={duplicating} source={role} locked={locked} onClose={() => setDuplicating(false)} onCreated={(r) => { setDuplicating(false); onCreated(r); }} />
      {dialog}
    </Drawer>
  );
}

/** Clone any role into a custom role (docs/12 §6 rule 1). Owner-only permissions are never copied. */
function DuplicateDialog({ isOpen, source, locked, onClose, onCreated }: { isOpen: boolean; source: Role; locked: Set<string>; onClose: () => void; onCreated: (r: Role) => void }) {
  const [name, setName] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (isOpen) { setName(`${source.name} copy`); setError(null); } }, [isOpen, source.name]);
  const dropped = source.permissions.filter((p) => locked.has(p));
  const key = roleKey(name);
  const { guard, dialog } = useStepUp((e) => setError(accessErrorText(e)));
  const submit = () => guard(async () => {
    setError(null); setBusy(true);
    try {
      const created = await api<Role>('/api/v1/roles', { method: 'POST', body: { key, name: name.trim(), permissions: source.permissions.filter((p) => !locked.has(p)), cloned_from: source.key } });
      onCreated(created);
    } finally { setBusy(false); }
  });
  return (
    <>
      <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} title={`Duplicate ${source.name}`}
        description="Creates a custom role with the same permissions. You can change its permissions afterwards."
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={name.trim().length < 2 || busy}>Duplicate role</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        {dropped.length > 0 && <Notice tone="info" title="Owner-only permissions aren’t copied.">{dropped.map(permLabel).join(', ')}</Notice>}
        <Form onSubmit={(e) => { e.preventDefault(); if (name.trim().length >= 2) void submit(); }}>
          <TextField label="Name" value={name} onChange={setName} autoFocus isRequired description={`Key: ${key}. The key is used in the API and the audit log and can’t be changed.`} />
        </Form>
      </FormDialog>
      {dialog}
    </>
  );
}
