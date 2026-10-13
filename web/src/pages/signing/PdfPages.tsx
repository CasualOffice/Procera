import { GlobalWorkerOptions, type PDFDocumentProxy, type RenderTask, getDocument } from 'pdfjs-dist';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import './signing.css';

// The worker ships with the app (same origin), never from a CDN.
GlobalWorkerOptions.workerSrc = workerUrl;

/**
 * Every page of a PDF, stacked, each drawn at the container's width (pdf.js, no annotation layer, no
 * PDF JavaScript), with an overlay per page for field boxes. `overlay(page, size)` draws inside a
 * box positioned exactly over the page, so fractions of the page map to percentages of the overlay.
 * Pages render as they scroll into view.
 */
export function PdfPages({ data, label, overlay, onError, maxWidth = 900 }: {
  data: ArrayBuffer;
  label: string;
  overlay?: (page: number, size: { width: number; height: number }) => ReactNode;
  onError?: (message: string) => void;
  maxWidth?: number;
}) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [sizes, setSizes] = useState<{ width: number; height: number }[]>([]);
  const [width, setWidth] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const fail = useRef(onError);
  fail.current = onError;

  useEffect(() => {
    const task = getDocument({ data: new Uint8Array(data.slice(0)), enableXfa: false, stopAtErrors: false });
    let live = true;
    task.promise.then(async (d) => {
      if (!live) return;
      const s = [];
      for (let i = 1; i <= d.numPages; i++) {
        const v = (await d.getPage(i)).getViewport({ scale: 1 });
        s.push({ width: v.width, height: v.height });
      }
      if (live) { setSizes(s); setDoc(d); }
    }).catch(() => live && fail.current?.('The document couldn’t be shown.'));
    return () => { live = false; void task.destroy(); };
  }, [data]);

  useEffect(() => {
    const el = box.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setWidth(Math.min(maxWidth, Math.floor(e!.contentRect.width))));
    ro.observe(el);
    return () => ro.disconnect();
  }, [maxWidth]);

  return (
    <div ref={box} className="pdf-pages" role="region" aria-label={label}>
      {!doc && <p className="muted pdf-pages-loading">Loading the document…</p>}
      {doc && width > 0 && sizes.map((s, i) => (
        <Page key={i} doc={doc} n={i + 1} width={width} size={s} overlay={overlay} />
      ))}
    </div>
  );
}

function Page({ doc, n, width, size, overlay }: {
  doc: PDFDocumentProxy; n: number; width: number; size: { width: number; height: number };
  overlay?: (page: number, size: { width: number; height: number }) => ReactNode;
}) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const [seen, setSeen] = useState(n <= 2);
  const height = Math.round((width * size.height) / size.width);

  useEffect(() => {
    if (seen || !wrap.current) return;
    const io = new IntersectionObserver(([e]) => e?.isIntersecting && setSeen(true), { rootMargin: '600px' });
    io.observe(wrap.current);
    return () => io.disconnect();
  }, [seen]);

  useEffect(() => {
    if (!seen || !canvas.current) return;
    let task: RenderTask | null = null;
    let live = true;
    void doc.getPage(n).then((p) => {
      if (!live || !canvas.current) return;
      const ratio = window.devicePixelRatio || 1;
      const v = p.getViewport({ scale: (width / size.width) * ratio });
      const c = canvas.current;
      c.width = Math.floor(v.width);
      c.height = Math.floor(v.height);
      task = p.render({ canvas: c, viewport: v });
      task.promise.catch(() => undefined);
    });
    return () => { live = false; task?.cancel(); };
  }, [doc, n, seen, width, size.width]);

  return (
    <div ref={wrap} className="pdf-page" style={{ width, height }} data-page={n}>
      <canvas ref={canvas} style={{ width, height }} aria-hidden="true" />
      <div className="pdf-page-overlay">{overlay?.(n, { width, height })}</div>
      <span className="pdf-page-no" aria-hidden="true">{n}</span>
    </div>
  );
}
