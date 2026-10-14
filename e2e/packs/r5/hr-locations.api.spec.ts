import { type APIRequestContext, expect, test } from '@playwright/test';
import { PW, client, hasDatabase } from '../../fixtures/accounts';
import { type Session, enter, inviteToken, workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R5-HR-01 — work locations and holiday calendars (BPMS-222, docs/25 §2.1, §3): an owner makes a
// calendar, imports CSV (a bad line is reported, not fatal) and iCal (a multi-day event becomes one
// holiday a day), adds and removes a holiday, copies a year; makes a location (unknown time zones
// and empty weeks refused) on that calendar; a member placed there sees its holidays; a free-text
// directory location becomes a work location; renaming a location renames it on profiles; a
// location people work at can't be archived and a calendar in use can't be deleted; members can't
// change any of it.

const year = new Date().getUTCFullYear() + 1;

async function member(request: APIRequestContext, owner: Session, ws: string, name: string) {
  const email = `${name.toLowerCase()}-${Date.now().toString(36)}${Math.random().toString(36).slice(2, 5)}@acme.test`;
  expect((await owner.c.post('/api/v1/members/invitations', { headers: owner.headers, data: { email, roles: ['member'] } })).ok()).toBe(true);
  const token = await inviteToken(request, ws, email);
  expect((await (await client()).post('/api/v1/invitations/register', { data: { token, display_name: name, password: PW } })).status()).toBe(201);
  const list = await (await owner.c.get('/api/v1/members', { headers: owner.headers })).json();
  return { email, id: (list.items as { id: string; email: string }[]).find((m) => m.email === email)!.id };
}

test.describe('holidays and locations @r5 @hr', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R5-HR-01 calendars, imports, locations and a member’s holidays', async ({ request }) => {
    const { ws, owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const cal = await (await owner.c.post('/api/v1/holiday-calendars', { headers: h, data: { name: 'India' } })).json();
    expect(cal).toMatchObject({ name: 'India', locations: [], years: [] });
    expect((await owner.c.post('/api/v1/holiday-calendars', { headers: h, data: { name: 'india' } })).status()).toBe(409);

    // CSV import: header, quotes, optional, and a bad date reported by line.
    const csv = `date,name,optional\n${year}-01-26,Republic Day\n${year}-03-04,"Holi, colours",yes\n26/01/${year},Bad line`;
    const imp = await (await owner.c.post(`/api/v1/holiday-calendars/${cal.id}/import`, { headers: h, data: { text: csv } })).json();
    expect(imp).toMatchObject({ added: 2, updated: 0, skipped: [expect.objectContaining({ line: 4, reason: expect.stringContaining('2026-10-02') })] });
    // iCal: a three-day event becomes three holidays; an existing date is renamed.
    const ics = ['BEGIN:VCALENDAR', 'BEGIN:VEVENT', `DTSTART;VALUE=DATE:${year}1224`, `DTEND;VALUE=DATE:${year}1227`, 'SUMMARY:Year-end break', 'END:VEVENT',
      'BEGIN:VEVENT', `DTSTART;VALUE=DATE:${year}0126`, 'SUMMARY:Republic Day (India)', 'END:VEVENT', 'END:VCALENDAR'].join('\r\n');
    expect(await (await owner.c.post(`/api/v1/holiday-calendars/${cal.id}/import`, { headers: h, data: { text: ics } })).json()).toMatchObject({ added: 3, updated: 1, skipped: [] });
    expect((await owner.c.put(`/api/v1/holiday-calendars/${cal.id}/holidays/${year}-10-02`, { headers: h, data: { name: 'Gandhi Jayanti' } })).status()).toBe(201);
    expect((await owner.c.put(`/api/v1/holiday-calendars/${cal.id}/holidays/${year}-10-02`, { headers: h, data: { name: 'Gandhi Jayanti', optional: false } })).status()).toBe(200);
    expect((await owner.c.put(`/api/v1/holiday-calendars/${cal.id}/holidays/02-10-${year}`, { headers: h, data: { name: 'X' } })).status()).toBe(422);
    expect((await owner.c.delete(`/api/v1/holiday-calendars/${cal.id}/holidays/${year}-12-26`, { headers: h })).status()).toBe(204);
    const hols = (await (await owner.c.get(`/api/v1/holiday-calendars/${cal.id}/holidays?year=${year}`, { headers: h })).json()).items;
    expect(hols).toEqual([
      { date: `${year}-01-26`, name: 'Republic Day (India)', optional: false },
      { date: `${year}-03-04`, name: 'Holi, colours', optional: true },
      { date: `${year}-10-02`, name: 'Gandhi Jayanti', optional: false },
      { date: `${year}-12-24`, name: 'Year-end break', optional: false },
      { date: `${year}-12-25`, name: 'Year-end break', optional: false },
    ]);
    const copied = await (await owner.c.post(`/api/v1/holiday-calendars/${cal.id}/copy-year`, { headers: h, data: { from: year, to: year + 1 } })).json();
    expect(copied.added).toBe(5);
    expect((await (await owner.c.get(`/api/v1/holiday-calendars/${cal.id}`, { headers: h })).json()).years.sort()).toEqual([year, year + 1]);

    // Locations.
    expect((await owner.c.post('/api/v1/work-locations', { headers: h, data: { name: 'Bangalore', timezone: 'Mars/Olympus' } })).status()).toBe(422);
    expect((await owner.c.post('/api/v1/work-locations', { headers: h, data: { name: 'Bangalore', timezone: 'Asia/Kolkata', work_week: [] } })).status()).toBe(422);
    const blr = await (await owner.c.post('/api/v1/work-locations', { headers: h, data: { name: 'Bangalore', timezone: 'Asia/Kolkata', work_week: [1, 2, 3, 4, 5, 6], holiday_calendar_id: cal.id } })).json();
    expect(blr).toMatchObject({ name: 'Bangalore', timezone: 'Asia/Kolkata', work_week: [1, 2, 3, 4, 5, 6], day_hours: 8, holiday_calendar: 'India', members: 0 });

    // A member placed at Bangalore: their profile says so, and they see its holidays.
    const ann = await member(request, owner, ws.id, 'Ann');
    const set = await owner.c.put(`/api/v1/members/${ann.id}/hr`, { headers: h, data: { employee_no: 'E-001', employment_type: 'full_time', work_location_id: blr.id, start_date: `${year}-01-05` } });
    expect(set.status(), await set.text()).toBe(200);
    expect((await (await owner.c.get(`/api/v1/members/${ann.id}/directory`, { headers: h })).json()).location).toBe('Bangalore');
    const a = await enter(ann.email, ws.id);
    const mine = await (await a.c.get(`/api/v1/me/holidays?from=${year}-01-01&to=${year}-12-31`, { headers: a.headers })).json();
    expect(mine.location).toMatchObject({ name: 'Bangalore', timezone: 'Asia/Kolkata' });
    expect(mine.items.map((x: { date: string }) => x.date)).toEqual([`${year}-01-26`, `${year}-03-04`, `${year}-10-02`, `${year}-12-24`, `${year}-12-25`]);

    // Members can't change locations or holidays, or someone's HR record.
    expect((await a.c.post('/api/v1/work-locations', { headers: a.headers, data: { name: 'Mine', timezone: 'UTC' } })).status()).toBe(403);
    expect((await a.c.put(`/api/v1/holiday-calendars/${cal.id}/holidays/${year}-05-01`, { headers: a.headers, data: { name: 'May Day' } })).status()).toBe(403);
    expect((await a.c.put(`/api/v1/members/${ann.id}/hr`, { headers: a.headers, data: {} })).status()).toBe(403);

    // A free-text directory location becomes a work location.
    const bo = await member(request, owner, ws.id, 'Bo');
    expect((await owner.c.patch(`/api/v1/members/${bo.id}`, { headers: h, data: { location: 'Pune' } })).ok()).toBe(true);
    const locs = (await (await owner.c.get('/api/v1/work-locations', { headers: h })).json()).items;
    const pune = locs.find((l: { name: string }) => l.name === 'Pune');
    expect(pune).toMatchObject({ timezone: 'UTC', work_week: [1, 2, 3, 4, 5], members: 1 });
    expect((await (await owner.c.get(`/api/v1/members/${bo.id}/hr`, { headers: h })).json()).work_location_id).toBe(pune.id);
    // Duplicate employee numbers are refused.
    expect((await owner.c.put(`/api/v1/members/${bo.id}/hr`, { headers: h, data: { employee_no: 'e-001', work_location_id: pune.id } })).status()).toBe(409);

    // Renaming Bangalore renames it on Ann's profile; it can't be archived while she works there.
    const one = await owner.c.get(`/api/v1/work-locations/${blr.id}`, { headers: h });
    const etag = one.headers().etag!;
    const body = { name: 'Bengaluru', timezone: 'Asia/Kolkata', work_week: [1, 2, 3, 4, 5, 6], holiday_calendar_id: cal.id };
    expect((await owner.c.patch(`/api/v1/work-locations/${blr.id}`, { headers: { ...h, 'if-match': '"999"' }, data: body })).status()).toBe(412);
    expect((await owner.c.patch(`/api/v1/work-locations/${blr.id}`, { headers: { ...h, 'if-match': etag }, data: body })).status()).toBe(200);
    expect((await (await owner.c.get(`/api/v1/members/${ann.id}/directory`, { headers: h })).json()).location).toBe('Bengaluru');
    const archive = await owner.c.patch(`/api/v1/work-locations/${blr.id}`, { headers: h, data: { ...body, archived: true } });
    expect(archive.status()).toBe(409);
    expect((await archive.json()).detail).toContain('1 person works at Bengaluru');
    expect((await owner.c.delete(`/api/v1/holiday-calendars/${cal.id}`, { headers: h })).status()).toBe(409);
  });
});
