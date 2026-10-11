import { type APIRequestContext, expect } from '@playwright/test';
import type { Session } from './workspace';

type Req = APIRequestContext;

/** A published SOP with an active, all-hands campaign moved 30 days into the past (so it is overdue). */
export async function overdueCampaign(request: Req, owner: Session, ws: string) {
  const h = owner.headers;
  const sop = await (await owner.c.post('/api/v1/artifacts', { headers: h, data: { kind: 'sop', key: 'ops.badges', title: 'Badge rules', content: { blocks: [{ type: 'paragraph', text: 'Wear it.' }] } } })).json();
  await owner.c.post(`/api/v1/versions/${sop.version.id}/submit`, { headers: h });
  await owner.c.post(`/api/v1/versions/${sop.version.id}/publish`, { headers: h, data: {} });
  const cmp = await (await owner.c.post('/api/v1/campaigns', { headers: h, data: { version_id: sop.version.id, audience: { all: true } } })).json();
  expect((await owner.c.post(`/api/v1/campaigns/${cmp.id}/activate`, { headers: h })).ok()).toBe(true);
  expect((await request.post('/__test/campaigns/backdate', { data: { workspace: ws, campaign: cmp.id, seconds: 60 * 86_400 } })).status()).toBe(204);
  return { sop, cmp };
}
