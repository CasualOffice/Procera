import type { Block } from '../lib/format';

/** Renders block content as a readable document (docs/05 §7.0 reading view). */
export function DocBody({ blocks }: { blocks: Block[] }) {
  if (!blocks.length) return <p className="muted">This document is empty.</p>;
  return (
    <div className="doc-body">
      {blocks.map((b, i) => {
        if (b.type === 'heading') {
          const H = (b.level ?? 2) <= 1 ? 'h2' : 'h3';
          return <H key={i}>{b.text}</H>;
        }
        if (b.type === 'list') return <ul key={i}>{(b.items ?? []).map((it, j) => <li key={j}>{it}</li>)}</ul>;
        return <p key={i}>{b.text}</p>;
      })}
    </div>
  );
}
