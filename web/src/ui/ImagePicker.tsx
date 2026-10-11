import { ImagePlus, Upload } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { ApiError, api } from '../lib/api';
import { type FileInfo, uploadFile } from '../lib/upload';
import { Button } from './components/Button';
import { FormDialog } from './components/FormDialog';
import { Notice } from './components/Notice';
import { TextField } from './components/TextField';
import { Empty, Skeleton } from './States';

const IMAGE = /^image\/(png|jpeg|gif|webp)$/;

/**
 * Insert an image into an SOP step (BPMS-042): pick one of the draft's image attachments or upload a
 * new one (scanned, then attached to the draft), and describe it. The description is required: it is
 * the alt text people using screen readers hear, and what exports show in its place.
 */
export function ImagePicker({ versionId, isOpen, onClose, onInsert }: { versionId: string; isOpen: boolean; onClose: () => void; onInsert: (img: { fileId: string; alt: string; caption: string }) => void }) {
  const [items, setItems] = useState<FileInfo[] | null>(null);
  const [chosen, setChosen] = useState<string | null>(null);
  const [alt, setAlt] = useState('');
  const [caption, setCaption] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const load = () => api<{ items: FileInfo[] }>(`/api/v1/versions/${versionId}/attachments`).then((r) => setItems(r.items.filter((f) => f.state === 'clean' && IMAGE.test(f.sniffed_type ?? '')))).catch(() => setItems([]));
  useEffect(() => { if (isOpen) { setChosen(null); setAlt(''); setCaption(''); setError(null); void load(); } }, [isOpen]); // eslint-disable-line react-hooks/exhaustive-deps

  const upload = async (f?: File) => {
    if (!f) return;
    setBusy(true); setError(null);
    try {
      const done = await uploadFile(f);
      if (done.state !== 'clean') { setError(`${f.name} couldn’t be used: ${done.state === 'quarantined' ? 'it was quarantined by the malware scan' : 'it wasn’t accepted'}.`); return; }
      if (!IMAGE.test(done.sniffed_type ?? '')) { setError(`${f.name} isn’t a PNG, JPEG, GIF or WebP image.`); return; }
      await api(`/api/v1/files/${done.id}/links`, { method: 'POST', body: { object_type: 'artifact_version', object_id: versionId } });
      await load();
      setChosen(done.id);
    } catch (e) { setError(e instanceof ApiError ? (e.problem.detail ?? e.problem.title) : 'The upload failed.'); } finally { setBusy(false); if (input.current) input.current.value = ''; }
  };

  return (
    <FormDialog isOpen={isOpen} onOpenChange={(o) => !o && onClose()} title="Insert image" width={560}
      description="Use one of this draft’s images or upload a new one."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button isDisabled={!chosen || !alt.trim() || busy} onPress={() => { onInsert({ fileId: chosen!, alt: alt.trim(), caption: caption.trim() }); onClose(); }}><ImagePlus aria-hidden="true" size="1rem" /> Insert</Button></>}>
      <div className="stack">
        {error && <Notice tone="danger" title={error} />}
        {items === null ? <Skeleton rows={2} label="Loading images" /> : items.length === 0 ? (
          <Empty compact icon={ImagePlus} title="No images on this draft yet">Upload a screenshot or photo to use it here.</Empty>
        ) : (
          <div className="image-pick" role="group" aria-label="Images on this draft">
            {items.map((f) => (
              <button key={f.id} type="button" className="doc-pick-item" aria-pressed={chosen === f.id} onClick={() => setChosen(f.id)}>{f.name}</button>
            ))}
          </div>
        )}
        <div>
          <input ref={input} id="img-input" type="file" accept="image/png,image/jpeg,image/gif,image/webp" className="visually-hidden" onChange={(e) => void upload(e.target.files?.[0])} />
          <label htmlFor="img-input" className="btn btn-secondary btn-md" role="button" tabIndex={0} aria-disabled={busy} onKeyDown={(e) => (e.key === 'Enter' || e.key === ' ') && input.current?.click()}><Upload aria-hidden="true" size="0.9375rem" /> {busy ? 'Uploading…' : 'Upload an image'}</label>
        </div>
        <TextField label="Description" isRequired value={alt} onChange={setAlt} description="What the image shows, for people who can’t see it. Required." />
        <TextField label="Caption (optional)" value={caption} onChange={setCaption} />
      </div>
    </FormDialog>
  );
}
