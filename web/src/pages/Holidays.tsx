import { CalendarDays, ChevronLeft, ChevronRight, Copy, MapPin, Pencil, Plus, Trash2, Upload } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Form, Label, TextArea, TextField as AriaTextField, ToggleButton } from 'react-aria-components';
import { ApiError, api } from '../lib/api';
import { useResource } from '../lib/data';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Checkbox } from '../ui/components/Checkbox';
import { ConfirmDialog } from '../ui/components/Dialog';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Select } from '../ui/components/Select';
import { Tabs } from '../ui/components/Tabs';
import { TextField } from '../ui/components/TextField';
import { Empty, LoadError, Skeleton } from '../ui/States';
import './holidays.css';

interface Holiday { date: string; name: string; optional: boolean }
interface Calendar { id: string; name: string; revision: number; locations: string[]; years: number[] }
interface Location { id: string; name: string; timezone: string; work_week: number[]; day_hours: number; holiday_calendar_id: string | null; holiday_calendar: string | null; members: number; archived: boolean; revision: number }
interface ImportReport { added: number; updated: number; skipped: { line: number; text: string; reason: string }[] }

const DAYS = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
const DAY_NAMES = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
const errText = (e: unknown, fallback: string) => (e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : fallback);
/** ISO weekday (1 = Monday) of a `YYYY-MM-DD` date, without time-zone drift. */
const isoDay = (d: string) => { const [y, m, dd] = d.split('-').map(Number); return ((new Date(Date.UTC(y!, m! - 1, dd!)).getUTCDay() + 6) % 7) + 1; };
const monthOf = (d: string) => new Date(`${d}T00:00:00Z`).toLocaleDateString(undefined, { month: 'short', timeZone: 'UTC' });
const monthLong = (d: string) => new Date(`${d}T00:00:00Z`).toLocaleDateString(undefined, { month: 'long', timeZone: 'UTC' });
const zones = (() => { try { return (Intl as unknown as { supportedValuesOf(k: string): string[] }).supportedValuesOf('timeZone'); } catch { return ['UTC']; } })();
const localTime = (tz: string) => { try { return new Date().toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', timeZone: tz }); } catch { return ''; } };

/**
 * Admin › Holidays and locations (BPMS-222, docs/25 §2.1, §8): holiday calendars by year (add,
 * import CSV or iCal, copy a year) and work locations (time zone, work week, hours, calendar). Leave
 * day counting and attendance read these.
 */
export function Holidays() {
  const { can } = useSession();
  const manage = can('calendar:manage');
  return (
    <>
      <div className="page-head">
        <div>
          <h1>Holidays and locations</h1>
          <p>Where people work, which days they work, and the public holidays they get. Time off and attendance count days from these.</p>
        </div>
      </div>
      <Tabs label="Holidays and locations" tabs={[
        { id: 'holidays', label: 'Holidays', content: <HolidaysTab manage={manage} /> },
        { id: 'locations', label: 'Locations', content: <LocationsTab manage={manage} /> },
      ]} />
    </>
  );
}

// ------------------------------------------------------------------ holidays

function HolidaysTab({ manage }: { manage: boolean }) {
  const [cals, reloadCals, calErr] = useResource<{ items: Calendar[] }>('/api/v1/holiday-calendars');
  const [calId, setCalId] = useState<string | null>(null);
  const [year, setYear] = useState(new Date().getFullYear());
  const [tick, setTick] = useState(0);
  const cal = cals?.items.find((c) => c.id === calId) ?? cals?.items[0] ?? null;
  const [hols, , holErr] = useResource<{ items: Holiday[] }>(cal ? `/api/v1/holiday-calendars/${cal.id}/holidays?year=${year}` : null, [tick]);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string; body?: React.ReactNode } | null>(null);
  const [editing, setEditing] = useState<Holiday | 'new' | null>(null);
  const [importing, setImporting] = useState(false);
  const [creating, setCreating] = useState(false);
  const reload = () => { setTick((t) => t + 1); reloadCals(); };

  if (calErr) return <LoadError error={calErr} onRetry={reloadCals} what="holiday calendars" />;
  if (!cals) return <Skeleton variant="list" label="Loading holiday calendars" />;

  const report = (r: ImportReport, what: string) => setNotice({
    tone: r.skipped.length && !r.added && !r.updated ? 'danger' : 'success',
    title: `${what}: ${r.added} added${r.updated ? `, ${r.updated} updated` : ''}${r.skipped.length ? `, ${r.skipped.length} skipped` : ''}.`,
    body: r.skipped.length ? <ul className="hol-skipped">{r.skipped.slice(0, 8).map((s, i) => <li key={i}>{s.line ? `Line ${s.line}: ` : ''}{s.text} — {s.reason}</li>)}</ul> : undefined,
  });
  const copyYear = async () => {
    if (!cal) return;
    try { report(await api<ImportReport>(`/api/v1/holiday-calendars/${cal.id}/copy-year`, { method: 'POST', body: { from: year - 1, to: year } }), `Copied from ${year - 1}`); reload(); }
    catch (e) { setNotice({ tone: 'danger', title: errText(e, 'Couldn’t copy.') }); }
  };

  if (!cal) {
    return (
      <>
        <Empty icon={CalendarDays} title="No holiday calendars yet" action={manage ? <Button onPress={() => setCreating(true)}><Plus aria-hidden="true" size="1rem" /> New calendar</Button> : undefined}>
          Create one calendar per set of public holidays (for example India or United Kingdom), then choose it for each location.
        </Empty>
        <CalendarDialog isOpen={creating} onClose={() => setCreating(false)} calendars={[]} onDone={(c) => { setCalId(c.id); reloadCals(); }} />
      </>
    );
  }

  const items = hols?.items ?? [];
  const prevHas = cal.years.includes(year - 1);
  const byMonth = items.reduce<Record<string, Holiday[]>>((acc, h) => { (acc[monthLong(h.date)] ??= []).push(h); return acc; }, {});

  return (
    <div className="hol">
      <div className="hol-bar">
        {cals.items.length > 1
          ? <Select label="Calendar" selectedKey={cal.id} onSelectionChange={(k) => setCalId(String(k))} options={cals.items.map((c) => ({ id: c.id, label: c.name }))} />
          : <div className="hol-cal"><span className="hol-cal-name">{cal.name}</span></div>}
        <div className="hol-year" role="group" aria-label="Year">
          <Button variant="ghost" aria-label="Previous year" onPress={() => setYear((y) => y - 1)}><ChevronLeft aria-hidden="true" size="1rem" /></Button>
          <span className="hol-year-n" aria-live="polite">{year}</span>
          <Button variant="ghost" aria-label="Next year" onPress={() => setYear((y) => y + 1)}><ChevronRight aria-hidden="true" size="1rem" /></Button>
        </div>
        {manage && (
          <div className="actions hol-acts">
            <Button variant="secondary" onPress={() => setCreating(true)}><Plus aria-hidden="true" size="1rem" /> New calendar</Button>
            <Button variant="secondary" onPress={() => setImporting(true)}><Upload aria-hidden="true" size="1rem" /> Import</Button>
            <Button onPress={() => setEditing('new')}><Plus aria-hidden="true" size="1rem" /> Add holiday</Button>
          </div>
        )}
      </div>
      <p className="hol-meta muted">
        {items.length} {items.length === 1 ? 'holiday' : 'holidays'} in {year}
        {items.some((h) => h.optional) && ` · ${items.filter((h) => h.optional).length} optional`}
        {' · '}{cal.locations.length ? <>Used by {cal.locations.join(', ')}</> : 'Not used by any location yet'}
      </p>
      {notice && <Notice tone={notice.tone} title={notice.title}>{notice.body}</Notice>}
      {holErr ? <LoadError error={holErr} what="holidays" /> : !hols ? <Skeleton variant="list" label="Loading holidays" /> : items.length === 0 ? (
        <Empty compact icon={CalendarDays} title={`No holidays in ${year}`} action={manage ? (
          <div className="actions">
            {prevHas && <Button variant="secondary" onPress={() => void copyYear()}><Copy aria-hidden="true" size="1rem" /> Copy from {year - 1}</Button>}
            <Button variant="secondary" onPress={() => setImporting(true)}><Upload aria-hidden="true" size="1rem" /> Import</Button>
          </div>
        ) : undefined}>
          {manage ? 'Add them one by one, paste a list, or import your provider’s iCal file.' : 'Your admin hasn’t added this year’s holidays yet.'}
        </Empty>
      ) : (
        <div className="hol-months">
          {Object.entries(byMonth).map(([month, hs]) => (
            <section key={month} className="hol-month" aria-label={month}>
              <h2 className="hol-month-h">{month}</h2>
              <ul className="hol-list" aria-label={`Holidays in ${month}`}>
                {hs.map((h) => {
                  const wd = isoDay(h.date);
                  return (
                    <li key={h.date} className="hol-row">
                      <span className="hol-date" aria-hidden="true"><span className="hol-day">{Number(h.date.slice(8))}</span><span className="hol-mon">{monthOf(h.date)}</span></span>
                      <span className="hol-main">
                        <span className="hol-name">{h.name}</span>
                        <span className="hol-sub">{DAY_NAMES[wd - 1]}, {h.date}{wd >= 6 && <span className="muted"> · weekend</span>}</span>
                      </span>
                      {h.optional && <span className="tag tag-purple" title="People may choose to take it">Optional</span>}
                      {manage && (
                        <span className="hol-row-acts">
                          <Button variant="ghost" aria-label={`Edit ${h.name}`} onPress={() => setEditing(h)}><Pencil aria-hidden="true" size="0.9375rem" /></Button>
                          <ConfirmDialog destructive title={`Remove ${h.name}?`} confirmLabel="Remove"
                            onConfirm={() => void api(`/api/v1/holiday-calendars/${cal.id}/holidays/${h.date}`, { method: 'DELETE' }).then(() => { setNotice({ tone: 'success', title: `${h.name} removed.` }); reload(); }).catch((e) => setNotice({ tone: 'danger', title: errText(e, 'Couldn’t remove it.') }))}
                            trigger={<Button variant="ghost" aria-label={`Remove ${h.name}`}><Trash2 aria-hidden="true" size="0.9375rem" /></Button>}>
                            <p>{h.date} becomes a normal day for {cal.locations.length ? cal.locations.join(', ') : 'locations using this calendar'}.</p>
                          </ConfirmDialog>
                        </span>
                      )}
                    </li>
                  );
                })}
              </ul>
            </section>
          ))}
        </div>
      )}
      {manage && prevHas && items.length > 0 && !cal.years.includes(year + 1) && (
        <p className="hol-foot muted">Next year’s dates? <Button variant="ghost" onPress={() => { setYear(year + 1); }}>Go to {year + 1}</Button></p>
      )}
      <HolidayDialog key={editing === 'new' ? 'new' : editing?.date ?? 'none'} calendar={cal} year={year} holiday={editing} onClose={() => setEditing(null)} onDone={(t) => { setNotice({ tone: 'success', title: t }); reload(); }} />
      <ImportDialog isOpen={importing} calendar={cal} onClose={() => setImporting(false)} onDone={(r) => { report(r, 'Imported'); reload(); }} />
      <CalendarDialog isOpen={creating} onClose={() => setCreating(false)} calendars={cals.items} onDone={(c) => { setCalId(c.id); reloadCals(); setNotice({ tone: 'success', title: `${c.name} created.` }); }} />
    </div>
  );
}

function HolidayDialog({ calendar, year, holiday, onClose, onDone }: { calendar: Calendar; year: number; holiday: Holiday | 'new' | null; onClose: () => void; onDone: (title: string) => void }) {
  const edit = holiday && holiday !== 'new' ? holiday : null;
  const [date, setDate] = useState(edit?.date ?? '');
  const [name, setName] = useState(edit?.name ?? '');
  const [optional, setOptional] = useState(edit?.optional ?? false);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true); setErr(null);
    try {
      if (edit && edit.date !== date) await api(`/api/v1/holiday-calendars/${calendar.id}/holidays/${edit.date}`, { method: 'DELETE' });
      await api(`/api/v1/holiday-calendars/${calendar.id}/holidays/${date}`, { method: 'PUT', body: { name, optional } });
      onDone(edit ? `${name.trim()} saved.` : `${name.trim()} added.`);
      onClose();
    } catch (e) { setErr(errText(e, 'Couldn’t save.')); } finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen={holiday !== null} onOpenChange={(o) => { if (!o) onClose(); }} title={edit ? 'Edit holiday' : 'Add a holiday'} description={calendar.name}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void save()} isDisabled={busy || !date || !name.trim()}>{edit ? 'Save' : 'Add holiday'}</Button></>}>
      <Form className="stack" onSubmit={(e) => { e.preventDefault(); void save(); }}>
        {err && <Notice tone="danger" title={err} />}
        <div className="field">
          <label className="field-label" htmlFor="hol-date">Date <span aria-hidden="true" className="field-required">*</span></label>
          <input id="hol-date" type="date" className="field-input" value={date} min={`${year - 1}-01-01`} max={`${year + 2}-12-31`} onChange={(e) => setDate(e.target.value)} required />
        </div>
        <TextField label="Name" value={name} onChange={setName} isRequired placeholder="Gandhi Jayanti" />
        <Checkbox isSelected={optional} onChange={setOptional}>Optional holiday (people choose whether to take it)</Checkbox>
      </Form>
    </FormDialog>
  );
}

