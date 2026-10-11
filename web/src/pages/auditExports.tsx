import { Download, FileDown } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { shortDate } from '../lib/format';
import { downloadFile } from '../lib/upload';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { StepUpDialog } from '../ui/StepUp';
import { Tag, type TagColor } from '../ui/Tag';

export interface ExportFilters { category: string; result: string; action: string; from: string; to: string }
interface Job { id: string; format: string; state: 'queued' | 'running' | 'done' | 'failed'; rows: number | null; file_id: string | null; error: string | null; created_at: string; finished_at: string | null }
const STATE: Record<Job['state'], [string, TagColor]> = { queued: ['Queued', 'gray'], running: ['Preparing', 'yellow'], done: ['Ready', 'green'], failed: ['Failed', 'red'] };

function apiFilters(f: ExportFilters) {
  return {
    ...(f.category ? { category: f.category } : {}),
    ...(f.result ? { result: f.result } : {}),
    ...(f.action.trim() ? { action: f.action.trim() } : {}),
    ...(f.from ? { from: new Date(`${f.from}T00:00:00`).toISOString() } : {}),
    ...(f.to ? { to: new Date(`${f.to}T00:00:00`).toISOString() } : {}),
  };
}

/**
 * Background audit exports (BPMS-064): any size, prepared on the server with the current filters,
 * stored encrypted and downloaded through a signed link. Needs a recent sign-in.
 */
export function AuditExports({ filters }: { filters: ExportFilters }) {
  const [jobs, setJobs] = useState<Job[] | null>(null);
  const [asking, setAsking] = useState(false);
  const load = useCallback(() => { api<{ items: Job[] }>('/api/v1/audit/exports').then((r) => setJobs(r.items)).catch(() => setJobs([])); }, []);
  useEffect(load, [load]);
  // Poll while something is being prepared.
  useEffect(() => {
    if (!jobs?.some((j) => j.state === 'queued' || j.state === 'running')) return;
    const t = window.setInterval(load, 3000);
    return () => window.clearInterval(t);
  }, [jobs, load]);
  return (
    <>
      <Button variant="secondary" onPress={() => setAsking(true)}><FileDown aria-hidden="true" size="1rem" /> Export</Button>
      {asking && <ExportDialog filters={filters} onClose={() => setAsking(false)} onQueued={() => { setAsking(false); load(); }} />}
      {jobs && jobs.length > 0 && (
        <section className="audit-exports" aria-labelledby="exports-h">
          <h2 id="exports-h">Your exports</h2>
          <ul>
            {jobs.slice(0, 5).map((j) => (
              <li key={j.id}>
                <span>{j.format.toUpperCase()} · requested {shortDate(j.created_at)}</span>
                <Tag color={STATE[j.state][1]} dot>{STATE[j.state][0]}</Tag>
                {j.state === 'done' && <span className="muted">{j.rows?.toLocaleString()} events</span>}
                {j.state === 'failed' && <span className="muted">{j.error}</span>}
                {j.state === 'done' && j.file_id && <button type="button" className="prop-link" onClick={() => void downloadFile(j.file_id!)} aria-label={`Download export from ${shortDate(j.created_at)}`}><Download aria-hidden="true" size="0.875rem" /> Download</button>}
              </li>
            ))}
          </ul>
        </section>
      )}
    </>
  );
}

function ExportDialog({ filters, onClose, onQueued }: { filters: ExportFilters; onClose: () => void; onQueued: () => void }) {
  const [format, setFormat] = useState<'csv' | 'jsonl'>('csv');
  const [error, setError] = useState<string | null>(null);
  const [stepUp, setStepUp] = useState(false);
  const start = async () => {
    setError(null);
    try {
      await api('/api/v1/audit/exports', { method: 'POST', body: { format, filters: apiFilters(filters), background: true } });
      onQueued();
    } catch (e) {
      if (e instanceof ApiError && e.code === 'step_up_required') setStepUp(true);
      else setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'Could not start the export.');
    }
  };
  const active = Object.values(apiFilters(filters)).length;
  return (
    <>
      <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Export the audit log" description={`Uses ${active ? 'the current filters' : 'every event (no filters set)'}. It’s prepared in the background and you’ll get a notification when it’s ready. The export itself is recorded in the audit log.`}
        footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void start()}>Start export</Button></>}>
        {error && <Notice tone="danger" title={error} />}
        <div className="radio-cards" role="radiogroup" aria-label="Format">
          {([['csv', 'CSV', 'Opens in spreadsheets'], ['jsonl', 'JSON Lines', 'For tools and SIEMs']] as const).map(([k, label, hint]) => (
            <button key={k} type="button" role="radio" className="radio-card" aria-checked={format === k} onClick={() => setFormat(k)}><span><strong>{label}</strong><small>{hint}</small></span></button>
          ))}
        </div>
      </FormDialog>
      <StepUpDialog isOpen={stepUp} onClose={() => setStepUp(false)} onDone={() => { setStepUp(false); void start(); }} />
    </>
  );
}
