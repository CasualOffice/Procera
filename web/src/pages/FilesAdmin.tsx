import { Download, FileX2, HardDrive, Trash2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type LoadFailure, type Member, memberMap, toFailure, useItems } from '../lib/data';
import { shortDate, timeAgo } from '../lib/format';
import { type FileInfo, downloadFile, formatBytes } from '../lib/upload';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { DataGrid } from '../ui/components/DataGrid';
import { Notice } from '../ui/components/Notice';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag, type TagColor } from '../ui/Tag';

interface Row { file: FileInfo; links: { object_type: string; object_id: string }[]; retention_class: string | null }
type GridRow = Row & { id: string };
const CLASS: Record<string, string> = { audit: 'Audit', governance_record: 'Governance record', draft: 'Draft' };
interface Usage { used_bytes: number; quota_bytes: number | null; states: Record<string, number>; scanner: string; by_type: { type: string | null; files: number; bytes: number }[] }
const STATE: Record<string, [string, TagColor]> = { clean: ['Clean', 'green'], scanning: ['Scanning', 'yellow'], quarantined: ['Quarantined', 'red'], rejected: ['Rejected', 'red'], uploading: ['Uploading', 'gray'] };
const FILTERS = [['', 'All'], ['clean', 'Clean'], ['quarantined', 'Quarantined'], ['rejected', 'Rejected'], ['uploading', 'Unfinished']] as const;

