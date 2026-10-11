import { Download, Eye, Paperclip, ShieldAlert, Trash2, Upload } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type FileInfo, downloadFile, formatBytes, uploadFile } from '../lib/upload';
import { Notice } from '../ui/components/Notice';
import { Tag, type TagColor } from '../ui/Tag';
import { FilePreview, canPreview } from './FilePreview';

const STATE: Record<string, [string, TagColor]> = { clean: ['Clean', 'green'], scanning: ['Scanning', 'yellow'], quarantined: ['Quarantined', 'red'], rejected: ['Rejected', 'red'], uploading: ['Uploading', 'gray'] };

/**
 * Files attached to a document version (BPMS-060, FR-F01/F05). Drafts take new attachments; once a
 * version moves past draft its attachments are fixed. Downloads use a short-lived signed link.
 */
export function Attachments({ versionId, canEdit }: { versionId: string; canEdit: boolean }) {
  const [items, setItems] = useState<FileInfo[] | null>(null);
  const [editable, setEditable] = useState(false);
  const [progress, setProgress] = useState<number | null>(null);
  const [notice, setNotice] = useState<{ tone: 'danger' | 'success'; title: string } | null>(null);
  const [previewing, setPreviewing] = useState<FileInfo | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const load = () => { api<{ items: FileInfo[]; editable: boolean }>(`/api/v1/versions/${versionId}/attachments`).then((r) => { setItems(r.items); setEditable(r.editable); }).catch(() => setItems([])); };
  useEffect(load, [versionId]);
  const fail = (e: unknown, fallback: string) => setNotice({ tone: 'danger', title: e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : fallback });

  const onPick = async (f: File | undefined) => {
    if (!f) return;
    setNotice(null);
    setProgress(0);
    try {
      const done = await uploadFile(f, setProgress);
      if (done.state === 'clean') {
        await api(`/api/v1/files/${done.id}/links`, { method: 'POST', body: { object_type: 'artifact_version', object_id: versionId } });
        setNotice({ tone: 'success', title: `${f.name} attached.` });
      } else if (done.state === 'quarantined') {
        setNotice({ tone: 'danger', title: `${f.name} was quarantined: the malware scan flagged it. It was not attached.` });
      } else if (done.state === 'rejected') {
        setNotice({ tone: 'danger', title: `${f.name} is a type that can’t be uploaded (${done.sniffed_type ?? 'unknown'}).` });
      } else {
        setNotice({ tone: 'danger', title: `${f.name} couldn’t be scanned yet. Try attaching it again in a few minutes.` });
      }
      load();
    } catch (e) { fail(e, 'The upload failed.'); } finally { setProgress(null); if (input.current) input.current.value = ''; }
  };

  if (items === null) return null;
  const mayEdit = canEdit && editable;
  if (!items.length && !mayEdit) return null;
  return (
    <section className="attachments" aria-labelledby="att-h">
      <div className="attachments-head">
        <h2 id="att-h"><Paperclip aria-hidden="true" size="0.9375rem" /> Attachments</h2>
        {mayEdit && (
          <>
            <input ref={input} id="att-input" type="file" className="visually-hidden" onChange={(e) => void onPick(e.target.files?.[0])} />
            <label htmlFor="att-input" className="btn btn-secondary btn-md" role="button" tabIndex={0} onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && input.current?.click()}><Upload aria-hidden="true" size="0.9375rem" /> Attach a file</label>
          </>
        )}
      </div>
      {progress !== null && <div className="upload-progress" role="progressbar" aria-label="Uploading" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(progress * 100)}><span style={{ width: `${Math.round(progress * 100)}%` }} /></div>}
      {notice && <Notice tone={notice.tone} title={notice.title} />}
      {items.length > 0 && (
        <ul className="attachment-list">
          {items.map((f) => (
            <li key={f.id}>
              <span className="attachment-name">{f.name}</span>
              <span className="muted">{formatBytes(f.size)}</span>
              <Tag color={STATE[f.state]?.[1] ?? 'gray'} dot>{STATE[f.state]?.[0] ?? f.state}</Tag>
              <span className="attachment-actions">
                {canPreview(f) && <button type="button" className="prop-link" onClick={() => setPreviewing(f)} aria-label={`Preview ${f.name}`}><Eye aria-hidden="true" size="0.875rem" /> Preview</button>}
                {f.state === 'clean' ? <button type="button" className="prop-link" onClick={() => void downloadFile(f.id).catch((e) => fail(e, 'The download failed.'))} aria-label={`Download ${f.name}`}><Download aria-hidden="true" size="0.875rem" /> Download</button> : <span className="muted"><ShieldAlert aria-hidden="true" size="0.875rem" /> Not available</span>}
                {mayEdit && <button type="button" className="prop-link" aria-label={`Remove ${f.name}`} onClick={() => void api(`/api/v1/files/${f.id}/links/artifact_version/${versionId}`, { method: 'DELETE' }).then(load).catch((e) => fail(e, 'Could not remove it.'))}><Trash2 aria-hidden="true" size="0.875rem" /> Remove</button>}
              </span>
            </li>
          ))}
        </ul>
      )}
      <FilePreview file={previewing} onClose={() => setPreviewing(null)} />
    </section>
  );
}
