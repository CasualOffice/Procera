import { ChevronLeft, ChevronRight, Search, ZoomIn, ZoomOut } from 'lucide-react';
import { GlobalWorkerOptions, type PDFDocumentProxy, type PDFPageProxy, type RenderTask, TextLayer, getDocument } from 'pdfjs-dist';
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url';
import { useEffect, useRef, useState } from 'react';

// The worker ships with the app (same origin), never from a CDN.
GlobalWorkerOptions.workerSrc = workerUrl;

/** pdf.js scale 1 is 72 dpi; "100%" means real size on a 96-dpi CSS pixel grid, as in Firefox. */
const CSS_PER_PT = 96 / 72;
const STEPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4];
const MAX_SEARCH_PAGES = 300;
type TextContent = Awaited<ReturnType<PDFPageProxy['getTextContent']>>;

const textOf = (c: TextContent) => c.items.map((i) => ('str' in i ? i.str : '')).join(' ');

/**
 * PDF preview with pdf.js (BPMS-061): one page at a time on a canvas, with a transparent text layer
 * so the text can be selected, searched and read by assistive technology. No annotation layer, no
 * PDF JavaScript and no XFA forms: links and form fields inside the file do nothing.
 */
export default function PdfView({ data, name, onError }: { data: ArrayBuffer; name: string; onError: (m: string) => void }) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [page, setPage] = useState(1);
  const [zoom, setZoom] = useState<number | 'fit'>('fit');
  const [shown, setShown] = useState(1);
  const [width, setWidth] = useState(0);
  const [query, setQuery] = useState('');
  const [hits, setHits] = useState<{ q: string; pages: number[]; at: number } | null>(null);
  const viewer = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const layer = useRef<HTMLDivElement>(null);
  const texts = useRef(new Map<number, TextContent>());
  const fail = useRef(onError);
  fail.current = onError;

  useEffect(() => {
    // pdf.js takes ownership of the buffer it is given, so hand it a copy.
    const task = getDocument({ data: new Uint8Array(data.slice(0)), enableXfa: false, stopAtErrors: false });
    let live = true; // a load destroyed by cleanup (e.g. StrictMode's second run) must not report an error
    task.promise.then((d) => { if (live) setDoc(d); }, () => { if (live) fail.current('This PDF could not be displayed. Download it to open it.'); });
    texts.current = new Map();
    return () => { live = false; void task.destroy(); };
  }, [data]);

  useEffect(() => {
    const el = viewer.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const cs = getComputedStyle(el);
      setWidth(el.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const content = async (n: number) => {
    let c = texts.current.get(n);
    if (!c && doc) { c = await (await doc.getPage(n)).getTextContent(); texts.current.set(n, c); }
    return c;
  };

  useEffect(() => {
    if (!doc || !canvas.current || !layer.current || width === 0) return;
    let task: RenderTask | null = null;
    let live = true;
    (async () => {
      const p = await doc.getPage(page);
      if (!live) return;
      const base = p.getViewport({ scale: CSS_PER_PT });
      const z = zoom === 'fit' ? Math.max(0.1, width / base.width) : zoom;
      const vp = p.getViewport({ scale: z * CSS_PER_PT });
      const c = canvas.current!;
      const ratio = window.devicePixelRatio || 1;
      c.width = Math.floor(vp.width * ratio);
      c.height = Math.floor(vp.height * ratio);
      // Canvas geometry is in CSS pixels by definition (it mirrors the PDF viewport).
      c.style.width = `${Math.floor(vp.width)}px`;
      c.style.height = `${Math.floor(vp.height)}px`;
      task = p.render({ canvas: c, viewport: vp, transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0] });
      await task.promise;
      if (!live) return;
      setShown(z);
      const tl = layer.current!;
      tl.replaceChildren();
      tl.parentElement!.style.setProperty('--total-scale-factor', String(vp.scale));
      const tc = await content(page);
      if (!live || !tc) return;
      await new TextLayer({ textContentSource: tc, container: tl, viewport: vp }).render();
      mark(tl, hits?.q ?? '');
    })().catch((e: unknown) => {
      if (live && !(e instanceof Error && e.name === 'RenderingCancelledException')) fail.current('This page could not be displayed.');
    });
    return () => { live = false; task?.cancel(); };
  }, [doc, page, zoom, width]); // `hits` only re-marks (below); a new render reads the latest.

  useEffect(() => { if (layer.current) mark(layer.current, hits?.q ?? ''); }, [hits]);

  const pages = doc?.numPages ?? 0;
  const go = (n: number) => setPage(Math.min(Math.max(1, n), pages || 1));
  const step = (dir: 1 | -1) => {
    const cur = zoom === 'fit' ? shown : zoom;
    const next = dir > 0 ? STEPS.find((s) => s > cur + 0.001) : [...STEPS].reverse().find((s) => s < cur - 0.001);
    setZoom(next ?? cur);
  };

  const search = async () => {
    const q = query.trim().toLowerCase();
    if (!q || !doc) { setHits(null); return; }
    if (hits && hits.q === q && hits.pages.length) {
      const at = (hits.at + 1) % hits.pages.length;
      setHits({ ...hits, at });
      go(hits.pages[at]!);
      return;
    }
    const found: number[] = [];
    for (let n = 1; n <= Math.min(pages, MAX_SEARCH_PAGES); n++) {
      const c = await content(n);
      if (c && textOf(c).toLowerCase().includes(q)) found.push(n);
    }
    const at = Math.max(0, found.findIndex((n) => n >= page));
    setHits({ q, pages: found, at });
    if (found.length) go(found[at]!);
  };

  const onKey = (e: React.KeyboardEvent) => {
    const k = e.key;
    if (k === 'ArrowRight' || k === 'PageDown') go(page + 1);
    else if (k === 'ArrowLeft' || k === 'PageUp') go(page - 1);
    else if (k === 'Home') go(1);
    else if (k === 'End') go(pages);
    else if (k === '+' || k === '=') step(1);
    else if (k === '-') step(-1);
    else return;
    e.preventDefault();
  };

  return (
    <div className="pdf-view">
      <div className="preview-toolbar" role="toolbar" aria-label="PDF controls">
        <button type="button" className="icon-btn" aria-label="Previous page" disabled={page <= 1} onClick={() => go(page - 1)}><ChevronLeft aria-hidden="true" size="1rem" /></button>
        <span className="preview-status" aria-live="polite">{pages ? `Page ${page} of ${pages}` : 'Loading…'}</span>
        <button type="button" className="icon-btn" aria-label="Next page" disabled={page >= pages} onClick={() => go(page + 1)}><ChevronRight aria-hidden="true" size="1rem" /></button>
        <span className="preview-sep" aria-hidden="true" />
        <button type="button" className="icon-btn" aria-label="Zoom out" onClick={() => step(-1)}><ZoomOut aria-hidden="true" size="1rem" /></button>
        <span className="preview-status" aria-live="polite">{Math.round(shown * 100)}%</span>
        <button type="button" className="icon-btn" aria-label="Zoom in" onClick={() => step(1)}><ZoomIn aria-hidden="true" size="1rem" /></button>
        <button type="button" className="preview-chip" aria-pressed={zoom === 'fit'} onClick={() => setZoom('fit')}>Fit width</button>
        <button type="button" className="preview-chip" aria-pressed={zoom === 1} onClick={() => setZoom(1)}>100%</button>
        <form className="preview-search" role="search" onSubmit={(e) => { e.preventDefault(); void search(); }}>
          <Search aria-hidden="true" size="0.875rem" />
          <input type="search" aria-label="Search in the document" placeholder="Search" value={query} onChange={(e) => { setQuery(e.target.value); setHits(null); }} />
          {hits && <span className="preview-status" aria-live="polite">{hits.pages.length ? `${hits.at + 1} of ${hits.pages.length} pages` : 'No matches'}</span>}
        </form>
      </div>
      <div ref={viewer} className="pdf-pages" tabIndex={0} role="region" aria-label={`${name}, page ${page}`} onKeyDown={onKey}>
        <div className="pdf-page">
          <canvas ref={canvas} aria-hidden="true" />
          <div ref={layer} className="textLayer" />
        </div>
      </div>
    </div>
  );
}

/** Highlights the text-layer spans that contain `q` (case-insensitive). */
function mark(root: HTMLElement, q: string) {
  const needle = q.toLowerCase();
  for (const s of root.querySelectorAll('span')) s.classList.toggle('pdf-hit', !!needle && (s.textContent ?? '').toLowerCase().includes(needle));
}