function ImportDialog({ isOpen, calendar, onClose, onDone }: { isOpen: boolean; calendar: Calendar; onClose: () => void; onDone: (r: ImportReport) => void }) {
  const [text, setText] = useState('');
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const file = async (f: File | undefined) => { if (f) setText(await f.text()); };
  const go = async () => {
    setBusy(true); setErr(null);
    try { onDone(await api<ImportReport>(`/api/v1/holiday-calendars/${calendar.id}/import`, { method: 'POST', body: { text } })); setText(''); onClose(); }
    catch (e) { setErr(errText(e, 'Couldn’t import.')); } finally { setBusy(false); }
  };
  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => { if (!o) onClose(); }} title="Import holidays" description={`Into ${calendar.name}. Dates already in the calendar are renamed.`} width={600}
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void go()} isDisabled={busy || !text.trim()}>{busy ? 'Importing…' : 'Import'}</Button></>}>
      <div className="stack">
        {err && <Notice tone="danger" title={err} />}
        <AriaTextField className="field" value={text} onChange={setText}>
          <Label className="field-label">Holidays</Label>
          <TextArea className="field-input hol-paste" rows={8} placeholder={'2026-10-02,Gandhi Jayanti\n2026-11-08,Diwali\n2026-11-09,Diwali (second day),optional'} />
        </AriaTextField>
        <p className="field-description">One per line: date, name, and “optional” for optional holidays. Or choose an iCal (.ics) file exported from Google Calendar, Outlook or a government site.</p>
        <label className="hol-file">
          <input type="file" accept=".ics,.csv,.txt,text/calendar,text/csv" onChange={(e) => void file(e.target.files?.[0])} />
          <Upload aria-hidden="true" size="1rem" /> Choose a file
        </label>
      </div>
    </FormDialog>
  );
}

