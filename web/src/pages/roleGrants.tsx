import { useEffect, useMemo, useState } from 'react';
import { api } from '../lib/api';
import { type Member, useItems } from '../lib/data';
import { shortDate } from '../lib/format';
import { type Grant, type Role, type Scope, accessErrorText, people } from '../lib/roles';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { RadioCards } from '../ui/components/RadioCards';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';
import { useStepUp } from '../ui/StepUp';

interface Named { id: string; name: string; path?: string[]; member_count?: number }

/** Names for grant targets and scopes, from the directory APIs the caller may read. */
export interface AccessDirectory {
  members: Member[]; groups: Named[]; departments: Named[]; collections: Named[];
  member: (id: string) => Member | undefined; group: (id: string) => Named | undefined;
  department: (id: string) => string | undefined; collection: (id: string) => string | undefined;
}

/** "Policies / HR": a tree item named by its path (both trees carry `path`, root first). */
function pathNames(items: Named[]): Map<string, string> {
  const byId = new Map(items.map((i) => [i.id, i.name]));
  return new Map(items.map((i) => [i.id, (i.path?.length ? i.path : [i.id]).map((p) => byId.get(p) ?? '…').join(' / ')]));
}

export function useAccessDirectory(): AccessDirectory {
  const { can } = useSession();
  const [members] = useItems<Member>(can('member:view') ? '/api/v1/members' : null);
  const [groups] = useItems<Named>(can('directory:view') ? '/api/v1/groups' : null);
  const [departments] = useItems<Named>(can('directory:view') ? '/api/v1/departments' : null);
  const [collections] = useItems<Named>(can('collection:view') ? '/api/v1/collections' : null);
  return useMemo(() => {
    const m = new Map((members ?? []).map((x) => [x.id, x]));
    const g = new Map((groups ?? []).map((x) => [x.id, x]));
    const d = pathNames(departments ?? []);
    const c = pathNames(collections ?? []);
    return {
      members: members ?? [], groups: groups ?? [], departments: departments ?? [], collections: collections ?? [],
      member: (id) => m.get(id), group: (id) => g.get(id), department: (id) => d.get(id), collection: (id) => c.get(id),
    };
  }, [members, groups, departments, collections]);
}

export function scopeLabel(s: Scope, dir: AccessDirectory): string {
  if (s.kind === 'workspace') return 'Whole workspace';
  if (s.kind === 'collection') return `Collection: ${dir.collection(s.id) ?? 'unknown'}`;
  if (s.kind === 'department') return `Department: ${dir.department(s.id) ?? 'unknown'}`;
  return `${s.kind.charAt(0).toUpperCase()}${s.kind.slice(1)}: ${s.id.slice(0, 8)}`;
}

export function granteeLabel(g: Grant, dir: AccessDirectory): string {
  if (g.membership_id) return dir.member(g.membership_id)?.display_name ?? 'Unknown person';
  const group = g.group_id ? dir.group(g.group_id) : undefined;
  return group ? `${group.name} (group)` : 'Unknown group';
}

export const isLive = (g: Grant, now = Date.now()) => !g.expires_at || Date.parse(g.expires_at) > now;
export const expiryLabel = (g: Grant) => (!g.expires_at ? 'No expiry' : isLive(g) ? `Until ${shortDate(g.expires_at)}` : `Expired ${shortDate(g.expires_at)}`);

/** Who holds a role through live grants: distinct people granted directly, plus groups. */
export function holders(roleKey: string, grants: Grant[]): { people: number; groups: number; text: string } {
  const live = grants.filter((g) => g.role_key === roleKey && isLive(g));
  const p = new Set(live.flatMap((g) => (g.membership_id ? [g.membership_id] : []))).size;
  const gr = new Set(live.flatMap((g) => (g.group_id ? [g.group_id] : []))).size;
  const text = !p && !gr ? 'Nobody' : [p ? people(p) : '', gr ? `${gr} ${gr === 1 ? 'group' : 'groups'}` : ''].filter(Boolean).join(', ');
  return { people: p, groups: gr, text };
}

/** End of the chosen local day, so "Expires on 31 Oct" still works on 31 Oct. */
const endOfDay = (d: string) => new Date(`${d}T23:59:59`).toISOString();
const today = () => { const t = new Date(); return `${t.getFullYear()}-${String(t.getMonth() + 1).padStart(2, '0')}-${String(t.getDate()).padStart(2, '0')}`; };

