export function timeAgo(iso: string | null | undefined): string {
  if (!iso) return '';
  const s = (Date.now() - Date.parse(iso)) / 1000;
  const fut = s < 0;
  const a = Math.abs(s);
  const v = a < 60 ? 'just now' : a < 3600 ? `${Math.floor(a / 60)}m` : a < 86400 ? `${Math.floor(a / 3600)}h` : a < 86400 * 30 ? `${Math.floor(a / 86400)}d` : new Date(iso).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
  if (v === 'just now' || v.includes(' ')) return v;
  return fut ? `in ${v}` : `${v} ago`;
}

export function shortDate(iso: string | null | undefined): string {
  return iso ? new Date(iso).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' }) : '—';
}

export function slugify(s: string): string {
  return s.toLowerCase().normalize('NFKD').replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 64);
}

export interface Block { type: string; text?: string; level?: number; items?: string[] }

/** Plain-text editing format for blocks: "# " / "## " headings, "- " bullets, blank line between paragraphs. */
export function blocksToText(blocks: Block[] = []): string {
  return blocks
    .map((b) => (b.type === 'heading' ? `${'#'.repeat(b.level ?? 2)} ${b.text ?? ''}` : b.type === 'list' ? (b.items ?? []).map((i) => `- ${i}`).join('\n') : b.text ?? ''))
    .join('\n\n');
}

export function textToBlocks(text: string): Block[] {
  return text
    .split(/\n{2,}/)
    .map((chunk) => chunk.trim())
    .filter(Boolean)
    .map((chunk): Block => {
      const h = /^(#{1,3})\s+(.*)$/s.exec(chunk);
      if (h) return { type: 'heading', level: h[1]!.length, text: h[2]!.trim() };
      const lines = chunk.split('\n');
      if (lines.every((l) => /^[-*]\s+/.test(l))) return { type: 'list', items: lines.map((l) => l.replace(/^[-*]\s+/, '')) };
      return { type: 'paragraph', text: chunk.replace(/\n/g, ' ') };
    });
}
