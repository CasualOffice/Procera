import { type HElement, renderAsync } from 'docx-preview';
import { useRef } from 'react';

/**
 * The frame's own policy: nothing loads but inline styles and `data:` images (no remote images,
 * fonts or stylesheets a document might point at). The frame has no `allow-scripts`, so nothing
 * inside it can run; `allow-same-origin` only lets this page write the rendered nodes into it.
 */
const FRAME_CSP = "default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:; form-action 'none'";

/** Elements that never belong in a rendered document. */
const BLOCKED = new Set(['script', 'iframe', 'frame', 'object', 'embed', 'link', 'meta', 'base', 'form', 'input', 'button', 'textarea', 'select', 'foreignobject', 'use', 'animate', 'set', 'portal']);

/** Only in-document bookmarks may link, and only `data:` images may load. */
const safeAttr = (name: string, value: string) => {
  const n = name.toLowerCase();
  if (n.startsWith('on')) return false;
  if (n === 'href' || n === 'xlink:href') return value.startsWith('#') || value.startsWith('data:image/');
  if (n === 'src' || n === 'srcset' || n === 'action' || n === 'formaction' || n === 'background' || n === 'poster') return n === 'src' && value.startsWith('data:image/');
  return true;
};

type HNode = string | Node | HElement;

/** docx-preview's element factory, rebound to the frame's document and filtered as it builds. */
function factory(doc: Document) {
  const isNode = (v: unknown): v is Node => typeof v === 'object' && v !== null && 'nodeType' in v;
  const h = (el: HNode): Node => {
    if (typeof el === 'string') return doc.createTextNode(el);
    if (isNode(el)) return el;
    const { ns, tagName, className, style, children, ...props } = el;
    if (tagName === '#fragment') {
      const f = doc.createDocumentFragment();
      children?.forEach((c) => f.appendChild(h(c)));
      return f;
    }
    if (tagName === '#comment' || BLOCKED.has(tagName.toLowerCase())) return doc.createComment('');
    const node = ns ? doc.createElementNS(ns, tagName) : doc.createElement(tagName);
    if (className) node.setAttribute('class', className);
    if (typeof style === 'string') node.setAttribute('style', style);
    else if (style) Object.assign((node as HTMLElement).style, style);
    for (const [k, v] of Object.entries(props)) {
      if (v === undefined || !safeAttr(k, String(v))) continue;
      (node as unknown as Record<string, unknown>)[k] = v;
    }
    children?.forEach((c) => node.appendChild(h(c)));
    return node;
  };
  return h;
}

/** Defence in depth after rendering: drop blocked elements and unsafe attributes the factory never saw. */
function scrub(root: Element) {
  for (const el of [...root.querySelectorAll('*')]) {
    if (BLOCKED.has(el.localName.toLowerCase())) { el.remove(); continue; }
    for (const a of [...el.attributes]) if (!safeAttr(a.name, a.value)) el.removeAttribute(a.name);
  }
}

/**
 * Word preview with docx-preview (BPMS-061). The document is rendered as inert HTML inside a
 * sandboxed frame: no scripts, no forms, no outbound links, no remote resources.
 */
export default function DocxView({ data, name, onError }: { data: ArrayBuffer; name: string; onError: (m: string) => void }) {
  const done = useRef(false);
  const css = getComputedStyle(document.documentElement);
  const bg = css.getPropertyValue('--color-bg').trim() || '#f4f4f5';
  const lang = (document.documentElement.lang || 'en').replace(/[^\w-]/g, '');
  const title = name.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
  const srcDoc = `<!doctype html><html lang="${lang}"><head><meta charset="utf-8"><title>${title}</title><meta http-equiv="Content-Security-Policy" content="${FRAME_CSP}"><style>html,body{margin:0;background:${bg}}.docx-wrapper{background:${bg}!important;padding:1rem!important}.docx-wrapper>section.docx{box-shadow:0 1px 3px rgb(0 0 0 / .2)!important}</style></head><body><div id="s"></div><div id="d"></div></body></html>`;

  const onLoad = (e: React.SyntheticEvent<HTMLIFrameElement>) => {
    const doc = e.currentTarget.contentDocument;
    if (done.current || !doc) return;
    done.current = true;
    const body = doc.getElementById('d')!;
    renderAsync(new Uint8Array(data.slice(0)), body, doc.getElementById('s')!, {
      h: factory(doc),
      inWrapper: true,
      breakPages: true,
      ignoreFonts: true,
      useBase64URL: true,
      renderAltChunks: false,
      renderComments: false,
      renderChanges: false,
      experimental: false,
    })
      .then(() => scrub(doc.body))
      .catch(() => onError('This Word document could not be displayed. Download it to open it.'));
  };

  return <iframe className="docx-frame" title={`Preview of ${name}`} sandbox="allow-same-origin" referrerPolicy="no-referrer" srcDoc={srcDoc} onLoad={onLoad} />;
}
