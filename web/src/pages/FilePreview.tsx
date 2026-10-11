import { Download } from 'lucide-react';
import { useEffect, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type FileInfo, downloadFile } from '../lib/upload';
import { Button } from '../ui/components/Button';
import { FormDialog } from '../ui/components/FormDialog';
import { Notice } from '../ui/components/Notice';

/** Types the server previews (mirrors `procera_files::preview_kind`); everything else downloads. */
const PREVIEWABLE = new Set(['image/png', 'image/jpeg', 'image/gif', 'image/webp', 'application/pdf']);

export const canPreview = (f: FileInfo) => f.state === 'clean' && PREVIEWABLE.has(f.sniffed_type ?? '');

type Loaded = { url: string | null; kind: 'image' | 'pdf' | 'none' };

/**
 * Previews a clean image or PDF (BPMS-061, FR-F04) through a short-lived preview link. The server
 * serves it inline under `CSP: sandbox; default-src 'none'`, so the document gets an opaque origin
 * and runs no script whatever it contains.
 *
 * Images render in an `<img>`. PDFs open in the browser's own viewer inside a frame. That frame
 * carries no `sandbox` attribute on purpose: Chromium refuses to start its PDF viewer in any
 * sandboxed frame (it shows a blocked-page icon even with an empty sandbox), while the response's
 * CSP sandbox gives the same isolation (opaque origin, no scripts, forms, popups or top navigation)
 * and still lets the viewer render. See docs/06 §5.7.
 */
export function FilePreview({ file, onClose }: { file: FileInfo | null; onClose: () => void }) {
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLoaded(null);
    setError(null);
    if (!file) return;
    let live = true;
    api<Loaded>(`/api/v1/files/${file.id}/preview-url`)
      .then((r) => { if (live) setLoaded(r); })
      .catch((e) => { if (live) setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'The preview could not be loaded.'); });
    return () => { live = false; };
  }, [file]);

  const name = file?.name ?? '';
  return (
    <FormDialog
      isOpen={file !== null}
      onOpenChange={(open) => { if (!open) onClose(); }}
      title={name}
      description="Preview. Links, scripts and forms inside the file are disabled."
      width={880}
      footer={
        <>
          <Button variant="secondary" onPress={onClose}>Close</Button>
          {file && <Button variant="primary" onPress={() => void downloadFile(file.id).catch(() => setError('The download failed.'))}><Download aria-hidden="true" size="1rem" /> Download</Button>}
        </>
      }
    >
      <div className="file-preview">
        {error && <Notice tone="danger" title={error} />}
        {!error && !loaded && <p className="muted" role="status">Loading the preview…</p>}
        {loaded?.kind === 'image' && loaded.url && <img src={loaded.url} alt={`Preview of ${name}`} referrerPolicy="no-referrer" />}
        {loaded?.kind === 'pdf' && loaded.url && <iframe src={loaded.url} title={`Preview of ${name}`} referrerPolicy="no-referrer" />}
        {loaded?.kind === 'none' && <p className="muted">This type of file has no preview. Download it to open it.</p>}
      </div>
    </FormDialog>
  );
}
