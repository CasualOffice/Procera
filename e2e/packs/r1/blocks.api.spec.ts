import { expect, test } from '@playwright/test';
import { hasDatabase } from '../../fixtures/accounts';
import { workspaceWithOwner } from '../../fixtures/workspace';

// E2E-R1-LIB-14 — the block model is validated and exported (BPMS-042; ADR-0013).

test.describe('block content @r1 @library', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await hasDatabase(request)), 'needs a database-backed e2e build');
  });

  test('E2E-R1-LIB-14 rich blocks save; unsafe or unknown blocks are refused; HTML, Markdown and Word exports are faithful, superseded versions say so, exports are audited', async ({ request }) => {
    const { owner } = await workspaceWithOwner(request);
    const h = owner.headers;
    const content = { blocks: [
      { type: 'heading', level: 1, text: 'Purpose' },
      { type: 'paragraph', text: 'Read the badge policy first.', runs: [{ text: 'Read the ' }, { text: 'badge policy', bold: true, href: '/library/badges' }, { text: ' first.' }] },
      { type: 'list', ordered: true, items: ['Open the door', 'Sign the log'] },
      { type: 'callout', tone: 'warning', text: 'Never prop the door.' },
      { type: 'table', rows: [['Role', 'Does'], ['Guard', 'Checks <badges>']] },
    ] };
    const r = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'sec.doors', title: 'Door & access', content } })).json();
    for (const bad of [
      { blocks: [{ type: 'script', text: 'x' }] },
      { blocks: [{ type: 'paragraph', text: 'x', runs: [{ text: 'x', href: 'javascript:alert(1)' }] }] },
      { blocks: [{ type: 'paragraph', text: 'abc', runs: [{ text: 'ab' }] }] },
      { text: 'not blocks' },
    ]) {
      expect((await owner.c.patch(`/api/v1/versions/${r.version.id}`, { headers: h, data: { content: bad } })).status()).toBe(422);
    }
    await owner.c.post(`/api/v1/versions/${r.version.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${r.version.id}/publish`, { headers: h, data: {} });

    const html = await owner.c.get(`/api/v1/versions/${r.version.id}/export?format=html`, { headers: h });
    expect(html.headers()['content-type']).toContain('text/html');
    const page = await html.text();
    expect(page).toContain('<title>Door &amp; access · v1</title>');
    expect(page).toContain('<h2>Purpose</h2>');
    expect(page).toContain('<a href="/library/badges"><strong>badge policy</strong></a>');
    expect(page).toContain('<ol><li>Open the door</li><li>Sign the log</li></ol>');
    expect(page).toContain('<aside class="callout warning" aria-label="Warning">');
    expect(page).toContain('<th scope="col">Role</th>');
    expect(page).toContain('Checks &lt;badges&gt;');
    expect(page).not.toContain('Superseded');

    const md = await (await owner.c.get(`/api/v1/versions/${r.version.id}/export?format=md`, { headers: h })).text();
    expect(md).toContain('# Door & access\n\n## Purpose\n\nRead the [**badge policy**](/library/badges) first.\n\n1. Open the door\n2. Sign the log\n\n> **Warning:** Never prop the door.\n\n| Role | Does |');
    // Word: a real .docx package (structure is checked in the renderer's unit tests).
    const docx = await owner.c.get(`/api/v1/versions/${r.version.id}/export?format=docx`, { headers: h });
    expect(docx.status()).toBe(200);
    expect(docx.headers()['content-type']).toBe('application/vnd.openxmlformats-officedocument.wordprocessingml.document');
    expect(docx.headers()['content-disposition']).toMatch(/attachment; filename=".*-v1\.docx"/);
    const bytes = await docx.body();
    expect(bytes.subarray(0, 2).toString('latin1')).toBe('PK');
    expect(Buffer.compare(bytes, await (await owner.c.get(`/api/v1/versions/${r.version.id}/export?format=docx`, { headers: h })).body())).toBe(0); // deterministic
    expect((await owner.c.get(`/api/v1/versions/${r.version.id}/export?format=pdfx`, { headers: h })).status()).toBe(422);

    // A newer version supersedes v1: its export carries the banner.
    const v2 = await (await owner.c.post(`/api/v1/artifacts/${r.artifact.id}/versions`, { headers: h })).json();
    await owner.c.post(`/api/v1/versions/${v2.id}/submit`, { headers: h });
    await owner.c.post(`/api/v1/versions/${v2.id}/publish`, { headers: h, data: { change_reason: 'Refresh' } });
    expect(await (await owner.c.get(`/api/v1/versions/${r.version.id}/export?format=html`, { headers: h })).text()).toContain('<strong>Superseded.</strong> This is version 1, not the current version.');
    const audit = await (await owner.c.get('/api/v1/audit/events?action=version.exported', { headers: h })).json();
    expect(audit.items.length).toBeGreaterThanOrEqual(5);
  });
});