/** Admin · Files (BPMS-062, FR-F06/F07): every file, its scan state and where it is used; storage use; orphans. */
export function FilesAdmin() {
  const { can } = useSession();
  const [state, setState] = useState('');
  const [rows, setRows] = useState<Row[] | null>(null);
  const [usage, setUsage] = useState<Usage | null>(null);
  const [orphans, setOrphans] = useState<FileInfo[]>([]);
  const [failed, setFailed] = useState<LoadFailure | null>(null);
  const [notice, setNotice] = useState<{ tone: 'success' | 'danger'; title: string } | null>(null);
  const [members] = useItems<Member>('/api/v1/members');
  const people = memberMap(members);
  const load = () => {
    setFailed(null);
    api<{ items: Row[] }>(`/api/v1/files${state ? `?state=${state}` : ''}`).then((r) => setRows(r.items)).catch((e) => setFailed(toFailure(e)));
    api<Usage>('/api/v1/files/usage').then(setUsage).catch(() => setUsage(null));
    api<{ items: FileInfo[] }>('/api/v1/files/orphans').then((r) => setOrphans(r.items)).catch(() => setOrphans([]));
  };
  useEffect(load, [state]);
  const act = async (fn: () => Promise<unknown>, ok: string) => {
    try { await fn(); setNotice({ tone: 'success', title: ok }); load(); } catch (e) { setNotice({ tone: 'danger', title: e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'That did not work.' }); }
  };

  if (failed) return <LoadError error={failed} onRetry={load} what="files" />;
  const pct = usage?.quota_bytes ? Math.min(100, Math.round((usage.used_bytes / usage.quota_bytes) * 100)) : null;
  return (
    <>
      <div className="page-head"><div><h1>Files</h1><p>Every file in the workspace, its malware scan result and where it is used.</p></div></div>
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      {usage && (
        <dl className="props" style={{ maxWidth: '44rem' }}>
          <dt><HardDrive aria-hidden="true" size="0.9375rem" /> Storage</dt>
          <dd>{formatBytes(usage.used_bytes)}{usage.quota_bytes ? <span className="muted">of {formatBytes(usage.quota_bytes)} ({pct}%)</span> : <span className="muted">· no quota</span>}</dd>
          <dt>Scanning</dt><dd>{usage.scanner}</dd>
        </dl>
      )}
      <div className="filter-chips" role="group" aria-label="Show" style={{ marginTop: '1rem' }}>
        {FILTERS.map(([k, label]) => <button key={k} type="button" className="filter-chip" aria-pressed={state === k} onClick={() => setState(k)}>{label}{usage && k && <span className="filter-chip-count">{usage.states[k] ?? 0}</span>}</button>)}
      </div>
      {rows === null ? <Skeleton variant="table" rows={4} label="Loading files" /> : rows.length === 0 ? (
        <Empty icon={FileX2} title="No files here">Files attached to documents show up here with their scan result.</Empty>
      ) : (
        <DataGrid<GridRow> caption="Files" showCaption={false} list="admin.files" data={rows.map((r) => ({ ...r, id: r.file.id }))} initialSort={[{ id: 'uploaded', desc: true }]}
          selectable={can('file:delete')}
          bulkActions={(sel, clear) => <Button variant="secondary" className="btn-sm" onPress={() => void act(async () => { for (const r of sel) await api(`/api/v1/files/${r.file.id}`, { method: 'DELETE' }); clear(); }, `${sel.length} file${sel.length === 1 ? '' : 's'} deleted.`)}><Trash2 aria-hidden="true" size="0.875rem" /> Delete {sel.length}</Button>}
          columns={[
            { id: 'name', header: 'Name', value: (r) => r.file.name, cell: (r) => <>{r.file.name}<span className="cell-muted" style={{ display: 'block', fontSize: '0.75rem' }}>{r.file.sniffed_type ?? ''}</span></> },
            { id: 'owner', header: 'Uploaded by', optional: true, value: (r) => people.get(r.file.uploaded_by)?.display_name ?? '' },
            { id: 'used', header: 'Used by', value: (r) => (r.links.length ? `${r.links.length} ${r.links.some((l) => l.object_type === 'audit_export') ? 'audit export' : `document version${r.links.length === 1 ? '' : 's'}`}` : 'Not attached') },
            { id: 'class', header: 'Retention class', type: 'enum', optional: true, value: (r) => CLASS[r.retention_class ?? ''] ?? 'Orphan rules' },
            { id: 'size', header: 'Size', type: 'number', value: (r) => r.file.size, cell: (r) => formatBytes(r.file.size), className: 'num' },
            { id: 'scan', header: 'Scan', type: 'enum', value: (r) => STATE[r.file.state]?.[0] ?? r.file.state, cell: (r) => <><Tag color={STATE[r.file.state]?.[1] ?? 'gray'} dot>{STATE[r.file.state]?.[0] ?? r.file.state}</Tag>{r.file.scan_result && r.file.state !== 'clean' && <span className="cell-muted" style={{ display: 'block', fontSize: '0.75rem' }}>{r.file.scan_result}</span>}</> },
            { id: 'uploaded', header: 'Uploaded', type: 'date', optional: true, value: (r) => r.file.created_at, cell: (r) => shortDate(r.file.created_at) },
            { id: 'accessed', header: 'Last accessed', type: 'date', optional: true, value: (r) => r.file.last_accessed_at ?? null, cell: (r) => (r.file.last_accessed_at ? timeAgo(r.file.last_accessed_at) : 'Never') },
            { id: 'actions', header: 'Actions', sortable: false, value: () => '', className: 'cell-actions', cell: (r) => <>
              {r.file.state === 'clean' && <button type="button" className="icon-btn" aria-label={`Download ${r.file.name}`} onClick={() => void downloadFile(r.file.id).then(load)}><Download aria-hidden="true" size="0.875rem" /></button>}
              {can('file:delete') && <button type="button" className="icon-btn" aria-label={`Delete ${r.file.name}`} onClick={() => void act(() => api(`/api/v1/files/${r.file.id}`, { method: 'DELETE' }), `${r.file.name} deleted.`)}><Trash2 aria-hidden="true" size="0.875rem" /></button>}
            </> },
          ]} />
      )}
      <section aria-labelledby="orph-h" style={{ marginTop: '2rem' }}>
        <h2 id="orph-h" style={{ fontSize: '0.9375rem', margin: '0 0 0.5rem' }}>Orphaned files</h2>
        <p className="muted" style={{ margin: '0 0 0.75rem', fontSize: '0.8125rem' }}>Files nothing uses any more, past the orphan period. They can be removed to free space.</p>
        {orphans.length === 0 ? <p className="muted" style={{ fontSize: '0.875rem' }}>None.</p> : (
          <div style={{ display: 'flex', alignItems: 'center', gap: '0.75rem' }}>
            <span>{orphans.length} file{orphans.length === 1 ? '' : 's'}, {formatBytes(orphans.reduce((a, f) => a + f.size, 0))}</span>
            {can('file:delete') && <Button variant="secondary" onPress={() => void act(() => api('/api/v1/files/orphans/purge', { method: 'POST' }), 'Orphaned files removed.')}>Remove orphaned files</Button>}
          </div>
        )}
      </section>
    </>
  );
}
