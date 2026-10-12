/**
 * A small RFC 4180 CSV reader for previews: quoted fields, doubled quotes, CRLF/LF, a leading BOM,
 * and `;` as the separator when the first line has more semicolons than commas. Stops after `max`
 * rows and says whether there were more.
 */
export function parseCsv(input: string, max: number): { rows: string[][]; truncated: boolean } {
  const text = input.charCodeAt(0) === 0xfeff ? input.slice(1) : input;
  const first = text.slice(0, text.search(/\r?\n|$/));
  const sep = (first.match(/;/g)?.length ?? 0) > (first.match(/,/g)?.length ?? 0) ? ';' : ',';
  const rows: string[][] = [];
  let row: string[] = [];
  let field = '';
  let quoted = false;
  const endRow = () => { row.push(field); rows.push(row); row = []; field = ''; };
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c !== '"') field += c;
      else if (text[i + 1] === '"') { field += '"'; i++; }
      else quoted = false;
    } else if (c === '"' && field === '') quoted = true;
    else if (c === sep) { row.push(field); field = ''; }
    else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i++;
      endRow();
      if (rows.length > max) return { rows: rows.slice(0, max), truncated: true };
    } else field += c;
  }
  if (field !== '' || row.length) endRow();
  return { rows: rows.slice(0, max), truncated: rows.length > max };
}
