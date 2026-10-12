import { Download, FileQuestion } from 'lucide-react';
import { Suspense, lazy, useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type FileInfo, downloadFile, formatBytes } from '../lib/upload';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';
import { Skeleton } from '../ui/States';

// The renderers pull in pdf.js, docx-preview and SheetJS: each is its own chunk, loaded on first use.
const PdfView = lazy(() => import('./preview/PdfView'));
const DocxView = lazy(() => import('./preview/DocxView'));
const SheetView = lazy(() => import('./preview/SheetView'));

export type PreviewKind = 'image' | 'pdf' | 'docx' | 'xlsx' | 'csv';

const IMAGES = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp']);
const DOCX = 'application/vnd.openxmlformats-officedocument.wordprocessingml.document';
const XLSX = 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet';

/** How a file previews (mirrors `procera_files::preview_kind`), or null when it has no preview. */
export function previewKindOf(f: FileInfo): PreviewKind | null {
  if (f.state !== 'clean') return null;
  const t = f.sniffed_type ?? '';
  if (IMAGES.has(t)) return 'image';
  if (t === 'application/pdf') return 'pdf';
  if (t === DOCX) return 'docx';
  if (t === XLSX) return 'xlsx';
  if (t === 'text/plain' && /\.csv$/i.test(f.name)) return 'csv';
  return null;
}

/** Every clean file offers Preview; types without a renderer show what the file is and a download. */
export const canPreview = (f: FileInfo) => f.state === 'clean';

type State =
  | { status: 'loading' }
  | { status: 'none' }
  | { status: 'error'; message: string }
  | { status: 'image'; url: string }
  | { status: 'data'; kind: Exclude<PreviewKind, 'image'>; data: ArrayBuffer };

const message = (e: unknown) => (e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'The preview could not be loaded.');

/**
 * The one file preview dialog (BPMS-061, FR-F04), used wherever files are listed. Bytes come
 * through a short-lived preview link that the server re-authorizes and serves under
 * `CSP: sandbox; default-src 'none'`, nosniff and CORP same-origin.
 *
 * - Images: an `<img>`.
 * - PDF: pdf.js renders pages to a canvas plus a text layer; no PDF scripting, XFA or annotations.
 * - DOCX: docx-preview renders inert HTML into a frame with `sandbox="allow-same-origin"` (no
 *   scripts) and a CSP that loads nothing but inline styles and `data:` images.
 * - XLSX/CSV: a read-only table of the first rows, built as text nodes.
 */
export function FilePreview({ file, onClose }: { file: FileInfo | null; onClose: () => void }) {
  const [state, setState] = useState<State>({ status: 'loading' });

  useEffect(() => {
    setState({ status: 'loading' });
    if (!file) return;
    if (!previewKindOf(file)) { setState({ status: 'none' }); return; }
    const ctl = new AbortController();
    (async () => {
      const r = await api<{ url: string | null; kind: PreviewKind | 'none' }>(`/api/v1/files/${file.id}/preview-url`);
      if (r.kind === 'none' || !r.url) return setState({ status: 'none' });
      if (r.kind === 'image') return setState({ status: 'image', url: r.url });
      const res = await fetch(r.url, { signal: ctl.signal, cache: 'no-store', credentials: 'omit', referrerPolicy: 'no-referrer' });
      if (res.status === 415) return setState({ status: 'none' });
      if (!res.ok) throw new Error(String(res.status));
      setState({ status: 'data', kind: r.kind, data: await res.arrayBuffer() });
    })().catch((e) => { if (!ctl.signal.aborted) setState({ status: 'error', message: message(e) }); });
    return () => ctl.abort();
  }, [file]);

  const name = file?.name ?? '';
  const failed = (m: string) => setState({ status: 'error', message: m });
  return (
    <FormDialog
      isOpen={file !== null}
      onOpenChange={(open) => { if (!open) onClose(); }}
      title={name}
      description="Preview. Links, scripts and forms inside the file are disabled."
      width={960}
      footer={
        <>
          <Button variant="secondary" onPress={onClose}>Close</Button>
          {file && <Button variant="primary" onPress={() => void downloadFile(file.id).catch(() => failed('The download failed.'))}><Download aria-hidden="true" size="1rem" /> Download</Button>}
        </>
      }
    >
      <div className="file-preview">
        {state.status === 'error' && <Notice tone="danger" title={state.message} />}
        {state.status === 'loading' && <Skeleton variant="block" label="Loading the preview" />}
        {state.status === 'image' && <img src={state.url} alt={`Preview of ${name}`} referrerPolicy="no-referrer" />}
        {state.status === 'none' && file && <NoPreview file={file} />}
        {state.status === 'data' && (
          <Suspense fallback={<Skeleton variant="block" label="Loading the preview" />}>
            {state.kind === 'pdf' && <PdfView data={state.data} name={name} onError={failed} />}
            {state.kind === 'docx' && <DocxView data={state.data} name={name} onError={failed} />}
            {(state.kind === 'xlsx' || state.kind === 'csv') && <SheetView data={state.data} kind={state.kind} name={name} onError={failed} />}
          </Suspense>
        )}
      </div>
    </FormDialog>
  );
}

function NoPreview({ file }: { file: FileInfo }) {
  return (
    <div className="preview-fallback">
      <FileQuestion aria-hidden="true" size="2rem" />
      <p className="preview-fallback-title">No preview for this type — download it.</p>
      <dl className="preview-facts">
        <dt>Name</dt><dd>{file.name}</dd>
        <dt>Size</dt><dd>{formatBytes(file.size)}</dd>
        <dt>Type</dt><dd>{file.sniffed_type ?? 'Unknown'}</dd>
      </dl>
    </div>
  );
}
