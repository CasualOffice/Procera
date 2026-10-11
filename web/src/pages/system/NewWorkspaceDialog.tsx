import { useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../../lib/api';
import { toFailure, useItems } from '../../lib/data';
import { PROFILES, type QuotaDefaults, profileLabel } from '../../lib/system';
import { Button } from '../../ui/components/Button';
import { FormDialog } from '../../ui/components/FormDialog';
import { Notice } from '../../ui/components/Notice';
import { RadioCards } from '../../ui/components/RadioCards';
import { TextField } from '../../ui/components/TextField';
import { useStepUp } from '../../ui/StepUp';

/** One line per profile: what it pre-sets (docs/20 §0). Every setting stays changeable later. */
const HINTS: Record<(typeof PROFILES)[number], string> = {
  starter: 'Under 50 people. Two-step verification for owners and admins; the owner publishes without review.',
  standard: 'Most organisations. Invite-only, independent review for controlled processes.',
  enterprise: 'Many departments. Two-step verification for everyone, two-person rule, access reviews.',
  regulated: 'Pharma, finance, public sector. Part 11 signatures and Separation mode on.',
};

/** The slug the name suggests: lowercase a–z, 0–9 and single hyphens, at most 63 characters. */
export function slugify(name: string): string {
  return name.normalize('NFKD').replace(/[̀-ͯ]/g, '').toLowerCase()
    .replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 63).replace(/-+$/, '');
}

/** Mirrors the server's rule (platform `create_workspace`) so the person sees it before submitting. */
function slugProblem(slug: string): string | null {
  if (slug.length < 3 || slug.length > 63) return 'Use 3 to 63 characters.';
  if (!/^[a-z0-9-]+$/.test(slug)) return 'Use lowercase letters, numbers and hyphens only.';
  if (slug.startsWith('-') || slug.endsWith('-')) return 'Don’t start or end with a hyphen.';
  return null;
}

type Field = 'name' | 'slug' | 'profile' | 'owner_email';

/** Maps a `validation_failed` detail ("slug must be …") to the field it names. */
function fieldOf(detail: string | undefined): Field | null {
  const f = detail?.split(' ')[0];
  return f === 'name' || f === 'slug' || f === 'profile' || f === 'owner_email' ? f : null;
}

export interface Created { name: string; slug: string; ownerEmail: string }

/**
 * System › Workspaces › New workspace (docs/05 §3.20, E2E-R1-02): name, slug, configuration profile
 * and the owner's email. Creating it seeds the workspace and emails the owner an invitation.
 */
export function NewWorkspaceDialog({ onClose, onCreated }: { onClose: () => void; onCreated: (c: Created) => void }) {
  const [name, setName] = useState('');
  const [slug, setSlug] = useState('');
  const [slugEdited, setSlugEdited] = useState(false);
  const [profile, setProfile] = useState<string>('standard');
  const [owner, setOwner] = useState('');
  const [tried, setTried] = useState(false);
  const [server, setServer] = useState<Partial<Record<Field, string>>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [defaults] = useItems<QuotaDefaults>('/sysapi/v1/quota-defaults');
  const { guard, dialog } = useStepUp((e) => fail(e));

  const effectiveSlug = slugEdited ? slug : slugify(name);
  const local: Partial<Record<Field, string>> = {
    name: !name.trim() ? 'Enter a name.' : name.trim().length > 120 ? 'Use 120 characters or fewer.' : undefined,
    slug: slugProblem(effectiveSlug.trim()) ?? undefined,
    owner_email: !/^[^\s@]+@[^\s@]+$/.test(owner.trim()) ? 'Enter the owner’s email address.' : undefined,
  };
  const errorFor = (f: Field) => server[f] ?? (tried ? local[f] : undefined);
  const valid = !local.name && !local.slug && !local.owner_email;

  function fail(e: unknown) {
    if (e instanceof ApiError && e.code === 'slug_taken') {
      setServer({ slug: 'That slug is already in use. Choose another.' });
    } else if (e instanceof ApiError && e.code === 'validation_failed' && fieldOf(e.problem.detail)) {
      setServer({ [fieldOf(e.problem.detail)!]: `${e.problem.detail!.charAt(0).toUpperCase()}${e.problem.detail!.slice(1).replace(/^Owner_email/, 'Owner email')}.` });
    } else {
      setError(toFailure(e).message);
    }
  }

  const submit = () => {
    setTried(true);
    setServer({});
    setError(null);
    if (!valid || busy) return;
    setBusy(true);
    const body = { name: name.trim(), slug: effectiveSlug.trim(), profile, owner_email: owner.trim() };
    void guard(async () => {
      await api('/sysapi/v1/workspaces', { method: 'POST', body });
      onCreated({ name: body.name, slug: body.slug, ownerEmail: body.owner_email });
    }).finally(() => setBusy(false));
  };

  const seatsFor = (p: string) => {
    const d = defaults?.find((x) => x.profile === p);
    if (!d) return undefined;
    return d.max_members === null ? 'Unlimited seats' : `${d.max_members.toLocaleString()} seats`;
  };

  return (
    <>
      <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="New workspace" width={600}
        description="The owner gets an email invitation to join and set the workspace up."
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={submit} isDisabled={busy}>{busy ? 'Creating…' : 'Create workspace'}</Button></>}>
        {error && <Notice tone="danger" title="Couldn’t create the workspace">{error}</Notice>}
        <Form className="quota-form" validationBehavior="aria" onSubmit={(e) => { e.preventDefault(); submit(); }}>
          <TextField label="Workspace name" value={name} onChange={(v) => { setName(v); setServer((s) => ({ ...s, name: undefined })); }}
            isRequired autoFocus autoComplete="off" isInvalid={Boolean(errorFor('name'))} errorMessage={errorFor('name')} />
          <TextField label="Slug" value={effectiveSlug} onChange={(v) => { setSlugEdited(true); setSlug(v.toLowerCase()); setServer((s) => ({ ...s, slug: undefined })); }}
            isRequired autoComplete="off" spellCheck="false"
            description="Identifies the workspace in addresses. Lowercase letters, numbers and hyphens."
            isInvalid={Boolean(errorFor('slug'))} errorMessage={errorFor('slug')} />
          <RadioCards label="Configuration profile" value={profile} onChange={setProfile}
            isInvalid={Boolean(server.profile)}
            options={PROFILES.map((p) => ({ id: p, label: profileLabel(p), meta: seatsFor(p), description: HINTS[p] }))} />
          {server.profile && <p className="field-error" role="alert">{server.profile}</p>}
          <p className="field-description">The profile pre-sets security, review and retention defaults. Every setting can be changed later.</p>
          <TextField label="Owner email" type="email" value={owner} onChange={(v) => { setOwner(v); setServer((s) => ({ ...s, owner_email: undefined })); }}
            isRequired autoComplete="off" description="We’ll email this person an invitation to own the workspace."
            isInvalid={Boolean(errorFor('owner_email'))} errorMessage={errorFor('owner_email')} />
        </Form>
      </FormDialog>
      {dialog}
    </>
  );
}
