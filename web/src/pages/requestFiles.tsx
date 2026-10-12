import { Download, Eye, FileText, Paperclip, Upload, X } from 'lucide-react';
import { useEffect, useId, useRef, useState } from 'react';
import { ApiError, api } from '../lib/api';
import type { Attachment, FormField } from '../lib/requests';
import { type FileInfo, downloadFile, formatBytes, uploadFile } from '../lib/upload';
import { Tag, type TagColor } from '../ui/Tag';
import { FilePreview, previewKindOf } from './FilePreview';

/*
 * Files on requests (BPMS-060/101): the `file` form field (drop or pick files, upload through the
 * resumable, scanned pipeline, thumbnails and a scan chip per file, remove before sending) and the
 * attachment list on the request page and decision panel (Preview in the shared dialog, Download).
 */

/** What request attachments accept: images, PDF, Word, Excel and CSV (the server checks the bytes). */
export const ACCEPT = 'image/jpeg,image/png,image/webp,image/gif,application/pdf,.pdf,.docx,.xlsx,.csv';

export type FileState = 'uploading' | 'scanning' | 'clean' | 'quarantined' | 'rejected' | 'failed';
export interface PickedFile { key: string; id?: string; name: string; size: number; state: FileState; sniffed_type?: string | null; thumb?: string; progress?: number; message?: string }

const CHIP: Record<FileState, [string, TagColor]> = {
  uploading: ['Uploading…', 'gray'], scanning: ['Scanning…', 'yellow'], clean: ['Ready', 'green'],
  quarantined: ['Blocked by the virus scan', 'red'], rejected: ['Can’t be attached', 'red'], failed: ['Upload failed', 'red'],
};

const asInfo = (f: { id: string; name: string; size: number; state: string; sniffed_type?: string | null }): FileInfo => ({
  id: f.id, name: f.name, size: f.size, received: f.size, state: f.state, sniffed_type: f.sniffed_type ?? null, scan_result: null, sha256: null, uploaded_by: '', created_at: '',
});

/** Form state from stored file refs (`{id, name, size, content_type}`), for a resubmit. */
export function pickedFrom(v: unknown): PickedFile[] {
  const list = Array.isArray(v) ? v : v ? [v] : [];
  return list.flatMap((x) => {
    const r = x as { id?: string; name?: string; size?: number; content_type?: string };
    return r.id ? [{ key: r.id, id: r.id, name: r.name ?? 'File', size: r.size ?? 0, state: 'clean' as const, sniffed_type: r.content_type?.split(';')[0] ?? null }] : [];
  });
}

/** Why a file field can't be sent yet, or null. */
export function fileProblem(items: PickedFile[]): string | null {
  const bad = items.find((f) => f.state !== 'clean' && f.state !== 'uploading' && f.state !== 'scanning');
  if (bad) return `Remove ${bad.name}: ${bad.message ?? CHIP[bad.state][0].toLowerCase()}.`;
  if (items.some((f) => f.state === 'uploading' || f.state === 'scanning')) return 'Waiting for the virus scan… You can send it as soon as every file is ready.';
  return null;
}

const allowedType = (f: FileInfo) => previewKindOf({ ...f, state: 'clean' }) !== null;

/** Uploads picked files and keeps their scan state current. */
function useUploads(initial: PickedFile[], onChange: (items: PickedFile[]) => void) {
  const [items, setItems] = useState<PickedFile[]>(initial);
  const first = useRef(true);
  useEffect(() => { if (first.current) { first.current = false; return; } onChange(items); }, [items]); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => () => items.forEach((f) => f.thumb && URL.revokeObjectURL(f.thumb)), []); // eslint-disable-line react-hooks/exhaustive-deps
  const patch = (key: string, p: Partial<PickedFile>) => setItems((xs) => xs.map((x) => (x.key === key ? { ...x, ...p } : x)));

  const settle = (key: string, info: FileInfo) => {
    if (info.state === 'clean' && !allowedType(info)) patch(key, { id: info.id, state: 'rejected', message: 'attach images, PDF, Word, Excel or CSV files' });
    else patch(key, { id: info.id, state: (info.state as FileState) ?? 'failed', sniffed_type: info.sniffed_type });
    if (info.state === 'scanning') {
      let tries = 0;
      const t = window.setInterval(() => {
        if (++tries > 40) { window.clearInterval(t); return; }
        api<{ file: FileInfo }>(`/api/v1/files/${info.id}`).then((r) => { if (r.file.state !== 'scanning') { window.clearInterval(t); settle(key, r.file); } }).catch(() => undefined);
      }, 3000);
    }
  };
  const add = (files: File[]) => {
    for (const file of files) {
      const key = crypto.randomUUID();
      const thumb = file.type.startsWith('image/') ? URL.createObjectURL(file) : undefined;
      setItems((xs) => [...xs, { key, name: file.name, size: file.size, state: 'uploading', thumb, progress: 0 }]);
      uploadFile(file, (p) => patch(key, { progress: p }))
        .then((info) => settle(key, info))
        .catch((e) => patch(key, { state: 'failed', message: e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'check your connection and try again' }));
    }
  };
  const remove = (key: string) => setItems((xs) => { const f = xs.find((x) => x.key === key); if (f?.thumb) URL.revokeObjectURL(f.thumb); return xs.filter((x) => x.key !== key); });
  return { items, add, remove };
}