function CalendarDialog({ isOpen, onClose, calendars, onDone }: { isOpen: boolean; onClose: () => void; calendars: Calendar[]; onDone: (c: Calendar) => void }) {
  const [name, setName] = useState('');
  const [from, setFrom] = useState('none');
  const [err, setErr] = useState<string | null>(null);
  const go = async () => {
    setErr(null);
    try { const c = await api<Calendar>('/api/v1/holiday-calendars', { method: 'POST', body: { name, ...(from !== 'none' ? { copy_from: from } : {}) } }); setName(''); onDone(c); onClose(); }
    catch (e) { setErr(errText(e, 'Couldn’t create it.')); }
  };
  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => { if (!o) onClose(); }} title="New holiday calendar"
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void go()} isDisabled={!name.trim()}>Create calendar</Button></>}>
      <Form className="stack" onSubmit={(e) => { e.preventDefault(); void go(); }}>
        {err && <Notice tone="danger" title={err} />}
        <TextField label="Name" value={name} onChange={setName} isRequired placeholder="India 2026" description="Usually a country or a region." />
        {calendars.length > 0 && <Select label="Start from" selectedKey={from} onSelectionChange={(k) => setFrom(String(k))} options={[{ id: 'none', label: 'An empty calendar' }, ...calendars.map((c) => ({ id: c.id, label: `A copy of ${c.name}` }))]} />}
      </Form>
    </FormDialog>
  );
}

