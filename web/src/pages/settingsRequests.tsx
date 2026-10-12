import { BookOpen, Users } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { useItems, useResource } from '../lib/data';
import { type Audience, AudiencePicker, EVERYONE, audienceOf, audienceSummary, isEmptyAudience, useDirectory } from '../ui/Audience';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import type { Setting } from './Settings';

interface TypeRow { key: string; name: string; description: string; category: string; enabled: boolean; audience: Audience | null }

/**
 * Settings › Requests › Request types (docs/20 §2.7a): each built-in type with an on/off switch and
 * "Who can ask". Writes `requests.types.enabled` (all on = `["*"]`) and `requests.types.audience`
 * (a type open to everyone is left out of the map).
 */
export function RequestTypesSettings({ enabled, audience, policy, canEdit, onSaved }: { enabled: Setting; audience: Setting; policy?: Setting; canEdit: boolean; onSaved: () => void }) {
  const [data, reload, loadError] = useResource<{ items: TypeRow[] }>('/api/v1/request-types?scope=all', [enabled.revision, audience.revision]);
  const dir = useDirectory();
  const [problem, setProblem] = useState<string | null>(null);
  const [editing, setEditing] = useState<TypeRow | null>(null);
  const [linking, setLinking] = useState<TypeRow | null>(null);
  const [policies] = useItems<Doc>(policy ? '/api/v1/artifacts?kind=policy' : null);
  const [sops] = useItems<Doc>(policy ? '/api/v1/artifacts?kind=sop' : null);
  const docs = [...(policies ?? []), ...(sops ?? [])].filter((d) => d.current_published_version_id);
  const linked = (policy?.value as Record<string, string> | undefined) ?? {};
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

  const saveLink = async (t: TypeRow, id: string | null) => {
    if (!policy) return;
    const map = { ...linked };
    if (id) map[t.key] = id; else delete map[t.key];
    if (await put(policy, map)) setLinking(null);
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
                  {policy && <p className="setting-meta"><BookOpen aria-hidden="true" size="0.75rem" /> Policy: <strong>{docs.find((d) => d.id === linked[t.key])?.title ?? (linked[t.key] ? 'A document that is no longer published' : 'None')}</strong></p>}
                </div>
                <div className="setting-value">
                  {canEdit && t.enabled && <Button variant="secondary" className="btn-sm" isDisabled={busy} onPress={() => setEditing(t)} aria-label={`Who can ask for ${t.name}`}>Who can ask</Button>}
                  {canEdit && policy && <Button variant="secondary" className="btn-sm" isDisabled={busy} onPress={() => setLinking(t)} aria-label={`Policy for ${t.name}`}>Policy</Button>}
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
      {linking && <PolicyFor type={linking} docs={docs} current={linked[linking.key] ?? null} busy={busy} onClose={() => setLinking(null)} onSave={(id) => void saveLink(linking, id)} />}
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

interface Doc { id: string; kind: string; title: string; current_published_version_id: string | null }

/** Picks the published policy or SOP shown to the people who decide a request type. */
function PolicyFor({ type, docs, current, busy, onClose, onSave }: { type: TypeRow; docs: Doc[]; current: string | null; busy: boolean; onClose: () => void; onSave: (id: string | null) => void }) {
  const [pick, setPick] = useState<string | null>(current);
  const NONE = '__none';
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} width={520} title={`Policy for ${type.name.toLowerCase()}s`}
      description="Approvers see a link to its current published version, and each decision records the version they were shown."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => onSave(pick)} isDisabled={busy}>Save</Button></>}>
      {docs.length === 0 ? <Notice tone="info" title="No published policies or SOPs yet">Publish one in the Library first.</Notice> : (
        <Select label="Policy or SOP" placeholder="Choose a document…" selectedKey={pick ?? NONE} onSelectionChange={(k) => setPick(k === null || k === NONE ? null : String(k))}
          options={[{ id: NONE, label: 'None' }, ...docs.map((d) => ({ id: d.id, label: `${d.title} (${d.kind === 'sop' ? 'SOP' : 'policy'})` }))]} />
      )}
    </FormDialog>
  );
}
