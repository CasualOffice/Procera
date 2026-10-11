/**
 * The SOP/policy block model (ADR-0013) and its mapping to the editor's document (TipTap/ProseMirror
 * JSON). The server validates the same model (crates/library/src/blocks.rs).
 *
 * `text` is always the plain text (diffs, search, hashes); `runs` add bold, italic and links.
 * Block heading levels 1–3 render as h2–h4 under the document title.
 */
export interface Run { text: string; bold?: boolean; italic?: boolean; href?: string }
export interface Block {
  type: 'paragraph' | 'heading' | 'list' | 'callout' | 'table' | string;
  text?: string;
  runs?: Run[];
  level?: number;
  ordered?: boolean;
  items?: string[];
  item_runs?: Run[][];
  tone?: 'info' | 'warning';
  header?: boolean;
  rows?: string[][];
  /** Image blocks: the attached file; `text` is the alt text. */
  file_id?: string;
  caption?: string;
}

interface PMMark { type: string; attrs?: Record<string, unknown> }
interface PMNode { type: string; attrs?: Record<string, unknown>; content?: PMNode[]; text?: string; marks?: PMMark[] }

const safeHref = (h: string) => /^(https?:|mailto:)/i.test(h.trim()) || (h.startsWith('/') && !h.startsWith('//'));

// ---- blocks → editor ----

function runsToNodes(text: string, runs?: Run[]): PMNode[] {
  const parts = runs?.length ? runs : text ? [{ text }] : [];
  return parts.filter((r) => r.text).map((r) => {
    const marks: PMMark[] = [];
    if (r.bold) marks.push({ type: 'bold' });
    if (r.italic) marks.push({ type: 'italic' });
    if (r.href && safeHref(r.href)) marks.push({ type: 'link', attrs: { href: r.href } });
    return marks.length ? { type: 'text', text: r.text, marks } : { type: 'text', text: r.text };
  });
}

const para = (text: string, runs?: Run[]): PMNode => ({ type: 'paragraph', content: runsToNodes(text, runs) });

export function toEditor(blocks: Block[] = []): PMNode {
  const content: PMNode[] = blocks.map((b) => {
    switch (b.type) {
      case 'heading':
        return { type: 'heading', attrs: { level: Math.min(4, Math.max(2, (b.level ?? 2) + 1)) }, content: runsToNodes(b.text ?? '', b.runs) };
      case 'list':
        return {
          type: b.ordered ? 'orderedList' : 'bulletList',
          content: (b.items ?? []).map((it, i) => ({ type: 'listItem', content: [para(it, b.item_runs?.[i])] })),
        };
      case 'callout':
        return { type: 'blockquote', content: [para(b.text ?? '', b.runs)] };
      case 'image':
        return { type: 'procImage', attrs: { fileId: b.file_id ?? '', alt: b.text ?? '', caption: b.caption ?? '' } };
      case 'table':
        return {
          type: 'table',
          content: (b.rows ?? []).map((r, i) => ({
            type: 'tableRow',
            content: r.map((c) => ({ type: i === 0 && b.header !== false ? 'tableHeader' : 'tableCell', content: [para(c)] })),
          })),
        };
      default:
        return para(b.text ?? '', b.runs);
    }
  });
  return { type: 'doc', content: content.length ? content : [{ type: 'paragraph' }] };
}

// ---- editor → blocks ----

function nodesToRuns(nodes: PMNode[] = []): { text: string; runs?: Run[] } {
  const runs: Run[] = [];
  for (const n of nodes) {
    if (n.type === 'hardBreak') { runs.push({ text: '\n' }); continue; }
    if (n.type !== 'text' || !n.text) continue;
    const r: Run = { text: n.text };
    for (const m of n.marks ?? []) {
      if (m.type === 'bold') r.bold = true;
      if (m.type === 'italic') r.italic = true;
      if (m.type === 'link' && typeof m.attrs?.href === 'string' && safeHref(m.attrs.href)) r.href = m.attrs.href;
    }
    const prev = runs[runs.length - 1];
    if (prev && prev.bold === r.bold && prev.italic === r.italic && prev.href === r.href) prev.text += r.text;
    else runs.push(r);
  }
  const text = runs.map((r) => r.text).join('');
  return runs.some((r) => r.bold || r.italic || r.href) ? { text, runs } : { text };
}

/** Plain text of a block container (list item, cell): its paragraphs joined. */
function containerText(n: PMNode): { text: string; runs?: Run[] } {
  const paras = (n.content ?? []).filter((c) => c.type === 'paragraph');
  if (paras.length <= 1) return nodesToRuns(paras[0]?.content);
  return { text: paras.map((p) => nodesToRuns(p.content).text).join('\n') };
}

export function fromEditor(doc: PMNode): Block[] {
  const out: Block[] = [];
  for (const n of doc.content ?? []) {
    switch (n.type) {
      case 'heading': {
        const r = nodesToRuns(n.content);
        if (r.text.trim()) out.push({ type: 'heading', level: Math.min(3, Math.max(1, Number(n.attrs?.level ?? 2) - 1)), ...r });
        break;
      }
      case 'bulletList':
      case 'orderedList': {
        const items = (n.content ?? []).map(containerText);
        if (!items.length) break;
        const b: Block = { type: 'list', items: items.map((i) => i.text) };
        if (n.type === 'orderedList') b.ordered = true;
        if (items.some((i) => i.runs)) b.item_runs = items.map((i) => i.runs ?? [{ text: i.text }]);
        out.push(b);
        break;
      }
      case 'procImage': {
        const fileId = String(n.attrs?.fileId ?? '');
        const alt = String(n.attrs?.alt ?? '').trim();
        if (fileId && alt) {
          const b: Block = { type: 'image', file_id: fileId, text: alt };
          const caption = String(n.attrs?.caption ?? '').trim();
          if (caption) b.caption = caption;
          out.push(b);
        }
        break;
      }
      case 'blockquote': {
        const r = containerText(n);
        if (r.text.trim()) out.push({ type: 'callout', tone: 'info', ...r });
        break;
      }
      case 'table': {
        const rows = (n.content ?? []).map((row) => (row.content ?? []).map((cell) => containerText(cell).text));
        if (rows.length) out.push({ type: 'table', header: (n.content?.[0]?.content ?? []).some((c) => c.type === 'tableHeader'), rows });
        break;
      }
      default: {
        const r = nodesToRuns(n.content);
        if (r.text.trim()) out.push({ type: 'paragraph', ...r });
      }
    }
  }
  return out;
}