// ------------------------------------------------------------------ locations

function WeekChips({ days }: { days: number[] }) {
  return (
    <span className="loc-week" role="img" aria-label={`Works ${days.map((d) => DAY_NAMES[d - 1]).join(', ')}`}>
      {DAYS.map((d, i) => <span key={d} aria-hidden="true" className={days.includes(i + 1) ? 'on' : ''}>{d[0]}</span>)}
    </span>
  );
}

function LocationsTab({ manage }: { manage: boolean }) {
  const [archived, setArchived] = useState(false);
  const [data, reload, err] = useResource<{ items: Location[] }>(`/api/v1/work-locations?archived=${archived}`);
  const [cals] = useResource<{ items: Calendar[] }>('/api/v1/holiday-calendars');
  const [editing, setEditing] = useState<Location | 'new' | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  if (err) return <LoadError error={err} onRetry={reload} what="locations" />;
  if (!data) return <Skeleton variant="list" label="Loading locations" />;
  return (
    <div className="loc">
      <div className="hol-bar">
        <Checkbox isSelected={archived} onChange={setArchived}>Show archived</Checkbox>
        {manage && <div className="actions hol-acts"><Button onPress={() => setEditing('new')}><Plus aria-hidden="true" size="1rem" /> Add location</Button></div>}
      </div>
      {notice && <Notice tone="success" title={notice} />}
      {data.items.length === 0 ? (
        <Empty compact icon={MapPin} title="No locations yet" action={manage ? <Button onPress={() => setEditing('new')}><Plus aria-hidden="true" size="1rem" /> Add location</Button> : undefined}>
          Locations also appear when you set someone’s location in People.
        </Empty>
      ) : (
        <ul className="loc-list" aria-label="Work locations">
          {data.items.map((l) => (
            <li key={l.id} className={`loc-row${l.archived ? ' is-archived' : ''}`}>
              <span className="loc-pin" aria-hidden="true"><MapPin size="1rem" /></span>
              <span className="loc-main">
                <span className="loc-name">{l.name}{l.archived && <span className="tag tag-gray">Archived</span>}</span>
                <span className="loc-sub">{l.timezone.replace(/_/g, ' ')} · {localTime(l.timezone)} now · {l.day_hours} h days</span>
              </span>
              <WeekChips days={l.work_week} />
              <span className="loc-cal">{l.holiday_calendar ?? <span className="muted">No holidays</span>}</span>
              <span className="loc-count">{l.members} {l.members === 1 ? 'person' : 'people'}</span>
              {manage && <Button variant="ghost" aria-label={`Edit ${l.name}`} onPress={() => setEditing(l)}><Pencil aria-hidden="true" size="0.9375rem" /></Button>}
            </li>
          ))}
        </ul>
      )}
      <LocationDialog key={editing === 'new' ? 'new' : editing?.id ?? 'none'} location={editing} calendars={cals?.items ?? []} onClose={() => setEditing(null)} onDone={(t) => { setNotice(t); reload(); }} />
    </div>
  );
}