/** Grant a role to a person or a group, workspace-wide or for one collection or department, optionally time-boxed. */
export function GrantDialog({ isOpen, roles, roleKey, dir, onClose, onGranted }: { isOpen: boolean; roles: Role[]; roleKey?: string; dir: AccessDirectory; onClose: () => void; onGranted: () => void }) {
  const [role, setRole] = useState(roleKey ?? '');
  const [target, setTarget] = useState<'member' | 'group'>('member');
  const [who, setWho] = useState('');
  const [scope, setScope] = useState<'workspace' | 'collection' | 'department'>('workspace');
  const [scopeId, setScopeId] = useState('');
  const [expires, setExpires] = useState('');
  const [reason, setReason] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (!isOpen) return;
    setRole(roleKey ?? ''); setTarget('member'); setWho(''); setScope('workspace'); setScopeId(''); setExpires(''); setReason(''); setError(null);
  }, [isOpen, roleKey]);
  const { guard, dialog } = useStepUp((e) => setError(accessErrorText(e)));
  const roleOptions = roles.filter((r) => target === 'member' || r.key !== 'workspace.owner').map((r) => ({ id: r.key, label: r.name }));
  const whoOptions = target === 'member'
    ? dir.members.filter((m) => m.status === 'active').map((m) => ({ id: m.id, label: `${m.display_name} (${m.email})` }))
    : dir.groups.map((g) => ({ id: g.id, label: g.member_count != null ? `${g.name} (${people(g.member_count)})` : g.name }));
  const scopeOptions = (scope === 'collection' ? dir.collections : dir.departments).map((x) => ({ id: x.id, label: (scope === 'collection' ? dir.collection(x.id) : dir.department(x.id)) ?? x.name }));
  // A Team Manager answers for one department, never the whole workspace (docs/12 §4).
  const deptOnly = role === 'team.manager';
  useEffect(() => { if (deptOnly && scope !== 'department') { setScope('department'); setScopeId(''); } }, [deptOnly, scope]);
  const pastExpiry = !!expires && expires < today();
  const ready = role && who && (scope === 'workspace' || scopeId) && !pastExpiry && !busy;

  const submit = () => guard(async () => {
    setError(null); setBusy(true);
    try {
      await api('/api/v1/role-grants', {
        method: 'POST',
        body: {
          ...(target === 'member' ? { membership_id: who } : { group_id: who }), role_key: role, scope_kind: scope,
          ...(scope === 'workspace' ? {} : { scope_id: scopeId }), ...(expires ? { expires_at: endOfDay(expires) } : {}), ...(reason.trim() ? { reason: reason.trim() } : {}),
        },
      });
      onGranted(); onClose();
    } finally { setBusy(false); }
  });

  return (
    <>
      <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} title="Grant a role" width={560}
        description="Choose who gets the role and where it applies. The change takes effect at once and is recorded in the audit log."
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void submit()} isDisabled={!ready}>Grant role</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        <div className="stack">
          <Select label="Role" options={roleOptions} selectedKey={role || null} onSelectionChange={(k) => setRole(String(k ?? ''))} placeholder="Choose a role" />
          <RadioCards label="Grant to" value={target} onChange={(v) => { setTarget(v as 'member' | 'group'); setWho(''); }}
            options={[{ id: 'member', label: 'A person', description: 'One member of the workspace.' }, { id: 'group', label: 'A group', description: 'Everyone in the group, including people who join it later.' }]} />
          <Select label={target === 'member' ? 'Person' : 'Group'} options={whoOptions} selectedKey={who || null} onSelectionChange={(k) => setWho(String(k ?? ''))}
            placeholder={target === 'member' ? 'Choose a person' : 'Choose a group'} description={!whoOptions.length ? (target === 'member' ? 'No members to choose from.' : 'No groups yet. Create them in the directory.') : undefined} />
          <RadioCards label="Where it applies" value={scope} onChange={(v) => { setScope(v as typeof scope); setScopeId(''); }}
            options={deptOnly
              ? [{ id: 'department', label: 'A department', description: 'A Team Manager answers for one department and the teams under it, never the whole workspace.' }]
              : [
                { id: 'workspace', label: 'Whole workspace', description: 'Everything in the workspace.' },
                { id: 'collection', label: 'A collection', description: 'One library collection and the collections inside it.' },
                { id: 'department', label: 'A department', description: 'One department and the teams under it.' },
              ]} />
          {scope !== 'workspace' && (
            <Select label={scope === 'collection' ? 'Collection' : 'Department'} options={scopeOptions} selectedKey={scopeId || null} onSelectionChange={(k) => setScopeId(String(k ?? ''))}
              placeholder={scope === 'collection' ? 'Choose a collection' : 'Choose a department'} description={!scopeOptions.length ? `No ${scope === 'collection' ? 'collections' : 'departments'} to choose from.` : undefined} />
          )}
          <TextField label="Expires on" type="date" value={expires} onChange={setExpires}
            description="Optional. Access ends at the end of this day. Leave it empty for access that doesn’t expire."
            isInvalid={pastExpiry} errorMessage="Choose today or a later date." />
          <TextField label="Reason" value={reason} onChange={setReason} description="Optional. Shown in the audit log, for example “External audit, Q4”." />
        </div>
      </FormDialog>
      {dialog}
    </>
  );
}

/** Confirms revoking one grant, saying what the person or group loses. */
export function RevokeDialog({ grant, roles, dir, onClose, onRevoked }: { grant: Grant | null; roles: Role[]; dir: AccessDirectory; onClose: () => void; onRevoked: () => void }) {
  const [error, setError] = useState<string | null>(null);
  useEffect(() => setError(null), [grant]);
  const { guard, dialog } = useStepUp((e) => setError(accessErrorText(e)));
  const role = roles.find((r) => r.key === grant?.role_key)?.name ?? grant?.role_key ?? '';
  const who = grant ? granteeLabel(grant, dir) : '';
  const revoke = () => guard(async () => {
    if (!grant) return;
    await api(`/api/v1/role-grants/${grant.id}`, { method: 'DELETE' });
    onRevoked(); onClose();
  });
  return (
    <>
      <FormDialog isOpen={grant !== null} onOpenChange={(o) => !o && onClose()} title={`Revoke ${role} from ${who}?`}
        description={grant ? `They lose this role’s permissions at once (${scopeLabel(grant.scope, dir)}). Permissions they hold through other roles stay.` : undefined}
        footer={<><Button variant="secondary" onPress={onClose}>Keep</Button><Button variant="danger" onPress={() => void revoke()}>Revoke</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        <p className="muted" style={{ margin: 0 }}>This is recorded in the audit log.</p>
      </FormDialog>
      {dialog}
    </>
  );
}
