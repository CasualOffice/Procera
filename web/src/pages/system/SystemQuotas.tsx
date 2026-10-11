import { useState } from 'react';
import { Form } from 'react-aria-components';
import { api } from '../../lib/api';
import { toFailure, useItems } from '../../lib/data';
import { type ConsoleWorkspace, type QuotaDefaults, STATE, bytes, profileLabel, seats, sysCan } from '../../lib/system';
import { Button } from '../../ui/components/Button';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { TextField } from '../../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../../ui/States';
import { useStepUp } from '../../ui/StepUp';
import { useSystem } from '../../ui/SystemShell';
import { Tag } from '../../ui/Tag';
import { SeatMeter } from './SystemWorkspaces';

const GB = 1024 ** 3;
const num = (n: number | null) => (n === null ? 'Unlimited' : n.toLocaleString());

/**
 * System · Quotas & usage (docs/05 §3.20, FR-S04): workspaces ranked by how close they are to their
 * member limit, and the default limits each configuration profile starts with.
 */
export function SystemQuotas() {
  const me = useSystem();
  const [items, reload, error] = useItems<ConsoleWorkspace>('/sysapi/v1/workspaces');
  const [defaults, reloadDefaults, defaultsError] = useItems<QuotaDefaults>('/sysapi/v1/quota-defaults');
  const [editing, setEditing] = useState<QuotaDefaults | null>(null);
  const [saved, setSaved] = useState<string | null>(null);
  const ranked = [...(items ?? [])].sort((a, b) => (seats(b).ratio ?? -1) - (seats(a).ratio ?? -1));
  const canConfig = sysCan(me.roles, 'platform.config');

  return (
    <>
      <div className="page-head"><div><h1>Quotas &amp; usage</h1><p>Member limits are enforced when someone is invited. Seats count members and open invitations.</p></div></div>
      {saved && <Notice tone="success" title={saved} />}
      {error ? <LoadError error={error} onRetry={reload} what="usage" /> : items === null ? <Skeleton variant="table" rows={5} label="Loading usage" /> : ranked.length === 0 ? (
        <Empty art="generic" title="No workspaces yet">Usage appears once a workspace exists.</Empty>
      ) : (
        <div className="table-wrap">
          <table className="table" aria-label="Usage by workspace">
            <thead><tr><th>Workspace</th><th>Seats</th><th className="col-optional">Limit</th><th className="col-optional">Storage limit</th><th>State</th></tr></thead>
            <tbody>
              {ranked.map((w) => {
                const r = seats(w).ratio;
                return (
                  <tr key={w.id}>
                    <td><span className="ws-row-btn"><strong>{w.name}</strong><span>{w.slug}</span></span></td>
                    <td><SeatMeter w={w} />{r !== null && r >= 1 ? <> <Tag color="red">At limit</Tag></> : r !== null && r >= 0.8 ? <> <Tag color="yellow">Near limit</Tag></> : null}</td>
                    <td className="cell-muted col-optional">{num(w.quotas.max_members)}</td>
                    <td className="cell-muted col-optional">{bytes(w.quotas.max_storage_bytes)}</td>
                    <td><Tag color={STATE[w.state]?.color} dot>{STATE[w.state]?.label ?? w.state}</Tag></td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      <section className="quota-section" aria-labelledby="defaults-h">
        <h2 id="defaults-h">Defaults by profile</h2>
        <p>A new workspace starts with its profile’s limits. Changing a default never changes existing workspaces.</p>
        {defaultsError ? <LoadError error={defaultsError} onRetry={reloadDefaults} what="quota defaults" /> : defaults === null ? <Skeleton rows={4} label="Loading defaults" /> : (
          <div className="table-wrap">
            <table className="table" aria-label="Quota defaults">
              <thead><tr><th>Profile</th><th>Members</th><th>Storage</th><th className="col-optional">API requests per minute</th>{canConfig && <th><span className="visually-hidden">Actions</span></th>}</tr></thead>
              <tbody>
                {defaults.map((d) => (
                  <tr key={d.profile}>
                    <td>{profileLabel(d.profile)}</td>
                    <td className="cell-muted">{num(d.max_members)}</td>
                    <td className="cell-muted">{bytes(d.max_storage_bytes)}</td>
                    <td className="cell-muted col-optional">{num(d.api_rpm)}</td>
                    {canConfig && <td className="num"><Button variant="ghost" onPress={() => { setSaved(null); setEditing(d); }} aria-label={`Edit ${profileLabel(d.profile)} defaults`}>Edit</Button></td>}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
      {editing && <DefaultsDialog d={editing} onClose={() => setEditing(null)} onSaved={() => { setSaved(`${profileLabel(editing.profile)} defaults saved`); setEditing(null); reloadDefaults(); }} />}
    </>
  );
}

function DefaultsDialog({ d, onClose, onSaved }: { d: QuotaDefaults; onClose: () => void; onSaved: () => void }) {
  const [members, setMembers] = useState(d.max_members?.toString() ?? '');
  const [storage, setStorage] = useState(d.max_storage_bytes ? String(d.max_storage_bytes / GB) : '');
  const [error, setError] = useState<string | null>(null);
  const { guard, dialog } = useStepUp((e) => setError(toFailure(e).message));
  const parse = (v: string, scale = 1): number | null | 'bad' => {
    if (!v.trim()) return null;
    const n = Number(v);
    return Number.isFinite(n) && n > 0 ? Math.round(n * scale) : 'bad';
  };
  const m = parse(members);
  const st = parse(storage, GB);
  const valid = m !== 'bad' && st !== 'bad';
  const submit = () => {
    if (!valid) return;
    setError(null);
    void guard(async () => {
      await api(`/sysapi/v1/quota-defaults/${d.profile}`, {
        method: 'PUT', headers: { 'if-match': `"${d.revision}"` },
        body: { max_members: m, max_active_instances: d.max_active_instances, max_storage_bytes: st, api_rpm: d.api_rpm, ai_tokens_month: d.ai_tokens_month },
      });
      onSaved();
    });
  };
  return (
    <>
      <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title={`${profileLabel(d.profile)} defaults`} width={460}
        description="Applies to workspaces created from now on. Leave a field empty for unlimited."
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={submit} isDisabled={!valid}>Save defaults</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        <Form className="quota-form" onSubmit={(e) => { e.preventDefault(); submit(); }}>
          <TextField label="Member limit" value={members} onChange={setMembers} inputMode="numeric" autoFocus errorMessage={m === 'bad' ? 'Enter a whole number above 0.' : undefined} isInvalid={m === 'bad'} />
          <TextField label="Storage limit in GB" value={storage} onChange={setStorage} inputMode="decimal" isInvalid={st === 'bad'} errorMessage={st === 'bad' ? 'Enter a number above 0.' : undefined} />
        </Form>
      </FormDialog>
      {dialog}
    </>
  );
}
