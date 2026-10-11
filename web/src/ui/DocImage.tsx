import { ImageOff } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';

/**
 * An image block (BPMS-042): the picture comes through a short-lived, re-authorized preview link
 * (EXIF stripped, CSP sandboxed); the description is the alt text and the caption sits below.
 * If the picture can't be shown, the description is, so the step still reads.
 */
export function DocImage({ fileId, alt, caption }: { fileId: string; alt: string; caption?: string }) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    setSrc(null);
    setFailed(false);
    api<{ url: string | null; kind: string }>(`/api/v1/files/${fileId}/preview-url`)
      .then((r) => { if (!live) return; if (r.url && r.kind === 'image') setSrc(r.url); else setFailed(true); })
      .catch(() => { if (live) setFailed(true); });
    return () => { live = false; };
  }, [fileId]);
  return (
    <figure className="doc-figure">
      {failed ? (
        <p className="doc-figure-missing"><ImageOff aria-hidden="true" size="1rem" /> <span>Image: {alt}</span></p>
      ) : src ? (
        <img src={src} alt={alt} referrerPolicy="no-referrer" />
      ) : (
        <span className="sk sk-block doc-figure-loading" role="status" aria-label={`Loading image: ${alt}`} />
      )}
      {caption && <figcaption>{caption}</figcaption>}
    </figure>
  );
}
