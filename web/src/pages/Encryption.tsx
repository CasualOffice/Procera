import { KeyRound, RefreshCw } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type LoadFailure, toFailure } from '../lib/data';
import { shortDate } from '../lib/format';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { TextField } from '../ui/components/TextField';
import { StepUpDialog } from '../ui/StepUp';
import { LoadError, Skeleton } from '../ui/States';
import { Tag, type TagColor } from '../ui/Tag';

interface KeyVersion { version: number; state: 'active' | 'retired' | 'destroyed'; created_at: string; retired_at: string | null; values: number }
interface Status { algorithm: string; active_version: number; pending_rewrap: number; versions: KeyVersion[] }

const STATE: Record<KeyVersion['state'], [string, TagColor]> = { active: ['In use', 'green'], retired: ['Retired, still decrypting', 'yellow'], destroyed: ['Destroyed', 'gray'] };

/** Admin · Encryption (BPMS-030): the workspace's own key, its versions, and rotation. */
export function Encryption() {
  const { can } = useSession();
  const [data, setData] = useState<Status | null>(null);
  const [failed, setFailed] = useState<LoadFailure | null>(null);
  const [rotating, setRotating] = useState(false);
  const [done, setDone] = useState<number | null>(null);
  const load = () => { setFailed(null); api<Status>('/api/v1/security/encryption').then(setData).catch((e) => setFailed(toFailure(e))); };
  useEffect(load, []);
  if (failed) return <LoadError error={failed} onRetry={load} what="encryption" />;
  if (!data) return <Skeleton rows={3} label="Loading encryption" />;
  const active = data.versions.find((v) => v.version === data.active_version);

  return (
    <>
      <div className="page-head">
        <div><h1>Encryption</h1><p>Sensitive data in this workspace is encrypted with a key that belongs to this workspace alone.</p></div>
        {can('workspace:settings.manage') && <Button variant="secondary" onPress={() => setRotating(true)}><RefreshCw aria-hidden="true" size="1rem" /> Rotate key</Button>}
      </div>
      {done !== null && <Notice tone="success" title={`Key rotated. Version ${done} is now in use.`}>Existing data moves to the new key in the background. Nothing needs to be re-encrypted.</Notice>}
      <dl className="props" style={{ maxWidth: '40rem' }}>
        <dt><KeyRound aria-hidden="true" size="0.9375rem" /> Key in use</dt><dd>Version {data.active_version}{active && <span className="muted">· since {shortDate(active.created_at)}</span>}</dd>
        <dt>Algorithm</dt><dd>{data.algorithm}, a separate key for every value</dd>
        <dt>Waiting to move</dt><dd>{data.pending_rewrap === 0 ? <span className="muted">Nothing. All data uses the current key.</span> : `${data.pending_rewrap} value${data.pending_rewrap === 1 ? '' : 's'}`}</dd>
      </dl>
      <div className="table-wrap" style={{ marginTop: '1.5rem' }}>
        <table className="table" aria-label="Key versions">
          <thead><tr><th>Version</th><th>State</th><th>Created</th><th className="col-optional">Retired</th><th className="num">Values</th></tr></thead>
          <tbody>{data.versions.map((v) => (
            <tr key={v.version}>
              <td>Version {v.version}</td>
              <td><Tag color={STATE[v.state][1]} dot>{STATE[v.state][0]}</Tag></td>
              <td className="cell-muted">{shortDate(v.created_at)}</td>
              <td className="cell-muted col-optional">{v.retired_at ? shortDate(v.retired_at) : ''}</td>
              <td className="num">{v.values}</td>
            </tr>
          ))}</tbody>
        </table>
      </div>
      <p className="muted" style={{ fontSize: '0.8125rem', marginTop: '0.75rem', maxWidth: '62ch' }}>Rotating creates a new key version. Values move to it in the background, and a retired version is destroyed once nothing uses it. If the workspace is deleted, its keys are destroyed, so its data can no longer be read by anyone.</p>
      {rotating && <RotateDialog onClose={() => setRotating(false)} onDone={(v) => { setRotating(false); setDone(v); load(); }} />}
    </>
  );
}

function RotateDialog({ onClose, onDone }: { onClose: () => void; onDone: (version: number) => void }) {
  const [reason, setReason] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [stepUp, setStepUp] = useState(false);
  const rotate = async () => {
    setError(null);
    try {
      const r = await api<{ active_version: number }>('/api/v1/security/encryption/rotate', { method: 'POST', body: { reason: reason.trim() } });
      onDone(r.active_version);
    } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(true);
      else setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not rotate the key.');
    }
  };
  return (
    <>
      <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Rotate the encryption key" description="Use this on a schedule, or straight away if you think a key may have been exposed. The reason is kept in the audit log."
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void rotate()} isDisabled={!reason.trim()}>Rotate key</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        <TextField label="Reason" value={reason} onChange={setReason} autoFocus />
      </FormDialog>
      <StepUpDialog isOpen={stepUp} onClose={() => setStepUp(false)} onDone={() => { setStepUp(false); void rotate(); }} />
    </>
  );
}
