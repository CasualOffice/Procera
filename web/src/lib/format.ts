export function timeAgo(raw: unknown): string {
  const iso = toIso(raw);
  if (!iso) return '';
  const s = (Date.now() - Date.parse(iso)) / 1000;
  const fut = s < 0;
  const a = Math.abs(s);
  const v = a < 60 ? 'just now' : a < 3600 ? `${Math.floor(a / 60)}m` : a < 86400 ? `${Math.floor(a / 3600)}h` : a < 86400 * 30 ? `${Math.floor(a / 86400)}d` : new Date(iso).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
  if (v === 'just now' || v.includes(' ')) return v;
  return fut ? `in ${v}` : `${v} ago`;
}

/**
 * Normalises a timestamp to ISO 8601. Some list payloads still serialise `time::OffsetDateTime`
 * as its tuple form `[year, ordinal_day, hour, minute, second, nanos, off_h, off_m, off_s]`
 * (reported as an API gap); accept both so dates never render as "Invalid Date".
 */
export function toIso(v: unknown): string | null {
  if (typeof v === 'string') return v;
  if (Array.isArray(v) && v.length >= 6 && v.every((x) => typeof x === 'number')) {
    const [y, ord, h, m, s, ns, oh = 0, om = 0, os = 0] = v as number[];
    const ms = Date.UTC(y!, 0, ord!, h!, m!, s!, Math.floor(ns! / 1e6)) - ((oh * 3600 + om * 60 + os) * 1000);
    return new Date(ms).toISOString();
  }
  return null;
}

export function shortDate(iso: unknown): string {
  const v = toIso(iso);
  return v ? new Date(v).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' }) : '';
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
