import { Users } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { useResource } from '../lib/data';
import { type Audience, AudiencePicker, EVERYONE, audienceOf, audienceSummary, isEmptyAudience, useDirectory } from '../ui/Audience';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import type { Setting } from './Settings';

interface TypeRow { key: string; name: string; description: string; category: string; enabled: boolean; audience: Audience | null }

/**
 * Settings › Requests › Request types (docs/20 §2.7a): each built-in type with an on/off switch and
 * "Who can ask". Writes `requests.types.enabled` (all on = `["*"]`) and `requests.types.audience`
 * (a type open to everyone is left out of the map).
 */
export function RequestTypesSettings({ enabled, audience, canEdit, onSaved }: { enabled: Setting; audience: Setting; canEdit: boolean; onSaved: () => void }) {
  const [data, reload, loadError] = useResource<{ items: TypeRow[] }>('/api/v1/request-types?scope=all', [enabled.revision, audience.revision]);
  const dir = useDirectory();
  const [problem, setProblem] = useState<string | null>(null);
  const [editing, setEditing] = useState<TypeRow | null>(null);
  const [busy, setBusy] = useState(false);
  const types = data?.items ?? [];

  const put = async (s: Setting, value: unknown) => {
    setBusy(true); setProblem(null);
    try {
      await api(`/api/v1/settings/${s.key}`, { method: 'PUT', headers: { 'if-match': `"${s.revision}"` }, body: { value } });
      onSaved(); reload();
      return true;
    } catch (e) {
      setProblem(e instanceof ApiError && e.code === 'precondition_failed' ? 'Someone else changed this. Reload the page to see the new value.'
        : e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not save. Try again.');
      return false;
    } finally { setBusy(false); }
  };
  const toggle = (t: TypeRow) => {
    const on = types.filter((x) => (x.key === t.key ? !x.enabled : x.enabled)).map((x) => x.key);
    void put(enabled, on.length === types.length ? ['*'] : on);
  };
  const saveAudience = async (t: TypeRow, a: Audience) => {
    const map = { ...((audience.value as Record<string, Audience>) ?? {}) };
    if (a.all) delete map[t.key]; else map[t.key] = a;
    if (await put(audience, map)) setEditing(null);
  };

  if (loadError) return <Notice tone="danger" title="Couldn’t load the request types">Reload the page to try again.</Notice>;
  if (!data) return <Skeleton rows={4} label="Loading request types" />;
  return (
    <>
      {problem && <Notice tone="danger" title={problem} />}
      <ul className="settings-list req-types" aria-label="Request types">
        {types.map((t) => {
          const who = t.audience ? audienceSummary(audienceOf(t.audience), dir) : 'Everyone';
          return (
            <li key={t.key} className="setting">
              <div className="setting-head">
                <div className="setting-text">
                  <p className="setting-label" id={`rt-${t.key}`}>{t.name} <Tag color="gray">{t.category}</Tag></p>
                  <p className="setting-hint">{t.description}</p>
                  <p className="setting-meta"><Users aria-hidden="true" size="0.75rem" /> Who can ask: <strong>{t.enabled ? who : 'Nobody (turned off)'}</strong></p>
                </div>
                <div className="setting-value">
                  {canEdit && t.enabled && <Button variant="secondary" className="btn-sm" isDisabled={busy} onPress={() => setEditing(t)} aria-label={`Who can ask for ${t.name}`}>Who can ask</Button>}
                  <label className="switch-row">
                    <input type="checkbox" role="switch" className="switch" checked={t.enabled} disabled={!canEdit || busy} onChange={() => toggle(t)} aria-label={`Offer ${t.name}`} />
                    <span aria-hidden="true">{t.enabled ? 'On' : 'Off'}</span>
                  </label>
                </div>
              </div>
            </li>
          );
        })}
      </ul>
      {editing && <WhoCanAsk type={editing} busy={busy} onClose={() => setEditing(null)} onSave={(a) => void saveAudience(editing, a)} />}
    </>
  );
}

function WhoCanAsk({ type, busy, onClose, onSave }: { type: TypeRow; busy: boolean; onClose: () => void; onSave: (a: Audience) => void }) {
  const [aud, setAud] = useState<Audience>(EVERYONE);
  useEffect(() => setAud(type.audience ? audienceOf(type.audience) : EVERYONE), [type]);
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} width={600} title={`Who can ask for ${type.name.toLowerCase()}s`}
      description="Only these people see the type in New request. Requests already sent are not affected."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => onSave(aud)} isDisabled={busy || (!aud.all && isEmptyAudience(aud))}>Save</Button></>}>
      <AudiencePicker value={aud} onChange={setAud} legend="Who can ask" summary="Can ask" />
    </FormDialog>
  );
}
