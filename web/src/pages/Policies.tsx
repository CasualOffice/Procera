import { BadgeCheck, FileText, Search } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { Input, SearchField } from 'react-aria-components';
import { useItems } from '../lib/data';
import { shortDate } from '../lib/format';
import { Link } from '../lib/router';
import { Empty, LoadError, Skeleton } from '../ui/States';
import { Tag } from '../ui/Tag';
import { LibraryTabs } from './areaTabs';

interface WhatChanged { from_version_no: number; added: number; removed: number; changed: number; change_reason: string | null }
interface PortalPolicy { id: string; key: string; title: string; category: string | null; version_no: number; published_at: string | null; verified: boolean; review_status: string; what_changed: WhatChanged | null }

const RECENT_DAYS = 30;
const parts = (w: WhatChanged) => [w.changed && `${w.changed} changed`, w.added && `${w.added} added`, w.removed && `${w.removed} removed`].filter(Boolean).join(', ');

/**
 * Policies (BPMS-045, FR-PL02): the published policies that apply to me, searchable and grouped
 * by category, each saying what changed in its latest version. Restricted policies that don't
 * apply to me never appear (server rule).
 */
export function Policies() {
  const [q, setQ] = useState('');
  const [term, setTerm] = useState('');
  const [category, setCategory] = useState('');
  useEffect(() => { const t = setTimeout(() => setTerm(q.trim()), 200); return () => clearTimeout(t); }, [q]);
  const params = new URLSearchParams();
  if (term) params.set('q', term);
  if (category) params.set('category', category);
  const [items, reload, error] = useItems<PortalPolicy>(`/api/v1/me/policies${params.toString() ? `?${params}` : ''}`);
  // The category list comes from the unfiltered load, so chips don't disappear while filtering.
  const [all] = useItems<PortalPolicy>('/api/v1/me/policies');
  const categories = useMemo(() => [...new Set((all ?? []).map((p) => p.category).filter((c): c is string => !!c))].sort(), [all]);
  const recent = (p: PortalPolicy) => p.version_no > 1 && p.published_at && Date.now() - Date.parse(p.published_at) < RECENT_DAYS * 86_400_000;

  return (
    <>
      <div className="page-head">
        <div><h1>Policies</h1><p>The policies that apply to you, always the current version.</p></div>
      </div>
      <LibraryTabs current="applies" />
      <div className="toolbar portal-toolbar">
        <SearchField className="search-input" aria-label="Search policies" value={q} onChange={setQ}>
          <Search aria-hidden="true" size="0.9375rem" /><Input placeholder="Search policies" />
        </SearchField>
        {categories.length > 0 && (
          <div className="aud-chips" role="group" aria-label="Category">
            <button type="button" className="aud-chip" aria-pressed={category === ''} onClick={() => setCategory('')}>All</button>
            {categories.map((c) => <button key={c} type="button" className="aud-chip" aria-pressed={category === c} onClick={() => setCategory(category === c ? '' : c)}>{c}</button>)}
          </div>
        )}
      </div>
      {error ? <LoadError error={error} onRetry={reload} what="policies" /> : items === null ? <Skeleton rows={4} label="Loading policies" /> : items.length === 0 ? (
        term || category ? <Empty art="no-match" title="No policies match" action={<button type="button" className="btn btn-secondary btn-md" onClick={() => { setQ(''); setCategory(''); }}>Clear search</button>}>Try another word or category.</Empty>
          : <Empty art="library" title="No policies yet">When a policy that applies to you is published, it shows up here.</Empty>
      ) : (
        <ul className="portal">
          {items.map((p) => (
            <li key={p.id} className="portal-item">
              <span className="doc-icon tag-purple"><FileText aria-hidden="true" size="0.9375rem" /></span>
              <div className="portal-main">
                <p className="portal-title">
                  <Link to={`/library/${p.id}`}>{p.title}</Link>
                  {p.verified && <Tag color="green"><BadgeCheck aria-hidden="true" size="0.75rem" /> Verified</Tag>}
                  {recent(p) && <Tag color="blue">Updated</Tag>}
                  {p.review_status === 'overdue' && <Tag color="red">Review overdue</Tag>}
                </p>
                <p className="portal-meta">
                  {p.category && <span>{p.category}</span>}
                  <span>v{p.version_no}</span>
                  {p.published_at && <span>published {shortDate(p.published_at)}</span>}
                </p>
                {p.what_changed && (
                  <p className="portal-changed">
                    <Link to={`/library/${p.id}/compare?from=${p.what_changed.from_version_no}&to=${p.version_no}`}>What changed in v{p.version_no}</Link>
                    <span className="muted">{parts(p.what_changed) || 'Wording only'}{p.what_changed.change_reason ? ` · ${p.what_changed.change_reason}` : ''}</span>
                  </p>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
