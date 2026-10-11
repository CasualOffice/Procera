import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { Link } from '../lib/router';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { type LoadFailure, toFailure } from '../lib/data';
import { LoadError, Skeleton } from '../ui/States';
import '../ui/prefs.css';

interface Overrides { digest: string | null; digest_hour: number | null; quiet_enabled: boolean | null; quiet_start: number | null; quiet_end: number | null }
interface Prefs {
  digest: 'off' | 'daily' | 'weekly'; digest_hour: number; quiet_enabled: boolean; quiet_start: number; quiet_end: number;
  overrides: Overrides;
  workspace: { digest: string; digest_hour: number; quiet_enabled: boolean; quiet_start: number; quiet_end: number };
}

const HOURS = Array.from({ length: 24 }, (_, h) => ({ id: String(h), label: `${String(h).padStart(2, '0')}:00` }));
const hour = (h: number) => `${String(h).padStart(2, '0')}:00`;
const DIGESTS = [
  ['off', 'Send right away', 'Each email arrives as it happens'],
  ['daily', 'Daily digest', 'One email a day with everything new'],
  ['weekly', 'Weekly digest', 'One email on Monday'],
] as const;
const digestLabel = (d: string) => (d === 'off' ? 'right away' : d === 'weekly' ? 'weekly' : 'daily');

/** Notification preferences (FR-N04): digest and quiet hours, each falling back to the workspace default. */
export function NotificationPreferences() {
  const [prefs, setPrefs] = useState<Prefs | null>(null);
  const [failed, setFailed] = useState<LoadFailure | null>(null);
  const [draft, setDraft] = useState<Overrides | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = () => {
    setFailed(null);
    api<Prefs>('/api/v1/me/notification-preferences').then((p) => { setPrefs(p); setDraft(p.overrides); }).catch((e) => setFailed(toFailure(e)));
  };
  useEffect(load, []);

  if (failed) return <LoadError error={failed} onRetry={load} what="your preferences" />;
  if (!prefs || !draft) return <Skeleton rows={4} label="Loading preferences" />;

  const ws = prefs.workspace;
  const eff = {
    digest: draft.digest ?? ws.digest,
    digest_hour: draft.digest_hour ?? ws.digest_hour,
    quiet_enabled: draft.quiet_enabled ?? ws.quiet_enabled,
    quiet_start: draft.quiet_start ?? ws.quiet_start,
    quiet_end: draft.quiet_end ?? ws.quiet_end,
  };
  const set = (patch: Partial<Overrides>) => { setSaved(false); setDraft({ ...draft, ...patch }); };
  const save = async (body: Overrides) => {
    setError(null);
    try {
      const p = await api<Prefs>('/api/v1/me/notification-preferences', { method: 'PUT', body });
      setPrefs(p); setDraft(p.overrides); setSaved(true);
    } catch (e) { setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not save your preferences.'); }
  };
  const custom = Object.values(prefs.overrides).some((v) => v !== null);

  return (
    <>
      <div className="page-head">
        <div><h1>Notification preferences</h1><p>How and when Procera emails you. In-app notifications always arrive right away.</p></div>
        <Link to="/notifications" className="btn btn-secondary btn-md">Back to notifications</Link>
      </div>
      {error && <Notice tone="danger" title={error} />}
      {saved && <Notice tone="success" title="Preferences saved" />}

      <section className="prefs-section" aria-labelledby="digest-h">
        <h2 id="digest-h">Email</h2>
        <p className="muted">Workspace default: {digestLabel(ws.digest)}{ws.digest !== 'off' ? ` at ${hour(ws.digest_hour)}` : ''}. Critical announcements and invitations are never held for a digest.</p>
        <div className="radio-cards prefs-cards" role="radiogroup" aria-label="Email delivery">
          {DIGESTS.map(([k, label, hint]) => (
            <button key={k} type="button" role="radio" className="radio-card" aria-checked={eff.digest === k} onClick={() => set({ digest: k })}>
              <span><strong>{label}</strong><small>{hint}</small></span>
            </button>
          ))}
        </div>
        {eff.digest !== 'off' && (
          <div className="prefs-row">
            <Select label="Digest time" description="In your time zone" options={HOURS} selectedKey={String(eff.digest_hour)} onSelectionChange={(k) => set({ digest_hour: Number(k) })} />
          </div>
        )}
      </section>

      <section className="prefs-section" aria-labelledby="quiet-h">
        <h2 id="quiet-h">Quiet hours</h2>
        <p className="muted">Emails wait until quiet hours end. Workspace default: {ws.quiet_enabled ? `${hour(ws.quiet_start)} to ${hour(ws.quiet_end)}` : 'off'}.</p>
        <Checkbox isSelected={eff.quiet_enabled} onChange={(v) => set({ quiet_enabled: v })}>Hold emails during quiet hours</Checkbox>
        {eff.quiet_enabled && (
          <div className="prefs-row">
            <Select label="From" options={HOURS} selectedKey={String(eff.quiet_start)} onSelectionChange={(k) => set({ quiet_start: Number(k) })} />
            <Select label="Until" options={HOURS} selectedKey={String(eff.quiet_end)} onSelectionChange={(k) => set({ quiet_end: Number(k) })} />
          </div>
        )}
      </section>

      <div className="prefs-actions">
        <Button onPress={() => void save(draft)}>Save preferences</Button>
        {custom && <Button variant="secondary" onPress={() => void save({ digest: null, digest_hour: null, quiet_enabled: null, quiet_start: null, quiet_end: null })}>Use workspace defaults</Button>}
      </div>
    </>
  );
}
