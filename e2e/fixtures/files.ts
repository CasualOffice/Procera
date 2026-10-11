import { type APIRequestContext, expect } from '@playwright/test';

type Session = { c: APIRequestContext; headers: Record<string, string> };

/** The EICAR anti-malware test string (harmless; every scanner flags it). */
export const EICAR = 'X5O!P%@AP[4\\PZX54(P^)7CC)7}$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*';

/** A valid 1×1 PNG. */
export const PNG_1PX = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==', 'base64');

/** A valid one-page PDF that says `text` (correct xref offsets, so viewers open it without repair). */
export function tinyPdf(text = 'Hello preview'): Buffer {
  const stream = `BT /F1 18 Tf 20 50 Td (${text}) Tj ET`;
  const objs = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    '<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
    '<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>',
    `<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`,
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>',
  ];
  let out = '%PDF-1.4\n';
  const offsets: number[] = [];
  objs.forEach((o, i) => { offsets.push(out.length); out += `${i + 1} 0 obj\n${o}\nendobj\n`; });
  const xref = out.length;
  out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n${offsets.map((o) => `${String(o).padStart(10, '0')} 00000 n \n`).join('')}`;
  out += `trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(out, 'latin1');
}

/** A structurally valid JPEG header carrying an Exif segment with a fake GPS position (for EXIF stripping). */
export function jpegWithExif(): Buffer {
  const seg = (marker: number, body: Buffer) => Buffer.concat([Buffer.from([0xff, marker, (body.length + 2) >> 8, (body.length + 2) & 0xff]), body]);
  return Buffer.concat([
    Buffer.from([0xff, 0xd8]),
    seg(0xe0, Buffer.from('JFIF\0\x01\x01\0\0\x01\0\x01\0\0', 'latin1')),
    seg(0xe1, Buffer.from('Exif\0\0GPS 51.5007N 0.1246W', 'latin1')),
    seg(0xdb, Buffer.alloc(65)),
    seg(0xda, Buffer.from([1, 1, 0, 0, 0x3f, 0])),
    Buffer.from([0x12, 0x34, 0x56, 0xff, 0xd9]),
  ]);
}

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
