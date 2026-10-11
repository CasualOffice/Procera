import { Lock, PauseCircle, PlayCircle, Plus, Trash2 } from 'lucide-react';
import { useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { api } from '../../lib/api';
import { useItems, useResource, toFailure } from '../../lib/data';
import { shortDate, timeAgo } from '../../lib/format';
import { type ConsoleWorkspace, PROFILES, type QuotaDefaults, STATE, bytes, profileLabel, seats, sysCan } from '../../lib/system';
import { Button } from '../../ui/components/Button';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { TextField } from '../../ui/components/TextField';
import { Drawer } from '../../ui/Drawer';
import { Empty, LoadError, Skeleton } from '../../ui/States';
import { useStepUp } from '../../ui/StepUp';
import { useSystem } from '../../ui/SystemShell';
import { Tag } from '../../ui/Tag';
import { actionText, type AuditEvent } from '../Audit';
import { type Created, NewWorkspaceDialog } from './NewWorkspaceDialog';
import '../../ui/admin.css';

const CHIPS: [string, string][] = [['', 'All'], ['active', 'Active'], ['provisioning', 'Provisioning'], ['suspended_ro', 'Read-only'], ['suspended_locked', 'Locked'], ['pending_deletion', 'Deletion scheduled']];

/** Seats used against the member limit, as a small meter with the numbers in text. */
export function SeatMeter({ w }: { w: ConsoleWorkspace }) {
  const { used, limit, ratio } = seats(w);
  if (limit === null) return <span className="meter"><span className="meter-num">{used} of unlimited</span></span>;
  const pct = Math.min(100, Math.round((ratio ?? 0) * 100));
  return (
    <span className={`meter${pct >= 100 ? ' meter-full' : pct >= 80 ? ' meter-warn' : ''}`}>
      <span className="meter-num">{used} of {limit.toLocaleString()}</span>
      <span className="meter-bar" aria-hidden="true"><span style={{ width: `${pct}%` }} /></span>
    </span>
  );
}

/**
 * System · Workspaces (docs/05 §3.20): every workspace with lifecycle state, profile, members, quota
 * usage and activity. A row opens its detail with lifecycle actions; no workspace content is shown.
 */
export function SystemWorkspaces() {
  const me = useSystem();
  const [items, reload, error] = useItems<ConsoleWorkspace>('/sysapi/v1/workspaces');
  const [state, setState] = useState('');
  const [profile, setProfile] = useState('');
  const [q, setQ] = useState('');
  const [openId, setOpenId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [created, setCreated] = useState<Created | null>(null);
  const canWrite = sysCan(me.roles, 'workspace.write');
  const counts = useMemo(() => {
    const c: Record<string, number> = { '': items?.length ?? 0 };
    for (const w of items ?? []) c[w.state] = (c[w.state] ?? 0) + 1;
    return c;
  }, [items]);
  const rows = (items ?? []).filter((w) => (!state || w.state === state) && (!profile || w.profile === profile)
    && (!q.trim() || `${w.name} ${w.slug}`.toLowerCase().includes(q.trim().toLowerCase())));
  const open = items?.find((w) => w.id === openId) ?? null;
  const filtered = Boolean(state || profile || q.trim());

  return (
    <>
      <div className="page-head">
        <div><h1>Workspaces</h1><p>Every workspace on this platform: lifecycle, size and activity. Content stays inside each workspace.</p></div>
        {canWrite && <div className="page-actions"><Button onPress={() => setCreating(true)}><Plus aria-hidden="true" size="1rem" /> New workspace</Button></div>}
      </div>
      {created && (
        <div className="sys-created">
          <Notice tone="success" title={`${created.name} was created`}>We emailed {created.ownerEmail} an invitation to own it. The workspace shows as Provisioning until they accept.</Notice>
        </div>
      )}
      <div className="sys-toolbar">
        <div className="filter-chips" role="group" aria-label="State">
          {CHIPS.map(([key, label]) => (
            <button key={key} type="button" className="filter-chip" aria-pressed={state === key} onClick={() => setState(key)}>
              {label}<span className="filter-chip-count">{counts[key] ?? 0}</span>
            </button>
          ))}
        </div>
        <label className="visually-hidden" htmlFor="ws-profile">Profile</label>
        <select id="ws-profile" value={profile} onChange={(e) => setProfile(e.target.value)}>
          <option value="">All profiles</option>
          {PROFILES.map((p) => <option key={p} value={p}>{profileLabel(p)}</option>)}
        </select>
        <label className="sys-search"><span className="visually-hidden">Search workspaces</span>
          <input type="search" placeholder="Search by name or slug" value={q} onChange={(e) => setQ(e.target.value)} />
        </label>
      </div>

      {error ? <LoadError error={error} onRetry={reload} what="workspaces" /> : items === null ? <Skeleton variant="table" rows={6} label="Loading workspaces" /> : rows.length === 0 ? (
        filtered ? <Empty art="no-match" title="No workspaces match" action={<Button variant="secondary" onPress={() => { setState(''); setProfile(''); setQ(''); }}>Clear filters</Button>} />
          : <Empty art="generic" title="No workspaces yet" action={canWrite ? <Button onPress={() => setCreating(true)}><Plus aria-hidden="true" size="1rem" /> New workspace</Button> : undefined}>Create one for each organisation, entity or department. Its owner is invited by email.</Empty>
      ) : (
        <div className="table-wrap">
          <table className="table" aria-label="Workspaces">
            <thead><tr><th>Workspace</th><th>State</th><th className="col-optional">Profile</th><th>Members</th><th className="col-optional">Storage</th><th className="col-optional">Last activity</th><th className="col-optional">Created</th></tr></thead>
            <tbody>
              {rows.map((w) => (
                <tr key={w.id}>
                  <td><button type="button" className="ws-row-btn" onClick={() => setOpenId(w.id)} aria-label={`${w.name}, open details`}><strong>{w.name}</strong><span>{w.slug}</span></button></td>
                  <td><Tag color={STATE[w.state]?.color} dot>{STATE[w.state]?.label ?? w.state}</Tag></td>
                  <td className="cell-muted col-optional">{profileLabel(w.profile)}</td>
                  <td><SeatMeter w={w} /></td>
                  <td className="cell-muted col-optional">{w.usage.storage_bytes === null ? 'Not tracked yet' : bytes(w.usage.storage_bytes)}</td>
                  <td className="cell-muted col-optional">{w.usage.last_activity_at ? timeAgo(w.usage.last_activity_at) : 'None'}</td>
                  <td className="cell-muted col-optional">{shortDate(w.created_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {creating && <NewWorkspaceDialog onClose={() => setCreating(false)} onCreated={(c) => { setCreating(false); setCreated(c); setState(''); setProfile(''); setQ(''); reload(); }} />}
      {open && <WorkspaceDrawer w={open} canWrite={canWrite} canAudit={sysCan(me.roles, 'audit.read')} onChanged={reload} onClose={() => setOpenId(null)} />}
    </>
  );
}

type Action = 'read_only' | 'locked' | 'restore' | 'delete' | 'quotas';

function WorkspaceDrawer({ w, canWrite, canAudit, onChanged, onClose }: { w: ConsoleWorkspace; canWrite: boolean; canAudit: boolean; onChanged: () => void; onClose: () => void }) {
  const [action, setAction] = useState<Action | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const [history, reloadHistory] = useResource<{ items: AuditEvent[] }>(canAudit ? `/sysapi/v1/system-audit/events?object_type=workspace&object_id=${w.id}&page[size]=50` : null);
  const suspended = w.state === 'suspended_ro' || w.state === 'suspended_locked';
  const live = w.state === 'active' || w.state === 'provisioning' || suspended;
  const changed = (msg: string) => { setAction(null); setDone(msg); onChanged(); reloadHistory(); };

  return (
    <Drawer title={w.name} onClose={onClose}>
      {done && <Notice tone="success" title={done} />}
      {w.suspended_reason && <div className="ws-reason"><Notice tone={w.state === 'pending_deletion' ? 'warning' : 'info'} title={`${STATE[w.state]?.label ?? w.state}: ${w.suspended_reason}`}>{w.deletion_due_at ? `Data is deleted on ${shortDate(w.deletion_due_at)} unless the workspace is reactivated.` : null}</Notice></div>}
      <dl className="ws-facts">
        <dt>State</dt><dd><Tag color={STATE[w.state]?.color} dot>{STATE[w.state]?.label ?? w.state}</Tag></dd>
        <dt>Slug</dt><dd><code>{w.slug}</code></dd>
        <dt>Profile</dt><dd>{profileLabel(w.profile)}</dd>
        <dt>Region</dt><dd>{w.region} <span className="muted">· {w.isolation_tier.replace(/_/g, ' ')}</span></dd>
        <dt>Members</dt><dd><SeatMeter w={w} />{w.usage.invited > 0 && <span className="muted"> · {w.usage.members} joined, {w.usage.invited} invited</span>}</dd>
        <dt>Storage limit</dt><dd>{bytes(w.quotas.max_storage_bytes)} <span className="muted">· usage tracked once files ship</span></dd>
        <dt>Last activity</dt><dd>{w.usage.last_activity_at ? timeAgo(w.usage.last_activity_at) : 'None yet'}</dd>
        {w.owner_invite_email && <><dt>Owner invited</dt><dd>{w.owner_invite_email}</dd></>}
        <dt>Created</dt><dd>{shortDate(w.created_at)}</dd>
      </dl>

      {canWrite && (
        <>
          <h3 className="ws-section-h">Actions</h3>
          <div className="ws-actions">
            {(w.state === 'active' || w.state === 'provisioning' || w.state === 'suspended_locked') && <Button variant="secondary" onPress={() => setAction('read_only')}><PauseCircle aria-hidden="true" size="1rem" /> Make read-only</Button>}
            {(w.state === 'active' || w.state === 'provisioning' || w.state === 'suspended_ro') && <Button variant="secondary" onPress={() => setAction('locked')}><Lock aria-hidden="true" size="1rem" /> Lock</Button>}
            {(suspended || w.state === 'pending_deletion') && <Button variant="secondary" onPress={() => setAction('restore')}><PlayCircle aria-hidden="true" size="1rem" /> Reactivate</Button>}
            {live && <Button variant="secondary" onPress={() => setAction('quotas')}>Edit quotas</Button>}
            {live && <Button variant="danger" onPress={() => setAction('delete')}><Trash2 aria-hidden="true" size="1rem" /> Schedule deletion</Button>}
          </div>
        </>
      )}

      {canAudit && (
        <>
          <h3 className="ws-section-h">Lifecycle history</h3>
          {history === null ? <Skeleton rows={3} label="Loading history" /> : history.items.length === 0 ? <p className="muted">No lifecycle events.</p> : (
            <ol className="tl" aria-label="Lifecycle history">
              {[...history.items].reverse().map((e) => (
                <li key={e.seq}>
                  <span className="tl-dot" aria-hidden="true" />
                  <div>
                    <p className="tl-title">{actionText(e.action)}</p>
                    <p className="tl-meta">{e.actor_name ?? 'System'} · <time dateTime={e.occurred_at}>{shortDate(e.occurred_at)}</time></p>
                    {reasonOf(e) && <p className="tl-reason">{reasonOf(e)}</p>}
                  </div>
                </li>
              ))}
            </ol>
          )}
        </>
      )}
      {action && <ActionDialog w={w} action={action} onClose={() => setAction(null)} onDone={changed} />}
    </Drawer>
  );
}

function reasonOf(e: AuditEvent): string | null {
  const d = e.diff as { reason?: unknown } | null;
  return typeof d?.reason === 'string' ? d.reason : null;
}

const TITLES: Record<Action, string> = {
  read_only: 'Make this workspace read-only', locked: 'Lock this workspace', restore: 'Reactivate this workspace',
  delete: 'Schedule deletion', quotas: 'Edit quotas',
};
const GB = 1024 ** 3;

function ActionDialog({ w, action, onClose, onDone }: { w: ConsoleWorkspace; action: Action; onClose: () => void; onDone: (msg: string) => void }) {
  const [reason, setReason] = useState('');
  const [days, setDays] = useState('30');
  const [confirm, setConfirm] = useState('');
  const [members, setMembers] = useState(w.quotas.max_members?.toString() ?? '');
  const [storage, setStorage] = useState(w.quotas.max_storage_bytes ? String(w.quotas.max_storage_bytes / GB) : '');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [defaults] = useItems<QuotaDefaults>(action === 'quotas' ? '/sysapi/v1/quota-defaults' : null);
  const def = defaults?.find((d) => d.profile === w.profile);
  const { guard, dialog } = useStepUp((e) => setError(toFailure(e).message));
  const headers = { 'if-match': `"${w.revision}"` };

  const limit = (v: string, scale = 1): number | null | 'bad' => {
    if (!v.trim()) return null;
    const n = Number(v);
    return Number.isFinite(n) && n > 0 ? Math.round(n * scale) : 'bad';
  };
  const grace = Number(days);
  const valid = action === 'restore' ? true
    : action === 'quotas' ? limit(members) !== 'bad' && limit(storage, GB) !== 'bad'
      : action === 'delete' ? reason.trim().length > 0 && Number.isInteger(grace) && grace >= 7 && grace <= 90 && confirm.trim() === w.slug
        : reason.trim().length > 0;

  const submit = () => {
    setError(null);
    setBusy(true);
    void guard(async () => {
      if (action === 'read_only' || action === 'locked') {
        await api(`/sysapi/v1/workspaces/${w.id}/suspend`, { method: 'POST', headers, body: { mode: action, reason: reason.trim() } });
        onDone(action === 'locked' ? `${w.name} is locked` : `${w.name} is read-only`);
      } else if (action === 'restore') {
        await api(`/sysapi/v1/workspaces/${w.id}/restore`, { method: 'POST', headers });
        onDone(`${w.name} is active again`);
      } else if (action === 'delete') {
        await api(`/sysapi/v1/workspaces/${w.id}/schedule-deletion`, { method: 'POST', headers, body: { grace_days: grace, reason: reason.trim() } });
        onDone(`${w.name} is scheduled for deletion`);
      } else {
        await api(`/sysapi/v1/workspaces/${w.id}/quotas`, { method: 'PUT', body: { ...w.quotas, max_members: limit(members), max_storage_bytes: limit(storage, GB) } });
        onDone('Quotas saved');
      }
    }).finally(() => setBusy(false));
  };

  const verb = action === 'delete' ? 'Schedule deletion' : action === 'restore' ? 'Reactivate' : action === 'quotas' ? 'Save quotas' : action === 'locked' ? 'Lock workspace' : 'Make read-only';
  return (
    <>
      <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={TITLES[action]} width={480}
        description={action === 'read_only' ? 'Members can still sign in and read. Every change is refused until you reactivate it.'
          : action === 'locked' ? 'Nobody can open the workspace until you reactivate it. Members are told it is unavailable.'
            : action === 'restore' ? 'Members get full access again. The reactivation is recorded in the system audit log.'
              : action === 'delete' ? 'The workspace is locked now and its data is deleted when the grace period ends. You can reactivate it until then.'
                : `Limits for ${w.name}. Leave a field empty for unlimited.`}
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button variant={action === 'delete' ? 'danger' : 'primary'} onPress={submit} isDisabled={!valid || busy}>{busy ? 'Saving…' : verb}</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        <Form className="quota-form" onSubmit={(e) => { e.preventDefault(); if (valid && !busy) submit(); }}>
          {(action === 'read_only' || action === 'locked' || action === 'delete') && (
            <TextField label="Reason" value={reason} onChange={setReason} isRequired autoFocus description="Shown in the system audit log." />
          )}
          {action === 'delete' && (
            <>
              <TextField label="Grace period in days" value={days} onChange={setDays} inputMode="numeric" isRequired description="Between 7 and 90 days." />
              <TextField label={`Type ${w.slug} to confirm`} value={confirm} onChange={setConfirm} isRequired autoComplete="off" />
            </>
          )}
          {action === 'quotas' && (
            <>
              <TextField label="Member limit" value={members} onChange={setMembers} inputMode="numeric" autoFocus
                description={`Members plus open invitations. Default for ${profileLabel(w.profile)}: ${def ? (def.max_members?.toLocaleString() ?? 'unlimited') : '…'}.`} />
              <TextField label="Storage limit in GB" value={storage} onChange={setStorage} inputMode="decimal"
                description={`Default for ${profileLabel(w.profile)}: ${def ? bytes(def.max_storage_bytes) : '…'}.`} />
            </>
          )}
        </Form>
      </FormDialog>
      {dialog}
    </>
  );
}