function LocationDialog({ location, calendars, onClose, onDone }: { location: Location | 'new' | null; calendars: Calendar[]; onClose: () => void; onDone: (title: string) => void }) {
  const edit = location && location !== 'new' ? location : null;
  const guess = useMemo(() => Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC', []);
  const [name, setName] = useState(edit?.name ?? '');
  const [tz, setTz] = useState(edit?.timezone ?? guess);
  const [week, setWeek] = useState<number[]>(edit?.work_week ?? [1, 2, 3, 4, 5]);
  const [hours, setHours] = useState(String(edit?.day_hours ?? 8));
  const [cal, setCal] = useState(edit?.holiday_calendar_id ?? calendars[0]?.id ?? 'none');
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => { if (!edit && cal === 'none' && calendars[0]) setCal(calendars[0].id); }, [calendars, edit, cal]);
  const body = (archived?: boolean) => ({ name, timezone: tz, work_week: week, day_hours: Number(hours), holiday_calendar_id: cal === 'none' ? null : cal, ...(archived !== undefined ? { archived } : {}) });
  const save = async (archived?: boolean) => {
    setErr(null);
    try {
      if (edit) await api(`/api/v1/work-locations/${edit.id}`, { method: 'PATCH', headers: { 'If-Match': `"${edit.revision}"` }, body: body(archived) });
      else await api('/api/v1/work-locations', { method: 'POST', body: body() });
      onDone(archived ? `${name.trim()} archived.` : archived === false ? `${name.trim()} restored.` : edit ? `${name.trim()} saved.` : `${name.trim()} added.`);
      onClose();
    } catch (e) { setErr(errText(e, 'Couldn’t save.')); }
  };
  const zoneOptions = useMemo(() => (zones.includes(tz) ? zones : [tz, ...zones]).map((z) => ({ id: z, label: z.replace(/_/g, ' ') })), [tz]);
  return (
    <FormDialog isOpen={location !== null} onOpenChange={(o) => { if (!o) onClose(); }} title={edit ? `Edit ${edit.name}` : 'Add a location'} width={560}
      footer={<>
        {edit && edit.members === 0 && <Button variant="ghost" onPress={() => void save(!edit.archived)}>{edit.archived ? 'Restore' : 'Archive'}</Button>}
        <span className="sheet-foot-gap" />
        <Button variant="secondary" onPress={onClose}>Cancel</Button>
        <Button onPress={() => void save()} isDisabled={!name.trim() || week.length === 0}>{edit ? 'Save' : 'Add location'}</Button>
      </>}>
      <Form className="stack" onSubmit={(e) => { e.preventDefault(); void save(); }}>
        {err && <Notice tone="danger" title={err} />}
        <TextField label="Name" value={name} onChange={setName} isRequired placeholder="Bangalore office" description={edit && edit.members ? `Renaming changes it on ${edit.members} ${edit.members === 1 ? 'profile' : 'profiles'} too.` : undefined} />
        <Select label="Time zone" selectedKey={tz} onSelectionChange={(k) => setTz(String(k))} options={zoneOptions} />
        <div className="field" role="group" aria-labelledby="loc-week-l">
          <span className="field-label" id="loc-week-l">Working days</span>
          <div className="loc-week-pick">
            {DAYS.map((d, i) => (
              <ToggleButton key={d} className="loc-day" aria-label={DAY_NAMES[i]} isSelected={week.includes(i + 1)} onChange={(on) => setWeek((w) => (on ? [...w, i + 1] : w.filter((x) => x !== i + 1)).sort())}>{d}</ToggleButton>
            ))}
          </div>
        </div>
        <TextField label="Hours in a working day" type="number" value={hours} onChange={setHours} inputMode="decimal" description="Used to turn hours into days in attendance." />
        <Select label="Holiday calendar" selectedKey={cal} onSelectionChange={(k) => setCal(String(k))} options={[...calendars.map((c) => ({ id: c.id, label: c.name })), { id: 'none', label: 'No holidays' }]} />
      </Form>
    </FormDialog>
  );
}
