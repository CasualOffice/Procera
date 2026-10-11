import { useEffect, useMemo, useState } from 'react';
import { Form } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { type LoadFailure, toFailure } from '../lib/data';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { TextField } from '../ui/components/TextField';
import { LoadError, Skeleton } from '../ui/States';

interface ProfileData { email: string; display_name: string; timezone: string; locale: string; locales: string[] }

const LANGUAGES: Record<string, string> = { en: 'English' };

/** Every IANA time zone the browser knows, with the server's default first. */
function timeZones(current: string): { id: string; label: string }[] {
  const all = typeof Intl.supportedValuesOf === 'function' ? Intl.supportedValuesOf('timeZone') : [];
  const set = new Set(['UTC', ...all, current]);
  return [...set].map((z) => ({ id: z, label: z.replaceAll('_', ' ') }));
}

/**
 * Account › Profile: your name, time zone and language. They belong to your account, so they apply
 * in every workspace you're a member of.
 */
export function Profile() {
  const { reload } = useSession();
  const [data, setData] = useState<ProfileData | null>(null);
  const [error, setError] = useState<LoadFailure | null>(null);
  const [name, setName] = useState('');
  const [tz, setTz] = useState('UTC');
  const [locale, setLocale] = useState('en');
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);

  const load = () => {
    setError(null);
    api<ProfileData>('/api/v1/me/profile')
      .then((p) => { setData(p); setName(p.display_name); setTz(p.timezone); setLocale(p.locale); })
      .catch((e) => setError(toFailure(e)));
  };
  useEffect(load, []);
  const zones = useMemo(() => timeZones(data?.timezone ?? 'UTC'), [data?.timezone]);

  if (error) return <LoadError error={error} onRetry={load} what="your profile" />;
  if (!data) return <Skeleton rows={3} />;

  const dirty = name.trim() !== data.display_name || tz !== data.timezone || locale !== data.locale;
  const browserTz = Intl.DateTimeFormat().resolvedOptions().timeZone;

  const save = async () => {
    setBusy(true); setSaveError(null); setSaved(false);
    try {
      const p = await api<ProfileData>('/api/v1/me/profile', { method: 'PATCH', body: { display_name: name.trim(), timezone: tz, locale } });
      setData(p); setName(p.display_name); setSaved(true);
      await reload();
    } catch (e) {
      setSaveError(e instanceof ApiError ? (e.problem.detail ?? e.message) : 'Could not save your profile');
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Profile</h1>
          <p>How you appear to others and how dates and times are shown to you. These apply in every workspace.</p>
        </div>
      </div>
      {saved && <Notice tone="success" title="Profile saved" />}
      {saveError && <Notice tone="danger" title="Couldn’t save your profile">{saveError}</Notice>}
      <section className="account-section" aria-labelledby="profile-about">
        <div className="section-head"><h2 id="profile-about">About you</h2></div>
        <Form className="card form-card" onSubmit={(e) => { e.preventDefault(); if (dirty) void save(); }}>
          <TextField label="Your name" value={name} onChange={setName} isRequired maxLength={200} description="Shown on documents, approvals and announcements." />
          <TextField label="Email" value={data.email} isReadOnly description="Your sign-in address. Ask a workspace admin if it needs to change." />
          <Select
            label="Time zone"
            options={zones}
            selectedKey={tz}
            onSelectionChange={(k) => k && setTz(String(k))}
            description={browserTz && browserTz !== tz ? `This device is set to ${browserTz.replaceAll('_', ' ')}.` : 'Used for due dates, reminders and digests.'}
          />
          {browserTz && browserTz !== tz && zones.some((z) => z.id === browserTz) && (
            <Button variant="ghost" className="btn-sm" onPress={() => setTz(browserTz)}>Use this device’s time zone</Button>
          )}
          <Select
            label="Language"
            options={data.locales.map((l) => ({ id: l, label: LANGUAGES[l] ?? l }))}
            selectedKey={locale}
            onSelectionChange={(k) => k && setLocale(String(k))}
            description="More languages are on the way."
          />
          <div className="form-actions">
            <Button type="submit" isDisabled={!dirty || busy || !name.trim()}>{busy ? 'Saving…' : 'Save changes'}</Button>
          </div>
        </Form>
      </section>
    </>
  );
}
