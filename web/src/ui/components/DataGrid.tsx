import { ArrowDown, ArrowUp, ArrowUpDown, ChevronDown, ChevronUp, Columns3, Download, Filter, Rows3, Save, X } from 'lucide-react';
import { useEffect, useId, useMemo, useRef, useState } from 'react';
import { api, downloadPost } from '../../lib/api';
import { navigate, useRoute } from '../../lib/router';
import { Button } from './Button';
import { FormDialog } from './FormDialog';
import { RadioCards } from './RadioCards';
import { TextField } from './TextField';
import './datagrid.css';

/**
 * DataGrid v1 (BPMS-028, FR-V01–V09, docs/05 §4). One grid for every list:
 * - quick search, a filter builder with chips and a plain-language summary (operators typed by field),
 *   one level of AND/OR groups (top-level AND of conditions and groups; each group matches all or any),
 *   multi-sort (shift-click, order numbers), a columns chooser (show, hide, move), density, CSV export
 *   (formula-safe, audited through `POST /exports`) and XLSX export (the same cells, built and audited by
 *   `POST /exports` with `format: xlsx`);
 * - saved views per list (`/views`): personal, shared with everyone or with one role, a default, a
 *   "modified" marker; the URL keeps the current state (`?grid=`), so a view can be linked;
 * - selection with shift-range and bulk actions supplied by the page; Enter opens a row, Space selects;
 *   counts are announced in a live region; below 40rem rows become cards.
 * v1 filters and sorts the rows it is given; the server-driven query model with cursor paging (docs/04 §5)
 * replaces that for very large lists.
 */

export type ColumnType = 'text' | 'enum' | 'number' | 'date' | 'boolean';
export interface GridColumn<T> {
  id: string;
  header: string;
  /** Plain value for sort, filter, search and export. */
  value: (row: T) => string | number | boolean | null | undefined;
  /** Rich cell (defaults to the value). */
  cell?: (row: T) => React.ReactNode;
  type?: ColumnType;
  /** Choices for enum filters (defaults to the distinct values present). */
  options?: string[];
  sortable?: boolean;
  /** Hidden unless chosen in Columns. */
  hidden?: boolean;
  /** Dropped first on narrow screens. */
  optional?: boolean;
  className?: string;
}

export type Op = 'contains' | 'is' | 'is_not' | 'empty' | 'not_empty' | 'eq' | 'neq' | 'lt' | 'gt' | 'any_of' | 'none_of' | 'before' | 'after' | 'last_days' | 'true' | 'false';
export interface GridFilter { id: string; op: Op; value?: string | string[] }
/** One level of grouping: a group matches when all (and) or any (or) of its conditions match. */
export interface FilterGroup { mode: 'and' | 'or'; filters: GridFilter[] }
/**
 * `filters` are ANDed (v1); `groups` (optional, so v1 saved views and `?grid=` links still load) are
 * ANDed with them, each group matching all or any of its own conditions.
 */
export interface GridState { q: string; sort: { id: string; desc: boolean }[]; filters: GridFilter[]; groups?: FilterGroup[]; columns: string[]; density: 'comfortable' | 'compact' }
interface SavedView { id: string; name: string; state: GridState; shared: boolean; shared_role?: string | null; shared_role_name?: string | null; is_default: boolean; mine: boolean }
interface ShareRole { key: string; name: string }

const OPS: Record<ColumnType, [Op, string][]> = {
  text: [['contains', 'contains'], ['is', 'is'], ['is_not', 'is not'], ['empty', 'is empty'], ['not_empty', 'is not empty']],
  enum: [['any_of', 'is any of'], ['none_of', 'is none of'], ['empty', 'is empty']],
  number: [['eq', '='], ['neq', '≠'], ['lt', '<'], ['gt', '>']],
  date: [['last_days', 'in the last N days'], ['before', 'before'], ['after', 'after'], ['empty', 'is empty']],
  boolean: [['true', 'is yes'], ['false', 'is no']],
};
const NEEDS_VALUE = new Set<Op>(['contains', 'is', 'is_not', 'eq', 'neq', 'lt', 'gt', 'any_of', 'none_of', 'before', 'after', 'last_days']);

