import { type APIRequestContext, expect } from '@playwright/test';

type Session = { c: APIRequestContext; headers: Record<string, string> };

/** The EICAR anti-malware test string (harmless; every scanner flags it). */
export const EICAR = 'X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*';

/** Uploads `bytes` in chunks of `chunk` and completes it. Returns the completed file. */
export async function upload(s: Session, name: string, bytes: Buffer, chunk = 1024 * 1024) {
  const created = await s.c.post('/api/v1/files', { headers: s.headers, data: { name, size: bytes.length } });
  expect(created.status(), await created.text()).toBe(201);
  const { file } = await created.json();
  for (let off = 0; off < bytes.length; off += chunk) {
    const r = await s.c.put(`/api/v1/files/${file.id}/content?offset=${off}`, { headers: { ...s.headers, 'content-type': 'application/octet-stream' }, data: bytes.subarray(off, off + chunk) });
    expect(r.status(), await r.text()).toBe(200);
  }
  const done = await s.c.post(`/api/v1/files/${file.id}/complete`, { headers: s.headers });
  expect(done.status(), await done.text()).toBe(200);
  return done.json();
}