/** A drop zone with "Add files", for a form field or a decision. Phones offer the camera for images. */
export function FileDrop({ label, value, onChange, multiple = true, error, description, name }: {
  label: string; value: PickedFile[]; onChange: (items: PickedFile[]) => void; multiple?: boolean; error?: string; description?: string; name?: string;
}) {
  const { items, add, remove } = useUploads(value, onChange);
  const [over, setOver] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const id = useId();
  const full = !multiple && items.length >= 1;
  const pick = (list: FileList | null) => { if (list?.length) add(Array.from(list).slice(0, multiple ? 10 : 1)); if (input.current) input.current.value = ''; };
  return (
    <div className="field req-files" data-invalid={error ? true : undefined}>
      <span className="field-label" id={`${id}-l`}>{label}</span>
      {description && <span className="field-description" id={`${id}-d`}>{description}</span>}
      {!full && (
        <div className={`file-drop${over ? ' file-drop-over' : ''}`} onDragOver={(e) => { e.preventDefault(); setOver(true); }} onDragLeave={() => setOver(false)}
          onDrop={(e) => { e.preventDefault(); setOver(false); pick(e.dataTransfer.files); }}>
          <Upload aria-hidden="true" size="1.125rem" />
          <span>Drop files here or</span>
          <input ref={input} id={`${id}-i`} name={name} type="file" className="visually-hidden" multiple={multiple} accept={ACCEPT} aria-labelledby={`${id}-l`} aria-describedby={error ? `${id}-e` : description ? `${id}-d` : undefined} onChange={(e) => pick(e.target.files)} />
          <label htmlFor={`${id}-i`} className="btn btn-secondary btn-sm" role="button" tabIndex={0} onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && input.current?.click()}>
            <Paperclip aria-hidden="true" size="0.875rem" /> Add files
          </label>
          <span className="muted file-drop-hint">Images, PDF, Word, Excel or CSV</span>
        </div>
      )}
      {items.length > 0 && (
        <ul className="file-picked" aria-label={`${label}: files`}>
          {items.map((f) => (
            <li key={f.key}>
              {f.thumb ? <img src={f.thumb} alt="" className="file-thumb" /> : <span className="file-thumb file-thumb-icon" aria-hidden="true"><FileText size="1rem" /></span>}
              <span className="file-picked-name">{f.name}<span className="muted"> · {formatBytes(f.size)}</span></span>
              <Tag color={CHIP[f.state][1]} dot>{f.state === 'uploading' && f.progress ? `Uploading ${Math.round(f.progress * 100)}%` : CHIP[f.state][0]}</Tag>
              <button type="button" className="icon-btn" onClick={() => remove(f.key)} aria-label={`Remove ${f.name}`}><X aria-hidden="true" size="0.875rem" /></button>
            </li>
          ))}
        </ul>
      )}
      {error && <span className="field-error" id={`${id}-e`}>{error}</span>}
    </div>
  );
}

/** A `file` form field. */
export function FileField({ field, value, onChange, error }: { field: FormField; value: unknown; onChange: (v: PickedFile[]) => void; error?: string }) {
  const items = Array.isArray(value) ? (value as PickedFile[]) : [];
  return <FileDrop name={field.name} label={field.label} value={items} onChange={onChange} multiple={field.multiple !== false} error={error} description={field.help} />;
}

/** Files on a request, each with Preview and Download (both re-checked by the server). */
export function AttachmentList({ items, heading = 'Attachments', headingLevel = 2 }: { items: Attachment[]; heading?: string; headingLevel?: 2 | 3 }) {
  const [previewing, setPreviewing] = useState<FileInfo | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const id = useId();
  if (!items.length) return null;
  const H = headingLevel === 2 ? 'h2' : 'h3';
  return (
    <section className="req-attachments" aria-labelledby={id}>
      <H id={id}><Paperclip aria-hidden="true" size="0.9375rem" /> {heading}</H>
      {problem && <p className="field-error" role="alert">{problem}</p>}
      <FileRows items={items} onPreview={(f) => setPreviewing(asInfo(f))} onProblem={setProblem} />
      <FilePreview file={previewing} onClose={() => setPreviewing(null)} />
    </section>
  );
}

/** The rows alone (the timeline lists a decision's files under it). */
export function FileRows({ items, onPreview, onProblem }: { items: Attachment[]; onPreview: (f: Attachment) => void; onProblem: (m: string) => void }) {
  return (
    <ul className="attachment-list">
      {items.map((f, i) => (
        <li key={`${f.id}-${f.field ?? f.source}-${i}`}>
          <span className="attachment-name">{f.name}</span>
          <span className="muted">{f.source} · {formatBytes(f.size)}</span>
          {f.state === 'clean' ? (
            <span className="attachment-actions">
              <button type="button" className="prop-link" onClick={() => onPreview(f)} aria-label={`Preview ${f.name}`}><Eye aria-hidden="true" size="0.875rem" /> Preview</button>
              <button type="button" className="prop-link" onClick={() => void downloadFile(f.id).catch((e) => onProblem(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'The download failed.'))} aria-label={`Download ${f.name}`}><Download aria-hidden="true" size="0.875rem" /> Download</button>
            </span>
          ) : <Tag color="red">Not available</Tag>}
        </li>
      ))}
    </ul>
  );
}

/** Timeline files: a compact list with the same Preview dialog. */
export function TimelineFiles({ items }: { items: Attachment[] }) {
  const [previewing, setPreviewing] = useState<FileInfo | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  return (
    <div className="req-timeline-files">
      {problem && <p className="field-error" role="alert">{problem}</p>}
      <FileRows items={items} onPreview={(f) => setPreviewing(asInfo(f))} onProblem={setProblem} />
      <FilePreview file={previewing} onClose={() => setPreviewing(null)} />
    </div>
  );
}