function matches<T>(col: GridColumn<T>, row: T, f: GridFilter): boolean {
  const raw = col.value(row);
  const s = raw === null || raw === undefined ? '' : String(raw);
  const v = Array.isArray(f.value) ? f.value : f.value ?? '';
  switch (f.op) {
    case 'contains': return s.toLowerCase().includes(String(v).toLowerCase());
    case 'is': return s.toLowerCase() === String(v).toLowerCase();
    case 'is_not': return s.toLowerCase() !== String(v).toLowerCase();
    case 'empty': return s === '';
    case 'not_empty': return s !== '';
    case 'eq': return Number(raw) === Number(v);
    case 'neq': return Number(raw) !== Number(v);
    case 'lt': return Number(raw) < Number(v);
    case 'gt': return Number(raw) > Number(v);
    case 'any_of': return (v as string[]).includes(s);
    case 'none_of': return !(v as string[]).includes(s);
    case 'before': return s !== '' && Date.parse(s) < Date.parse(String(v));
    case 'after': return s !== '' && Date.parse(s) > Date.parse(String(v));
    case 'last_days': return s !== '' && Date.now() - Date.parse(s) <= Number(v) * 86_400_000 && Date.parse(s) <= Date.now();
    case 'true': return raw === true;
    case 'false': return raw === false;
  }
}

export function describe<T>(cols: GridColumn<T>[], f: GridFilter): string {
  const col = cols.find((c) => c.id === f.id);
  if (!col) return '';
  const op = OPS[col.type ?? 'text'].find(([o]) => o === f.op)?.[1] ?? f.op;
  const v = Array.isArray(f.value) ? f.value.join(' or ') : f.value;
  return `${col.header} ${op}${NEEDS_VALUE.has(f.op) && v ? ` ${f.op === 'last_days' ? `${v}` : `“${v}”`}` : ''}`.replace('in the last N days 0', 'today').replace(/in the last N days (\d+)/, 'in the last $1 days');
}

/** Row test for the whole filter state: top-level conditions AND every non-empty group. */
function passes<T>(cols: GridColumn<T>[], row: T, s: GridState): boolean {
  const one = (f: GridFilter) => { const c = cols.find((x) => x.id === f.id); return !c || matches(c, row, f); };
  return s.filters.every(one) && (s.groups ?? []).every((g) => g.filters.length === 0 || (g.mode === 'or' ? g.filters.some(one) : g.filters.every(one)));
}

/** The plain-language summary: `Status is “Published” and (Owner is “Ana” or Owner is “Ben”)`. */
export function summarize<T>(cols: GridColumn<T>[], s: GridState): string {
  const groups = (s.groups ?? []).filter((g) => g.filters.length > 0).map((g) => {
    const inner = g.filters.map((f) => describe(cols, f)).join(g.mode === 'or' ? ' or ' : ' and ');
    return g.filters.length > 1 ? `(${inner})` : inner;
  });
  return [...s.filters.map((f) => describe(cols, f)), ...groups].join(' and ');
}

const filterCount = (s: GridState) => s.filters.length + (s.groups ?? []).reduce((n, g) => n + g.filters.length, 0);
const modeWord = (m: FilterGroup['mode']) => (m === 'or' ? 'any' : 'all');

function csvCell(v: unknown): string {
  let s = v === null || v === undefined ? '' : String(v);
  if (/^[=+\-@\t\r]/.test(s)) s = `'${s}`; // spreadsheet formula injection (docs/06 §5)
  return `"${s.replace(/"/g, '""')}"`;
}

