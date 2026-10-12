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

/** A valid PDF with one page per entry of `texts`, each page saying its text. */
export function pdfPages(texts: string[]): Buffer {
  const objs = ['<< /Type /Catalog /Pages 2 0 R >>', '', '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>'];
  const kids: string[] = [];
  for (const t of texts) {
    const stream = `BT /F1 18 Tf 20 50 Td (${t}) Tj ET`;
    const page = objs.length + 1;
    kids.push(`${page} 0 R`);
    objs.push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 100] /Contents ${page + 1} 0 R /Resources << /Font << /F1 3 0 R >> >> >>`);
    objs.push(`<< /Length ${stream.length} >>\nstream\n${stream}\nendstream`);
  }
  objs[1] = `<< /Type /Pages /Kids [${kids.join(' ')}] /Count ${texts.length} >>`;
  let out = '%PDF-1.4\n';
  const offsets: number[] = [];
  objs.forEach((o, i) => { offsets.push(out.length); out += `${i + 1} 0 obj\n${o}\nendobj\n`; });
  const xref = out.length;
  out += `xref\n0 ${objs.length + 1}\n0000000000 65535 f \n${offsets.map((o) => `${String(o).padStart(10, '0')} 00000 n \n`).join('')}`;
  out += `trailer\n<< /Size ${objs.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  return Buffer.from(out, 'latin1');
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (b: Buffer) => { let c = 0xffffffff; for (const x of b) c = CRC_TABLE[(c ^ x) & 0xff]! ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };

/** A stored (uncompressed) zip of these parts, in order. Enough for Office Open XML fixtures. */
export function zipStored(parts: [string, string][]): Buffer {
  const locals: Buffer[] = [];
  const central: Buffer[] = [];
  let offset = 0;
  for (const [path, body] of parts) {
    const name = Buffer.from(path, 'utf8');
    const data = Buffer.from(body, 'utf8');
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt16LE(33, 12); // 1980-01-01
    local.writeUInt32LE(crc, 14); local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22); local.writeUInt16LE(name.length, 26);
    const dir = Buffer.alloc(46);
    dir.writeUInt32LE(0x02014b50, 0); dir.writeUInt16LE(20, 4); dir.writeUInt16LE(20, 6); dir.writeUInt16LE(33, 14);
    dir.writeUInt32LE(crc, 16); dir.writeUInt32LE(data.length, 20); dir.writeUInt32LE(data.length, 24); dir.writeUInt16LE(name.length, 28); dir.writeUInt32LE(offset, 42);
    locals.push(local, name, data);
    central.push(dir, name);
    offset += 30 + name.length + data.length;
  }
  const cd = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(parts.length, 8); end.writeUInt16LE(parts.length, 10); end.writeUInt32LE(cd.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, end]);
}

const xml = (s: string) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships';
const PKG_REL = 'http://schemas.openxmlformats.org/package/2006/relationships';
const TYPES = 'http://schemas.openxmlformats.org/package/2006/content-types';
const DEFAULTS = '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/>';
const rels = (items: [string, string, string][]) => `<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="${PKG_REL}">${items.map(([id, type, target]) => `<Relationship Id="${id}" Type="${REL}/${type}" Target="${target}"${target.startsWith('http') ? ' TargetMode="External"' : ''}/>`).join('')}</Relationships>`;

/** A minimal Word document: one paragraph per entry, then an external hyperlink saying `linkText`. */
export function tinyDocx(paragraphs: string[], linkText = 'Visit the site'): Buffer {
  const body = paragraphs.map((p) => `<w:p><w:r><w:t>${xml(p)}</w:t></w:r></w:p>`).join('');
  return zipStored([
    ['[Content_Types].xml', `<?xml version="1.0" encoding="UTF-8"?><Types xmlns="${TYPES}">${DEFAULTS}<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>`],
    ['_rels/.rels', rels([['rId1', 'officeDocument', 'word/document.xml']])],
    ['word/_rels/document.xml.rels', rels([['rId9', 'hyperlink', 'https://example.com/']])],
    ['word/document.xml', `<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="${REL}"><w:body>${body}<w:p><w:hyperlink r:id="rId9"><w:r><w:t>${xml(linkText)}</w:t></w:r></w:hyperlink></w:p><w:sectPr/></w:body></w:document>`],
  ]);
}

/** A minimal workbook with these sheets (inline strings, no shared-strings part). */
export function tinyXlsx(sheets: Record<string, string[][]>): Buffer {
  const names = Object.keys(sheets);
  const cell = (v: string, i: number, j: number) => `<c r="${String.fromCharCode(65 + j)}${i + 1}" t="inlineStr"><is><t>${xml(v)}</t></is></c>`;
  const sheetXml = (rows: string[][]) => `<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>${rows.map((r, i) => `<row r="${i + 1}">${r.map((v, j) => cell(v, i, j)).join('')}</row>`).join('')}</sheetData></worksheet>`;
  return zipStored([
    ['[Content_Types].xml', `<?xml version="1.0" encoding="UTF-8"?><Types xmlns="${TYPES}">${DEFAULTS}<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>${names.map((_, i) => `<Override PartName="/xl/worksheets/sheet${i + 1}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>`).join('')}</Types>`],
    ['_rels/.rels', rels([['rId1', 'officeDocument', 'xl/workbook.xml']])],
    ['xl/workbook.xml', `<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="${REL}"><sheets>${names.map((n, i) => `<sheet name="${xml(n)}" sheetId="${i + 1}" r:id="rId${i + 1}"/>`).join('')}</sheets></workbook>`],
    ['xl/_rels/workbook.xml.rels', rels(names.map((_, i) => [`rId${i + 1}`, 'worksheet', `worksheets/sheet${i + 1}.xml`]))],
    ...names.map((n, i): [string, string] => [`xl/worksheets/sheet${i + 1}.xml`, sheetXml(sheets[n]!)]),
  ]);
}
