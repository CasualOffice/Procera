import { Download, FileX2, HardDrive, Trash2 } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type LoadFailure, type Member, memberMap, toFailure, useItems } from '../lib/data';
import { shortDate } from '../lib/format';
import { type FileInfo, downloadFile, formatBytes } from '../lib/upload';
import { useSession } from '../lib/session';
import { Button } from '../ui/components/Button';
import { Notice } from '../ui/components/Notice';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag, type TagColor } from '../ui/Tag';

interface Row { file: FileInfo; links: { object_type: string; object_id: string }[] }
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
        <div className="table-wrap">
          <table className="table" aria-label="Files">
            <thead><tr><th>Name</th><th className="col-optional">Uploaded by</th><th>Used by</th><th className="num">Size</th><th>Scan</th><th className="col-optional">Uploaded</th><th><span className="visually-hidden">Actions</span></th></tr></thead>
            <tbody>{rows.map(({ file: f, links }) => (
              <tr key={f.id}>
                <td>{f.name}<span className="cell-muted" style={{ display: 'block', fontSize: '0.75rem' }}>{f.sniffed_type ?? ''}</span></td>
                <td className="cell-muted col-optional">{people.get(f.uploaded_by)?.display_name ?? ''}</td>
                <td className="cell-muted">{links.length ? `${links.length} document version${links.length === 1 ? '' : 's'}` : 'Not attached'}</td>
                <td className="num">{formatBytes(f.size)}</td>
                <td><Tag color={STATE[f.state]?.[1] ?? 'gray'} dot>{STATE[f.state]?.[0] ?? f.state}</Tag>{f.scan_result && f.state !== 'clean' && <span className="cell-muted" style={{ display: 'block', fontSize: '0.75rem' }}>{f.scan_result}</span>}</td>
                <td className="cell-muted col-optional">{shortDate(f.created_at)}</td>
                <td className="cell-actions">
                  {f.state === 'clean' && <button type="button" className="icon-btn" aria-label={`Download ${f.name}`} onClick={() => void downloadFile(f.id)}><Download aria-hidden="true" size="0.875rem" /></button>}
                  {can('file:delete') && <button type="button" className="icon-btn" aria-label={`Delete ${f.name}`} onClick={() => void act(() => api(`/api/v1/files/${f.id}`, { method: 'DELETE' }), `${f.name} deleted.`)}><Trash2 aria-hidden="true" size="0.875rem" /></button>}
                </td>
              </tr>
            ))}</tbody>
          </table>
        </div>
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
