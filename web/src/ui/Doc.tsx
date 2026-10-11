import type { Block, Run } from '../lib/blocks';
import './doc-editor.css';

function Runs({ text, runs }: { text?: string; runs?: Run[] }) {
  if (!runs?.length) return <>{text}</>;
  return (
    <>
      {runs.map((r, i) => {
        let node: React.ReactNode = r.text;
        if (r.bold) node = <strong>{node}</strong>;
        if (r.italic) node = <em>{node}</em>;
        if (r.href && (/^(https?:|mailto:)/i.test(r.href) || (r.href.startsWith('/') && !r.href.startsWith('//')))) node = <a href={r.href} rel="noopener noreferrer">{node}</a>;
        return <span key={i}>{node}</span>;
      })}
    </>
  );
}

/** Renders the block model (ADR-0013) as accessible HTML. Heading levels 1–3 → h2–h4 under the page title. */
export function DocBody({ blocks }: { blocks: Block[] }) {
  if (!blocks.length) return <p className="muted">This document is empty.</p>;
  return (
    <div className="doc-body">
      {blocks.map((b, i) => {
        switch (b.type) {
          case 'heading': {
            const H: 'h2' | 'h3' | 'h4' = b.level === 1 ? 'h2' : b.level === 3 ? 'h4' : 'h3';
            return <H key={i}><Runs text={b.text} runs={b.runs} /></H>;
          }
          case 'list': {
            const L = b.ordered ? 'ol' : 'ul';
            return <L key={i}>{(b.items ?? []).map((it, j) => <li key={j}><Runs text={it} runs={b.item_runs?.[j]} /></li>)}</L>;
          }
          case 'callout':
            return <aside key={i} className={`doc-callout ${b.tone ?? 'info'}`} aria-label={b.tone === 'warning' ? 'Warning' : 'Note'}><p><Runs text={b.text} runs={b.runs} /></p></aside>;
          case 'table': {
            const rows = b.rows ?? [];
            const head = b.header !== false && rows.length > 0;
            return (
              <table key={i}>
                {head && <thead><tr>{rows[0]!.map((c, j) => <th key={j} scope="col">{c}</th>)}</tr></thead>}
                <tbody>{rows.slice(head ? 1 : 0).map((r, k) => <tr key={k}>{r.map((c, j) => <td key={j}>{c}</td>)}</tr>)}</tbody>
              </table>
            );
          }
          default:
            return <p key={i}><Runs text={b.text} runs={b.runs} /></p>;
        }
      })}
    </div>
  );
}