const encode = (s: GridState) => btoa(unescape(encodeURIComponent(JSON.stringify(s)))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const decode = (g: string): GridState | null => { try { return JSON.parse(decodeURIComponent(escape(atob(g.replace(/-/g, '+').replace(/_/g, '/'))))); } catch { return null; } };

export function DataGrid<T extends { id: string }>({
  caption, data, columns, list, selectable = false, rowHref, rowLabel, bulkActions, empty, initialSort = [], initialSearch = '', showCaption = true,
}: {
  caption: string;
  data: T[];
  columns: GridColumn<T>[];
  /** Enables saved views, URL state and audited export under this list name (e.g. `library.documents`). */
  list?: string;
  selectable?: boolean;
  rowHref?: (row: T) => string;
  /** Names a row for its selection checkbox ("Select Nina"); defaults to the row id. */
  rowLabel?: (row: T) => string;
  bulkActions?: (selected: T[], clear: () => void) => React.ReactNode;
  /** Shown when there is no data at all (distinct from "no matches"). */
  empty?: React.ReactNode;
  initialSort?: { id: string; desc: boolean }[];
  /** Seeds the quick search (e.g. from a `?q=` deep link). */
  initialSearch?: string;
  showCaption?: boolean;
}) {
  const { query } = useRoute();
  const defaults: GridState = useMemo(() => ({ q: initialSearch, sort: initialSort, filters: [], groups: [], columns: columns.filter((c) => !c.hidden).map((c) => c.id), density: 'comfortable' }), [columns, initialSort]);
  const fromUrl = list ? decode(query.get('grid') ?? '') : null;
  const [state, setStateRaw] = useState<GridState>(fromUrl ? { ...defaults, ...fromUrl } : defaults);
  const [views, setViews] = useState<SavedView[]>([]);
  const [viewId, setViewId] = useState<string | null>(list ? query.get('view') : null);
  const [panel, setPanel] = useState<'filter' | 'columns' | null>(null);
  const [savingAs, setSavingAs] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [exportError, setExportError] = useState<string | null>(null);
  const anchor = useRef<number | null>(null);
  const summaryId = useId();

  // Saved views: load, then apply the requested or default view unless the URL already has a state.
  useEffect(() => {
    if (!list) return;
    api<{ items: SavedView[] }>(`/api/v1/views?list=${encodeURIComponent(list)}`).then((r) => {
      setViews(r.items);
      if (fromUrl) return;
      const v = r.items.find((x) => x.id === viewId) ?? r.items.find((x) => x.is_default);
      if (v) { setViewId(v.id); setStateRaw({ ...defaults, ...v.state }); }
    }).catch(() => setViews([]));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [list]);

  const setState = (s: GridState) => {
    setStateRaw(s);
    if (!list) return;
    const q = new URLSearchParams(query);
    q.set('grid', encode(s));
    if (viewId) q.set('view', viewId);
    navigate(`${window.location.pathname}?${q}`, { replace: true });
  };

  const current = views.find((v) => v.id === viewId) ?? null;
  const modified = current ? JSON.stringify({ ...defaults, ...current.state }) !== JSON.stringify(state) : JSON.stringify(state) !== JSON.stringify(defaults);
  const visibleCols = state.columns.map((id) => columns.find((c) => c.id === id)).filter((c): c is GridColumn<T> => !!c);

  const rows = useMemo(() => {
    let out = data.filter((r) => passes(columns, r, state));
    if (state.q.trim()) {
      const q = state.q.trim().toLowerCase();
      out = out.filter((r) => columns.some((c) => String(c.value(r) ?? '').toLowerCase().includes(q)));
    }
    if (state.sort.length) {
      out = [...out].sort((a, b) => {
        for (const s of state.sort) {
          const c = columns.find((x) => x.id === s.id);
          if (!c) continue;
          const av = c.value(a), bv = c.value(b);
          const cmp = typeof av === 'number' && typeof bv === 'number' ? av - bv : String(av ?? '').localeCompare(String(bv ?? ''), undefined, { numeric: true, sensitivity: 'base' });
          if (cmp) return s.desc ? -cmp : cmp;
        }
        return 0;
      });
    }
    return out;
  }, [data, columns, state]);

  const toggleSort = (id: string, multi: boolean) => {
    const cur = state.sort.find((s) => s.id === id);
    let next = multi ? state.sort.filter((s) => s.id !== id) : [];
    if (!cur) next = [...next, { id, desc: false }];
    else if (!cur.desc) next = [...next, { id, desc: true }];
    setState({ ...state, sort: next });
  };

  const selectRow = (index: number, shift: boolean) => {
    const id = rows[index]!.id;
    const next = new Set(selected);
    if (shift && anchor.current !== null) {
      const [a, b] = [Math.min(anchor.current, index), Math.max(anchor.current, index)];
      const on = !selected.has(id);
      for (let i = a; i <= b; i++) { if (on) next.add(rows[i]!.id); else next.delete(rows[i]!.id); }
    } else if (next.has(id)) next.delete(id); else next.add(id);
    anchor.current = index;
    setSelected(next);
  };
  const allSelected = rows.length > 0 && rows.every((r) => selected.has(r.id));
  const someSelected = rows.some((r) => selected.has(r.id));

  const exportCsv = async () => {
    const head = visibleCols.map((c) => csvCell(c.header)).join(',');
    const body = rows.map((r) => visibleCols.map((c) => csvCell(c.value(r))).join(',')).join('\n');
    const blob = new Blob([`${head}\n${body}\n`], { type: 'text/csv;charset=utf-8' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `${(list ?? caption).replace(/[^a-z0-9]+/gi, '-').toLowerCase()}.csv`;
    a.click();
    URL.revokeObjectURL(a.href);
    if (list) await api('/api/v1/exports', { method: 'POST', body: { ...exportBody('csv'), list } }).catch(() => undefined);
  };
  const exportBody = (format: 'csv' | 'xlsx') => ({ format, rows: rows.length, columns: visibleCols.map((c) => c.id), filters: state.filters, ...(state.groups?.length ? { groups: state.groups } : {}) });
  /** XLSX is built by the server from exactly the cells CSV would contain (same columns, rows, order and values). */
  const exportXlsx = async () => {
    if (!list) return;
    const cell = (v: unknown) => (v === null || v === undefined ? null : typeof v === 'number' || typeof v === 'boolean' ? v : String(v));
    const body = { ...exportBody('xlsx'), list, header: visibleCols.map((c) => c.header), cells: rows.map((r) => visibleCols.map((c) => cell(c.value(r)))) };
    await downloadPost('/api/v1/exports', body, `${list.replace(/[^a-z0-9]+/gi, '-').toLowerCase()}.xlsx`);
  };

  const saveCurrent = async () => {
    if (!current?.mine) { setSavingAs(true); return; }
    const v = await api<SavedView>(`/api/v1/views/${current.id}`, { method: 'PATCH', body: { state } });
    setViews(views.map((x) => (x.id === v.id ? v : x)));
  };
  const applyView = (id: string) => {
    const v = views.find((x) => x.id === id);
    setViewId(v?.id ?? null);
    const s = v ? { ...defaults, ...v.state } : defaults;
    setStateRaw(s);
    const q = new URLSearchParams(query);
    q.delete('grid');
    if (v) q.set('view', v.id); else q.delete('view');
    navigate(`${window.location.pathname}${q.toString() ? `?${q}` : ''}`, { replace: true });
  };

  const groups = state.groups ?? [];
  /** Replaces group `gi`; a group left without conditions disappears. */
  const setGroup = (gi: number, g: FilterGroup) => setState({ ...state, groups: g.filters.length ? groups.map((x, j) => (j === gi ? g : x)) : groups.filter((_, j) => j !== gi) });
  const addFilter = (f: GridFilter, to: AddTo) => {
    if (to === 'all') setState({ ...state, filters: [...state.filters, f] });
    else if (typeof to === 'number' && groups[to]) setGroup(to, { ...groups[to], filters: [...groups[to].filters, f] });
    else setState({ ...state, groups: [...groups, { mode: to === 'new-and' ? 'and' : 'or', filters: [f] }] });
  };

  const selectedRows = rows.filter((r) => selected.has(r.id));
  return (
    <div className={`grid density-${state.density}`}>
      <div className="grid-toolbar">
        <label className="grid-search">
          <span className="field-label visually-hidden">Search</span>
          <input className="field-input" type="search" aria-label="Search" value={state.q} onChange={(e) => setState({ ...state, q: e.target.value })} placeholder="Search…" />
        </label>
        {list && (
          <span className="grid-views">
            <label className="visually-hidden" htmlFor={`${list}-view`}>View</label>
            <select id={`${list}-view`} className="field-input grid-view-select" value={viewId ?? ''} onChange={(e) => applyView(e.target.value)}>
              <option value="">All rows</option>
              {views.map((v) => <option key={v.id} value={v.id}>{v.name}{v.shared ? ' (shared)' : v.shared_role ? ` (shared with ${v.shared_role_name ?? v.shared_role})` : ''}{v.is_default ? ' · default' : ''}</option>)}
            </select>
            {modified && <span className="grid-modified" title="Changed since the view was saved"><span aria-hidden="true">●</span><span className="visually-hidden">Modified</span></span>}
            {modified && <button type="button" className="grid-tool" onClick={() => void saveCurrent()}><Save aria-hidden="true" size="0.875rem" /> {current?.mine ? 'Save' : 'Save view'}</button>}
            {current?.mine && <button type="button" className="grid-tool" onClick={() => setSavingAs(true)}>Save as…</button>}
          </span>
        )}
        <span className="grid-tools">
          <button type="button" className="grid-tool" aria-expanded={panel === 'filter'} onClick={() => setPanel(panel === 'filter' ? null : 'filter')}><Filter aria-hidden="true" size="0.875rem" /> Filter{filterCount(state) ? ` (${filterCount(state)})` : ''}</button>
          <button type="button" className="grid-tool" aria-expanded={panel === 'columns'} onClick={() => setPanel(panel === 'columns' ? null : 'columns')}><Columns3 aria-hidden="true" size="0.875rem" /> Columns</button>
          <button type="button" className="grid-tool" aria-pressed={state.density === 'compact'} onClick={() => setState({ ...state, density: state.density === 'compact' ? 'comfortable' : 'compact' })}><Rows3 aria-hidden="true" size="0.875rem" /> Compact</button>
          <button type="button" className="grid-tool" onClick={() => void exportCsv()}><Download aria-hidden="true" size="0.875rem" /> Export</button>
          {list && <button type="button" className="grid-tool" aria-label="Export XLSX" title="Export the rows and columns shown as an Excel workbook" onClick={() => { setExportError(null); exportXlsx().catch((e: Error) => setExportError(`The export failed: ${e.message}`)); }}>XLSX</button>}
        </span>
      </div>

      {filterCount(state) > 0 && (
        <>
          <div className="grid-chips" role="group" aria-label="Active filters" aria-describedby={summaryId}>
            {state.filters.map((f, i) => (
              <span key={`${f.id}-${i}`} className="grid-chip">
                {describe(columns, f)}
                <button type="button" aria-label={`Remove filter: ${describe(columns, f)}`} onClick={() => setState({ ...state, filters: state.filters.filter((_, j) => j !== i) })}><X aria-hidden="true" size="0.75rem" /></button>
              </span>
            ))}
            {groups.map((g, gi) => g.filters.length > 0 && (
              <span key={gi} className="grid-chip-group" role="group" aria-label={`Filter group ${gi + 1}: match ${modeWord(g.mode)}`}>
                <button type="button" className="grid-group-mode" aria-label={`Group ${gi + 1}: ${modeWord(g.mode)} of, switch to ${modeWord(g.mode === 'or' ? 'and' : 'or')} of`} title="Switch between matching any and all of these conditions" onClick={() => setGroup(gi, { ...g, mode: g.mode === 'or' ? 'and' : 'or' })}>{modeWord(g.mode)} of</button>
                {g.filters.map((f, i) => (
                  <span key={`${f.id}-${i}`} className="grid-chip-wrap">
                    {i > 0 && <span className="grid-join" aria-hidden="true">{g.mode}</span>}
                    <span className="grid-chip">
                      {describe(columns, f)}
                      <button type="button" aria-label={`Remove filter: ${describe(columns, f)}`} onClick={() => setGroup(gi, { ...g, filters: g.filters.filter((_, j) => j !== i) })}><X aria-hidden="true" size="0.75rem" /></button>
                    </span>
                  </span>
                ))}
                <button type="button" className="grid-group-remove" aria-label={`Remove group ${gi + 1}`} onClick={() => setState({ ...state, groups: groups.filter((_, j) => j !== gi) })}><X aria-hidden="true" size="0.75rem" /></button>
              </span>
            ))}
            <button type="button" className="grid-clear" onClick={() => setState({ ...state, filters: [], groups: [] })}>Clear all</button>
          </div>
          <p className="grid-summary" id={summaryId}>Showing rows where {summarize(columns, state)}</p>
        </>
      )}
      {panel === 'filter' && <FilterBuilder columns={columns} data={data} groups={groups} onAdd={(f, to) => { addFilter(f, to); setPanel(null); }} onClose={() => setPanel(null)} />}
      {panel === 'columns' && <ColumnChooser columns={columns} chosen={state.columns} onChange={(c) => setState({ ...state, columns: c })} />}

      <div className="grid-statusbar">
        <p className="grid-status" role="status" aria-live="polite">{rows.length} of {data.length} rows{selected.size ? ` · ${selected.size} selected` : ''}</p>
        {exportError && <p className="grid-error" role="alert">{exportError}</p>}
        {selectable && selected.size > 0 && bulkActions && <span className="grid-bulk">{bulkActions(selectedRows, () => setSelected(new Set()))}</span>}
      </div>

      <div className="grid-scroll">
        <table className="grid-table">
          <caption className={showCaption ? undefined : 'visually-hidden'}>{caption}</caption>
          <thead>
            <tr>
              {selectable && <th scope="col" className="grid-select"><input type="checkbox" aria-label="Select all rows" checked={allSelected} ref={(el) => { if (el) el.indeterminate = someSelected && !allSelected; }} onChange={() => setSelected(allSelected ? new Set() : new Set(rows.map((r) => r.id)))} /></th>}
              {visibleCols.map((c) => {
                const si = state.sort.findIndex((s) => s.id === c.id);
                const s = si >= 0 ? state.sort[si] : undefined;
                const can = c.sortable !== false;
                return (
                  <th key={c.id} scope="col" className={[c.optional ? 'col-optional' : '', c.className ?? ''].join(' ')} aria-sort={s ? (s.desc ? 'descending' : 'ascending') : can ? 'none' : undefined}>
                    {can ? (
                      <button type="button" className="grid-sort" onClick={(e) => toggleSort(c.id, e.shiftKey)} title="Click to sort; shift-click to add to the sort">
                        {c.header}
                        {s ? (s.desc ? <ArrowDown aria-hidden="true" size="0.875rem" /> : <ArrowUp aria-hidden="true" size="0.875rem" />) : <ArrowUpDown aria-hidden="true" size="0.875rem" className="grid-sort-idle" />}
                        {s && state.sort.length > 1 && <span className="grid-sort-order" aria-label={`sort ${si + 1}`}>{si + 1}</span>}
                      </button>
                    ) : c.header}
                  </th>
                );
              })}
            </tr>
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr><td colSpan={visibleCols.length + (selectable ? 1 : 0)} className="grid-empty">
                {data.length === 0 && empty ? empty : <>No rows match. <button type="button" className="grid-clear" onClick={() => setState({ ...state, q: '', filters: [], groups: [] })}>Clear search and filters</button></>}
              </td></tr>
            )}
            {rows.map((r, i) => (
              <tr key={r.id} aria-selected={selectable ? selected.has(r.id) : undefined} tabIndex={rowHref || selectable ? 0 : undefined}
                onKeyDown={(e) => {
                  if (e.target !== e.currentTarget) return;
                  if (e.key === 'Enter' && rowHref) { e.preventDefault(); navigate(rowHref(r)); }
                  if (e.key === ' ' && selectable) { e.preventDefault(); selectRow(i, e.shiftKey); }
                }}>
                {selectable && <td className="grid-select"><input type="checkbox" aria-label={rowLabel ? `Select ${rowLabel(r)}` : `Select row ${r.id}`} checked={selected.has(r.id)} onClick={(e) => { e.preventDefault(); selectRow(i, e.shiftKey); }} onChange={() => undefined} /></td>}
                {visibleCols.map((c, ci) => {
                  const v = c.value(r);
                  const content = c.cell ? c.cell(r) : String(v ?? '');
                  const empty = content === '' || content === null || content === undefined || content === false;
                  return (
                    <td key={c.id} className={[c.optional ? 'col-optional' : '', ci === 0 ? 'col-first' : '', c.type === 'date' ? 'col-date' : '', c.className ?? ''].join(' ')}
                      data-label={ci === 0 ? '' : c.header} data-empty={empty ? '' : undefined}>
                      {content}
                    </td>
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {list && savingAs && <SaveViewDialog list={list} state={state} onClose={() => setSavingAs(false)} onSaved={(v) => { setSavingAs(false); setViews([...views.filter((x) => !(v.is_default && x.is_default && x.mine) || x.id === v.id).map((x) => (v.is_default && x.mine ? { ...x, is_default: false } : x)), v]); setViewId(v.id); const q = new URLSearchParams(query); q.delete('grid'); q.set('view', v.id); navigate(`${window.location.pathname}?${q}`, { replace: true }); }} />}
    </div>
  );
}

/** Where a new condition goes: the top-level AND, an existing group, or a new group. */
type AddTo = 'all' | 'new-or' | 'new-and' | number;

function FilterBuilder<T>({ columns, data, groups, onAdd, onClose }: { columns: GridColumn<T>[]; data: T[]; groups: FilterGroup[]; onAdd: (f: GridFilter, to: AddTo) => void; onClose: () => void }) {
  const [to, setTo] = useState<string>('all');
  const [id, setId] = useState(columns[0]?.id ?? '');
  const col = columns.find((c) => c.id === id);
  const type = col?.type ?? 'text';
  const [op, setOp] = useState<Op>(OPS[type][0]![0]);
  const [value, setValue] = useState('');
  const [picked, setPicked] = useState<string[]>([]);
  useEffect(() => { setOp(OPS[type][0]![0]); setValue(''); setPicked([]); }, [id, type]);
  const options = col?.options ?? [...new Set(data.map((r) => String(col?.value(r) ?? '')).filter(Boolean))].sort();
  const multi = op === 'any_of' || op === 'none_of';
  const ready = !NEEDS_VALUE.has(op) || (multi ? picked.length > 0 : value.trim() !== '');
  return (
    <form className="grid-panel" aria-label="Add a filter" onSubmit={(e) => { e.preventDefault(); if (ready) onAdd({ id, op, value: multi ? picked : NEEDS_VALUE.has(op) ? value.trim() : undefined }, /^\d+$/.test(to) ? Number(to) : (to as AddTo)); }}>
      <label className="grid-field"><span className="field-label">Add to</span>
        <select className="field-input" value={to} onChange={(e) => setTo(e.target.value)}>
          <option value="all">All rows must match (and)</option>
          {groups.map((g, i) => <option key={i} value={String(i)}>{`Group ${i + 1} (match ${modeWord(g.mode)})`}</option>)}
          <option value="new-or">New group: match any (or)</option>
          <option value="new-and">New group: match all (and)</option>
        </select>
      </label>
      <label className="grid-field"><span className="field-label">Field</span>
        <select className="field-input" value={id} onChange={(e) => setId(e.target.value)}>{columns.map((c) => <option key={c.id} value={c.id}>{c.header}</option>)}</select>
      </label>
      <label className="grid-field"><span className="field-label">Condition</span>
        <select className="field-input" value={op} onChange={(e) => setOp(e.target.value as Op)}>{OPS[type].map(([o, l]) => <option key={o} value={o}>{l}</option>)}</select>
      </label>
      {NEEDS_VALUE.has(op) && (multi ? (
        <fieldset className="grid-field grid-options"><legend className="field-label">Values</legend>
          {options.map((o) => <label key={o} className="check-row"><input type="checkbox" checked={picked.includes(o)} onChange={(e) => setPicked(e.target.checked ? [...picked, o] : picked.filter((x) => x !== o))} /><span>{o}</span></label>)}
        </fieldset>
      ) : (
        <label className="grid-field"><span className="field-label">{op === 'last_days' ? 'Days' : 'Value'}</span>
          <input className="field-input" type={type === 'number' || op === 'last_days' ? 'number' : type === 'date' ? 'date' : 'text'} value={value} onChange={(e) => setValue(e.target.value)} autoFocus />
        </label>
      ))}
      <span className="grid-panel-actions"><Button type="submit" isDisabled={!ready}>Add filter</Button><Button variant="secondary" onPress={onClose}>Cancel</Button></span>
    </form>
  );
}

function ColumnChooser<T>({ columns, chosen, onChange }: { columns: GridColumn<T>[]; chosen: string[]; onChange: (c: string[]) => void }) {
  const order = [...chosen, ...columns.map((c) => c.id).filter((id) => !chosen.includes(id))];
  const move = (id: string, d: -1 | 1) => {
    const shown = [...chosen];
    const i = shown.indexOf(id);
    const j = i + d;
    if (i < 0 || j < 0 || j >= shown.length) return;
    [shown[i], shown[j]] = [shown[j]!, shown[i]!];
    onChange(shown);
  };
  return (
    <div className="grid-panel" role="group" aria-label="Columns">
      <ul className="grid-columns">
        {order.map((id) => {
          const c = columns.find((x) => x.id === id)!;
          const on = chosen.includes(id);
          return (
            <li key={id}>
              <label className="check-row"><input type="checkbox" checked={on} disabled={on && chosen.length === 1} onChange={(e) => onChange(e.target.checked ? [...chosen, id] : chosen.filter((x) => x !== id))} /><span>{c.header}</span></label>
              {on && <span className="grid-move"><button type="button" aria-label={`Move ${c.header} up`} onClick={() => move(id, -1)}><ChevronUp aria-hidden="true" size="0.875rem" /></button><button type="button" aria-label={`Move ${c.header} down`} onClick={() => move(id, 1)}><ChevronDown aria-hidden="true" size="0.875rem" /></button></span>}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function SaveViewDialog({ list, state, onClose, onSaved }: { list: string; state: GridState; onClose: () => void; onSaved: (v: SavedView) => void }) {
  const [name, setName] = useState('');
  const [audience, setAudience] = useState<'me' | 'everyone' | 'role'>('me');
  const [roles, setRoles] = useState<ShareRole[] | null>(null);
  const [role, setRole] = useState('');
  const [isDefault, setIsDefault] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (audience !== 'role' || roles) return;
    api<{ items: ShareRole[] }>('/api/v1/views/roles').then((r) => { setRoles(r.items); setRole((cur) => cur || r.items[0]?.key || ''); }).catch(() => setRoles([]));
  }, [audience, roles]);
  const ready = !!name.trim() && (audience !== 'role' || !!role);
  const save = async () => {
    setError(null);
    try {
      onSaved(await api<SavedView>('/api/v1/views', { method: 'POST', body: { list, name: name.trim(), state, shared: audience === 'everyone', ...(audience === 'role' ? { shared_role: role } : {}), is_default: isDefault } }));
    } catch (e) { setError((e as Error).message); }
  };
  return (
    <FormDialog isOpen onOpenChange={(o) => !o && onClose()} title="Save view" description="Keeps the search, filters, sort and columns. A shared view shows the same rows to each person only as far as they may see them."
      footer={<><Button variant="secondary" onPress={onClose}>Cancel</Button><Button onPress={() => void save()} isDisabled={!ready}>Save view</Button></>}>
      <TextField label="Name" value={name} onChange={setName} autoFocus />
      <RadioCards label="Share with" value={audience} onChange={(v) => setAudience(v as typeof audience)} options={[
        { id: 'me', label: 'Only me', description: 'A personal view.' },
        { id: 'everyone', label: 'Everyone', description: 'Everyone who can open this list.' },
        { id: 'role', label: 'A role…', description: 'The members who hold one role.' },
      ]} />
      {audience === 'role' && (
        <label className="grid-field"><span className="field-label">Role</span>
          <select className="field-input" value={role} onChange={(e) => setRole(e.target.value)} disabled={!roles}>
            {!roles && <option value="">Loading roles…</option>}
            {roles?.length === 0 && <option value="">No roles to share with</option>}
            {roles?.map((r) => <option key={r.key} value={r.key}>{r.name}</option>)}
          </select>
        </label>
      )}
      {error && <p className="grid-error" role="alert">{error}</p>}
      <label className="check-row"><input type="checkbox" checked={isDefault} onChange={(e) => setIsDefault(e.target.checked)} /><span><strong>Open this view by default</strong></span></label>
    </FormDialog>
  );
}
