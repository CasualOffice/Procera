import { Lock, Users } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { type Audience, AudiencePicker, EVERYONE, audienceOf, audienceSummary, isEmptyAudience, useDirectory } from '../ui/Audience';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { TextField } from '../ui/components/TextField';
import { Tag } from '../ui/Tag';
import type { Act } from './docGovernance';

/**
 * Who a policy applies to (BPMS-045, FR-PL02): a category for the policy portal, an audience, and
 * "restricted" (only that audience can see it at all). Publishers change it; readers see it.
 */
export function ApplicabilityRow({ artifactId, category, applicability, restricted, canEdit, act }: { artifactId: string; category: string | null; applicability: unknown; restricted: boolean; canEdit: boolean; act: Act }) {
  const dir = useDirectory();
  const current: Audience = applicability ? audienceOf(applicability) : EVERYONE;
  const [open, setOpen] = useState(false);
  const [cat, setCat] = useState('');
  const [aud, setAud] = useState<Audience>(EVERYONE);
  const [only, setOnly] = useState(false);
  useEffect(() => { if (open) { setCat(category ?? ''); setAud(current); setOnly(restricted); } // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);
  const save = () => act(() => api(`/api/v1/artifacts/${artifactId}/applicability`, { method: 'PUT', body: { category: cat.trim() || null, applicability: aud.all ? null : aud, restricted: !aud.all && only } }), 'Applicability saved.').then((ok) => ok && setOpen(false));

  return (
    <>
      <dt><Users aria-hidden="true" size="0.9375rem" /> Applies to</dt>
      <dd>
        <span>{audienceSummary(current, dir)}</span>
        {restricted && <Tag color="orange"><Lock aria-hidden="true" size="0.6875rem" /> Only they can see it</Tag>}
        {category && <Tag color="gray">{category}</Tag>}
        {canEdit && <span className="prop-actions"><button type="button" className="prop-link" onClick={() => setOpen(true)}>Change</button></span>}
      </dd>
      <FormDialog isOpen={open} onOpenChange={setOpen} width={600} title="Who this policy applies to" description="It shows in the Policies list of the people it applies to."
        footer={<><Button variant="secondary" onPress={() => setOpen(false)}>Cancel</Button><Button onPress={() => void save()} isDisabled={!aud.all && isEmptyAudience(aud)}>Save</Button></>}>
        <TextField label="Category" value={cat} onChange={setCat} description="Groups policies in the Policies list, for example HR, IT or Finance." />
        <AudiencePicker value={aud} onChange={setAud} />
        {!aud.all && (
          <label className="check-row"><input type="checkbox" checked={only} onChange={(e) => setOnly(e.target.checked)} /><span><strong>Only these people can see it</strong><small>Everyone else won’t find it in the library, search or the Policies list.</small></span></label>
        )}
      </FormDialog>
    </>
  );
}
