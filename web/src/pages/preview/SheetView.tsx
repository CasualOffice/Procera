import { useEffect, useRef, useState } from 'react';
import { Tabs } from '../../ui/components/Tabs';
import { parseCsv } from './csv';

const MAX_ROWS = 500;
const MAX_COLS = 100;

interface Sheet { name: string; rows: string[][]; truncated: boolean }

/** Column letters as a spreadsheet shows them: A … Z, AA … */
const letters = (n: number) => { let s = ''; for (let i = n + 1; i > 0; i = Math.floor((i - 1) / 26)) s = String.fromCharCode(65 + ((i - 1) % 26)) + s; return s; };

async function readSheets(data: ArrayBuffer, kind: 'xlsx' | 'csv', name: string): Promise<Sheet[]> {
  if (kind === 'csv') return [{ name, ...parseCsv(new TextDecoder().decode(data), MAX_ROWS) }];
  // SheetJS loads only for workbooks. Formulas, styles, macros and HTML are not read: values only.
  const { read, utils } = await import('xlsx');
  const wb = read(new Uint8Array(data), { type: 'array', sheetRows: MAX_ROWS + 1, cellFormula: false, cellHTML: false, cellStyles: false, bookVBA: false });
  return wb.SheetNames.map((n) => {
    const rows = utils.sheet_to_json<string[]>(wb.Sheets[n]!, { header: 1, raw: false, defval: '', blankrows: true });
    return { name: n, rows: rows.slice(0, MAX_ROWS), truncated: rows.length > MAX_ROWS };
  });
}

/** Read-only table preview of a workbook (every sheet, switchable) or a CSV file (BPMS-061). */
export default function SheetView({ data, kind, name, onError }: { data: ArrayBuffer; kind: 'xlsx' | 'csv'; name: string; onError: (m: string) => void }) {
  const [sheets, setSheets] = useState<Sheet[] | null>(null);
  const fail = useRef(onError);
  fail.current = onError;
  useEffect(() => {
    let live = true;
    readSheets(data, kind, name)
      .then((s) => { if (live) setSheets(s); })
      .catch(() => { if (live) fail.current('This spreadsheet could not be displayed. Download it to open it.'); });
    return () => { live = false; };
  }, [data, kind, name]);

  if (!sheets) return <p className="muted">Reading the spreadsheet…</p>;
  if (sheets.length === 1) return <SheetTable sheet={sheets[0]!} />;
  return <div className="sheet-view"><Tabs label="Sheets" tabs={sheets.map((s) => ({ id: s.name, label: s.name, content: <SheetTable sheet={s} /> }))} /></div>;
}

function SheetTable({ sheet }: { sheet: Sheet }) {
  const cols = Math.min(MAX_COLS, Math.max(1, ...sheet.rows.map((r) => r.length)));
  return (
    <div className="sheet-view">
      <p className="muted sheet-note">
        {sheet.truncated ? `Read-only. Showing the first ${MAX_ROWS} rows; download the file to see them all.` : `Read-only. ${sheet.rows.length} row${sheet.rows.length === 1 ? '' : 's'}.`}
      </p>
      <div className="sheet-scroll" tabIndex={0} role="region" aria-label={`${sheet.name} table`}>
        <table className="sheet-table" aria-label={sheet.name}>
          <thead><tr><td />{Array.from({ length: cols }, (_, c) => <th key={c} scope="col">{letters(c)}</th>)}</tr></thead>
          <tbody>
            {sheet.rows.map((r, i) => (
              <tr key={i}><th scope="row">{i + 1}</th>{Array.from({ length: cols }, (_, c) => <td key={c}>{r[c] ?? ''}</td>)}</tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
