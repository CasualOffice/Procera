import { FileUp, ListChecks } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Tag } from '../ui/Tag';

interface Row { line: number; email: string; outcome: 'update' | 'unchanged' | 'error'; changes: string[]; error: string | null }
interface Report { rows: Row[]; created_departments: string[]; updated: number; errors: number }

const EXAMPLE = 'email,department,title,manager_email,location\nana@acme.test,Finance,Accountant,olivia@acme.test,Bangalore';

/**
 * Directory import (BPMS-023, FR-W02): a CSV of existing members' department, title, manager and
 * location. Check first: every row says what would change or why it can't (unknown member, manager
 * cycle…), and nothing is saved. Apply saves the rows without errors and is audited.
 */
export function DirectoryImport({ isOpen, onClose, onApplied }: { isOpen: boolean; onClose: () => void; onApplied: () => void }) {
  const [csv, setCsv] = useState('');
  const [name, setName] = useState<string | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { if (isOpen) { setCsv(''); setName(null); setReport(null); setError(null); } }, [isOpen]);

  const run = async (mode: 'plan' | 'apply') => {
    setBusy(true); setError(null);
    try {
      const r = await api<{ report: Report }>(`/api/v1/members/import?mode=${mode}`, { method: 'POST', raw: csv, headers: { 'content-type': 'text/csv' } });
      if (mode === 'apply') { onApplied(); onClose(); } else setReport(r.report);
    } catch (e) { setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'The file couldn’t be read.'); setReport(null); } finally { setBusy(false); }
  };
  const pick = async (f?: File) => { if (!f) return; setName(f.name); setReport(null); setCsv(await f.text()); };
  const ok = report ? report.rows.length - report.errors : 0;

  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} title="Import directory" width={720}
      description="Update departments, titles, managers and locations from a CSV. Check it first: nothing changes until you apply."
      footer={<>
        <Button variant="secondary" onPress={onClose}>Cancel</Button>
        {report ? <Button isDisabled={busy || ok === 0} onPress={() => void run('apply')}>Apply {ok} {ok === 1 ? 'row' : 'rows'}</Button>
          : <Button isDisabled={busy || !csv.trim()} onPress={() => void run('plan')}><ListChecks aria-hidden="true" size="1rem" /> {busy ? 'Checking…' : 'Check file'}</Button>}
      </>}>
      <div className="stack">
        {error && <Notice tone="danger" title={error} />}
        <div>
          <input ref={input} id="dir-csv" type="file" accept=".csv,text/csv" className="visually-hidden" onChange={(e) => void pick(e.target.files?.[0])} />
          <label htmlFor="dir-csv" className="btn btn-secondary btn-md" role="button" tabIndex={0} onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && input.current?.click()}><FileUp aria-hidden="true" size="0.9375rem" /> {name ? `Replace ${name}` : 'Choose a CSV file'}</label>
          <p className="field-description">Columns: <code>email</code> (required), <code>department</code>, <code>title</code>, <code>manager_email</code>, <code>location</code>. Example: <code style={{ whiteSpace: 'pre-wrap' }}>{EXAMPLE}</code></p>
        </div>
        {report && (
          <section aria-label="Import check">
            <p role="status">{report.updated} to update · {report.rows.length - report.updated - report.errors} unchanged · {report.errors} with errors{report.created_departments.length ? ` · new departments: ${report.created_departments.join(', ')}` : ''}</p>
            {report.errors > 0 && <Notice tone="warning" title="Rows with errors are skipped">Fix them in the file and check again, or apply the others now.</Notice>}
            <div className="table-wrap" style={{ maxHeight: '18rem', overflow: 'auto' }}>
              <table className="table" aria-label="Rows">
                <thead><tr><th className="num">Line</th><th>Email</th><th>Result</th><th>Details</th></tr></thead>
                <tbody>{report.rows.map((r) => (
                  <tr key={r.line}>
                    <td className="num">{r.line}</td>
                    <td>{r.email}</td>
                    <td>{r.outcome === 'error' ? <Tag color="red" dot>Error</Tag> : r.outcome === 'update' ? <Tag color="blue" dot>Update</Tag> : <Tag color="gray">Unchanged</Tag>}</td>
                    <td className="cell-muted">{r.error ?? r.changes.join('; ')}</td>
                  </tr>
                ))}</tbody>
              </table>
            </div>
          </section>
        )}
      </div>
    </FormDialog>
  );
}
