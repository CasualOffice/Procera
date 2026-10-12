import { BriefcaseBusiness, KeyRound, Lock, UserPlus } from 'lucide-react';
import { useMemo, useState } from 'react';
import { useItems, useResource } from '../lib/data';
import { type Catalog, type Grant, type Role } from '../lib/roles';
import { navigate, useRoute } from '../lib/router';
import { useSession } from '../lib/session';
import { useTerms } from '../lib/terms';
import { Button } from '../ui/components/Button';
import { DataGrid } from '../ui/components/DataGrid';
import { Notice } from '../ui/components/Notice';
import { Tabs } from '../ui/components/Tabs';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import '../ui/admin.css';
import { PresetDialog } from './rolePresets';
import { RoleDrawer } from './roleDrawer';
import { GrantDialog, RevokeDialog, expiryLabel, granteeLabel, holders, isLive, scopeLabel, useAccessDirectory } from './roleGrants';

interface RoleRow { id: string; role: Role; type: string; holders: string; count: number }
interface GrantRow { id: string; grant: Grant; role: string; who: string; scope: string; expires: string | null; live: boolean }

/** Admin · Roles & access (docs/05 §3.15): built-in and custom roles, their permissions, and who holds them where. */
export function Roles() {
  useTerms(); // the workspace's terms (CFG-008)
  const { can } = useSession();
  const { query } = useRoute();
  const [roles, reloadRoles, rolesError] = useItems<Role>('/api/v1/roles');
  const [grants, reloadGrants, grantsError] = useItems<Grant>('/api/v1/role-grants');
  const [catalog] = useResource<Catalog>('/api/v1/permission-catalog');
  const dir = useAccessDirectory();
  const [openId, setOpenId] = useState<string | null>(query.get('role'));
  const [granting, setGranting] = useState<{ roleKey?: string } | null>(null);
  const [revoking, setRevoking] = useState<Grant | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [presets, setPresets] = useState(false);
  const canGrant = can('member:manage_roles');
  const open = roles?.find((r) => r.id === openId) ?? null;
  const roleName = (key: string) => roles?.find((r) => r.key === key)?.name ?? key;

  const roleRows = useMemo<RoleRow[]>(() => (roles ?? []).map((r) => {
    const h = holders(r.key, grants ?? []);
    return { id: r.id, role: r, type: r.is_builtin ? 'Built-in' : 'Custom', holders: h.text, count: h.people + h.groups };
  }), [roles, grants]);
  const grantRows = useMemo<GrantRow[]>(() => (grants ?? []).map((g) => ({
    id: g.id, grant: g, role: roleName(g.role_key), who: granteeLabel(g, dir), scope: scopeLabel(g.scope, dir), expires: g.expires_at, live: isLive(g),
  // eslint-disable-next-line react-hooks/exhaustive-deps
  })), [grants, roles, dir]);

  const openRole = (id: string | null) => { setOpenId(id); if (!id && query.get('role')) navigate('/admin/roles', { replace: true }); };
  const changed = () => { reloadRoles(); reloadGrants(); };
  const loading = roles === null || grants === null;
  const error = rolesError ?? grantsError;

  const rolesTab = (
    <DataGrid<RoleRow> caption="Roles" showCaption={false} data={roleRows} initialSort={[]}
      columns={[
        { id: 'name', header: 'Role', value: (r) => r.role.name, cell: (r) => (
          <span className="cell-entity">
            <span className={`doc-icon tag-${r.role.is_builtin ? 'gray' : 'purple'}`}>{r.role.is_builtin ? <Lock aria-hidden="true" size="0.875rem" /> : <KeyRound aria-hidden="true" size="0.875rem" />}</span>
            <span className="cell-stack"><button type="button" className="cell-link" onClick={() => openRole(r.id)}>{r.role.name}</button>{r.role.description && <span className="cell-sub">{r.role.description}</span>}</span>
          </span>
        ) },
        { id: 'type', header: 'Type', type: 'enum', value: (r) => r.type, cell: (r) => <Tag color={r.role.is_builtin ? 'gray' : 'purple'}>{r.type}</Tag> },
        { id: 'permissions', header: 'Permissions', type: 'number', value: (r) => r.role.permissions.length, cell: (r) => <span className="cell-muted">{r.role.permissions.length}</span> },
        { id: 'holders', header: 'Held by', type: 'number', value: (r) => r.count, cell: (r) => <span className="cell-muted">{r.holders}</span> },
      ]} />
  );
  const grantsTab = (
    <DataGrid<GrantRow> caption="Grants" showCaption={false} data={grantRows} initialSort={[{ id: 'role', desc: false }]}
      empty={<Empty art="roles" title="No grants yet" action={canGrant ? <Button onPress={() => setGranting({})}><UserPlus aria-hidden="true" size="1rem" /> Grant a role</Button> : undefined}>Grant a role to a person or a group, for the whole workspace, one collection or one department.</Empty>}
      columns={[
        { id: 'role', header: 'Role', type: 'enum', value: (g) => g.role },
        { id: 'who', header: 'Granted to', value: (g) => g.who },
        { id: 'scope', header: 'Where', value: (g) => g.scope },
        { id: 'expires', header: 'Expires', type: 'date', value: (g) => g.expires ?? '', cell: (g) => <span className={g.live ? 'cell-muted' : ''}>{g.live ? expiryLabel(g.grant) : <Tag color="gray">{expiryLabel(g.grant)}</Tag>}</span> },
        { id: 'reason', header: 'Reason', optional: true, value: (g) => g.grant.reason ?? '' },
        ...(canGrant ? [{ id: 'actions', header: 'Actions', sortable: false, value: () => '', cell: (g: GrantRow) => <Button variant="ghost" onPress={() => setRevoking(g.grant)} aria-label={`Revoke ${g.role} from ${g.who}`}>Revoke</Button> }] : []),
      ]} />
  );

  return (
    <>
      <div className="page-head">
        <div><h1>Roles &amp; access</h1><p>{roles ? `${roles.filter((r) => r.is_builtin).length} built-in roles · ${roles.filter((r) => !r.is_builtin).length} custom · ${(grants ?? []).filter((g) => isLive(g)).length} active grants` : 'What each role allows, and who holds it where.'}</p></div>
        {(canGrant || can('role:manage')) && (
          <div className="actions">
            {can('role:manage') && <Button variant="secondary" onPress={() => setPresets(true)}><BriefcaseBusiness aria-hidden="true" size="1rem" /> Add a role for a job</Button>}
            {canGrant && <Button onPress={() => setGranting({})}><UserPlus aria-hidden="true" size="1rem" /> Grant a role</Button>}
          </div>
        )}
      </div>
      {notice && <Notice tone="success" title={notice} />}
      {error ? <LoadError error={error} onRetry={changed} what="roles" /> : loading ? <Skeleton variant="table" rows={8} label="Loading roles" /> : (
        <Tabs label="Roles and grants" tabs={[{ id: 'roles', label: 'Roles', content: rolesTab }, { id: 'grants', label: 'Grants', content: grantsTab }]} />
      )}
      {open && roles && grants && (
        <RoleDrawer role={open} roles={roles} grants={grants} catalog={catalog} dir={dir} onClose={() => openRole(null)} onChanged={changed}
          onCreated={(r) => { setNotice(`Created ${r.name}.`); changed(); setOpenId(r.id); }}
          onGrant={() => setGranting({ roleKey: open.key })} onRevoke={setRevoking} />
      )}
      <GrantDialog isOpen={granting !== null} roles={roles ?? []} roleKey={granting?.roleKey} dir={dir} onClose={() => setGranting(null)}
        onGranted={() => { setNotice('Role granted.'); reloadGrants(); }} />
      <RevokeDialog grant={revoking} roles={roles ?? []} dir={dir} onClose={() => setRevoking(null)} onRevoked={() => { setNotice('Grant revoked.'); reloadGrants(); }} />
      <PresetDialog isOpen={presets} roles={roles ?? []} onClose={() => setPresets(false)}
        onCreated={(r) => { setPresets(false); changed(); setNotice(`“${r.name}” is ready. Grant it to the people who do that job.`); if (canGrant) setGranting({ roleKey: r.key }); }} />
    </>
  );
}
