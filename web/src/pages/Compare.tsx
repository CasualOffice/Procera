import { ArrowLeft, ArrowRight } from 'lucide-react';
import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { type LoadFailure, toFailure } from '../lib/data';
import { shortDate } from '../lib/format';
import { Link, navigate, useRoute } from '../lib/router';
import { setCrumb } from '../ui/AppShell';
import { Empty, LoadError, Skeleton } from '../ui/States';

interface Block { type: string; text?: string; level?: number; items?: string[] }
type Word = { op: 'same' | 'added' | 'removed'; text: string };
type Op = { op: 'same' | 'added' | 'removed'; block: Block } | { op: 'changed'; before: Block; after: Block; words: Word[] };
interface Diff { from: { id: string; version_no: number }; to: { id: string; version_no: number }; summary: { added: number; removed: number; changed: number }; ops: Op[] }
interface Artifact { artifact: { id: string; title: string }; versions: { id: string; version_no: number; lifecycle: string; published_at: string | null; change_reason: string | null }[] }

const n = (k: number, one: string, many: string) => `${k} ${k === 1 ? one : many}`;

/** Renders one block with an optional diff state; headings keep their level, lists their items. */
function BlockView({ b, state, children }: { b: Block; state: 'same' | 'added' | 'removed' | 'changed'; children?: React.ReactNode }) {
  const cls = `cmp-block cmp-${state}`;
  const label = state === 'added' ? 'Added: ' : state === 'removed' ? 'Removed: ' : state === 'changed' ? 'Changed: ' : '';
  const body = children ?? (b.type === 'list' ? <ul>{(b.items ?? []).map((i, k) => <li key={k}>{i}</li>)}</ul> : b.text);
  const sr = label ? <span className="visually-hidden">{label}</span> : null;
  if (b.type === 'heading') return (b.level ?? 2) <= 1 ? <h2 className={cls}>{sr}{body}</h2> : <h3 className={cls}>{sr}{body}</h3>;
  if (b.type === 'list' && !children) return <div className={cls}>{sr}{body}</div>;
  return <p className={cls}>{sr}{body}</p>;
}

/**
 * Version compare (BPMS-043, docs/05 §3.21): the newer version as a document, with added blocks in
 * green, removed blocks struck through in red and word-level changes marked inside changed blocks.
 * Colour is never alone: insertions are underlined, deletions struck through, and both are named
 * for screen readers.
 */
export function Compare({ id }: { id: string }) {
  const { query } = useRoute();
  const [art, setArt] = useState<Artifact | null>(null);
  const [diff, setDiff] = useState<Diff | null>(null);
  const [error, setError] = useState<LoadFailure | null>(null);
  const toNo = Number(query.get('to'));
  const fromNo = Number(query.get('from'));

  useEffect(() => {
    let live = true;
    setDiff(null); setError(null);
    (async () => {
      try {
        const a = await api<Artifact>(`/api/v1/artifacts/${id}`);
        if (!live) return;
        setArt(a);
        setCrumb(a.artifact.title);
        const vs = [...a.versions].sort((x, y) => x.version_no - y.version_no);
        const to = vs.find((v) => v.version_no === toNo) ?? vs[vs.length - 1];
        const from = vs.find((v) => v.version_no === fromNo) ?? vs[vs.indexOf(to!) - 1];
        if (!to || !from) { setError({ message: 'There is only one version, so there is nothing to compare yet.', status: 404 }); return; }
        const d = await api<Diff>(`/api/v1/versions/${from.id}/diff/${to.id}`);
        if (live) setDiff(d);
      } catch (e) { if (live) setError(toFailure(e)); }
    })();
    return () => { live = false; setCrumb(null); };
  }, [id, toNo, fromNo]);

  if (error) return <div className="doc">{error.status === 404 ? <Empty title="Nothing to compare" action={<Link className="btn btn-secondary link-btn" to={`/library/${id}`}>Back to the document</Link>}>{error.message === 'Not found' ? 'One of these versions isn’t available to you.' : error.message}</Empty> : <LoadError error={error} what="this comparison" />}</div>;
  if (!art || !diff) return <div className="doc"><Skeleton variant="doc" label="Loading comparison" /></div>;

  const vs = [...art.versions].sort((x, y) => x.version_no - y.version_no);
  const toV = vs.find((v) => v.id === diff.to.id);
  const fromV = vs.find((v) => v.id === diff.from.id);
  const s = diff.summary;
  const parts = [s.changed && n(s.changed, 'section changed', 'sections changed'), s.added && n(s.added, 'added', 'added'), s.removed && n(s.removed, 'removed', 'removed')].filter(Boolean);
  const pick = (which: 'from' | 'to', no: number) => navigate(`/library/${id}/compare?from=${which === 'from' ? no : diff.from.version_no}&to=${which === 'to' ? no : diff.to.version_no}`, { replace: true });

  return (
    <article className="doc cmp">
      <Link className="cmp-back" to={`/library/${id}`}><ArrowLeft aria-hidden="true" size="0.875rem" /> {art.artifact.title}</Link>
      <h1 className="doc-title">What changed</h1>
      <div className="cmp-bar">
        <label className="cmp-pick"><span className="visually-hidden">Older version</span>
          <select className="field-input" value={diff.from.version_no} onChange={(e) => pick('from', Number(e.target.value))}>
            {vs.filter((v) => v.version_no < diff.to.version_no).map((v) => <option key={v.id} value={v.version_no}>v{v.version_no}{v.published_at ? `, ${shortDate(v.published_at)}` : ''}</option>)}
          </select>
        </label>
        <ArrowRight aria-hidden="true" size="0.9375rem" className="muted" />
        <label className="cmp-pick"><span className="visually-hidden">Newer version</span>
          <select className="field-input" value={diff.to.version_no} onChange={(e) => pick('to', Number(e.target.value))}>
            {vs.filter((v) => v.version_no > diff.from.version_no).map((v) => <option key={v.id} value={v.version_no}>v{v.version_no}{v.published_at ? `, ${shortDate(v.published_at)}` : ''}</option>)}
          </select>
        </label>
        <p className="cmp-summary" role="status">{parts.length ? `${parts.join(', ')}.` : 'No changes to the content.'}</p>
      </div>
      {toV?.change_reason && <p className="cmp-reason"><span className="muted">Reason given for v{toV.version_no}:</span> {toV.change_reason}</p>}
      <div className="cmp-legend" aria-hidden="true"><span className="cmp-key cmp-key-add">Added</span><span className="cmp-key cmp-key-del">Removed</span>{fromV && <span className="muted">Compared with v{fromV.version_no}</span>}</div>
      <hr className="doc-rule" />
      <div className="doc-body">
        {diff.ops.map((o, i) => {
          if (o.op === 'changed') {
            return (
              <BlockView key={i} b={o.after} state="changed">
                {o.words.map((w, k) => <span key={k}>{k > 0 && ' '}{w.op === 'same' ? w.text : w.op === 'added' ? <ins>{w.text}</ins> : <del>{w.text}</del>}</span>)}
              </BlockView>
            );
          }
          return <BlockView key={i} b={o.block} state={o.op} />;
        })}
      </div>
    </article>
  );
}
