import { useMemo, useState } from 'react';
import {
  type ColumnDef, type RowSelectionState, type SortingState, flexRender, getCoreRowModel, getFilteredRowModel, getSortedRowModel, useReactTable,
} from '@tanstack/react-table';
import { ArrowDown, ArrowUp, ArrowUpDown } from 'lucide-react';
import './datagrid.css';

/**
 * DataGrid shell (FR-V01–V09, docs/05 §4). This slice: client-side sort/filter/select over native
 * table semantics (caption, scope, aria-sort), a live region announcing result counts, and a card
 * layout below 640px. The server-driven query model (docs/04 §5) replaces the row models in R1.
 */
export function DataGrid<T extends { id: string }>({
  caption, data, columns, selectable = false,
}: { caption: string; data: T[]; columns: ColumnDef<T, unknown>[]; selectable?: boolean }) {
  const [sorting, setSorting] = useState<SortingState>([]);
  const [globalFilter, setGlobalFilter] = useState('');
  const [rowSelection, setRowSelection] = useState<RowSelectionState>({});
  const cols = useMemo<ColumnDef<T, unknown>[]>(() => {
    if (!selectable) return columns;
    return [{
      id: 'select', enableSorting: false,
      header: ({ table }) => (
        <input type="checkbox" aria-label="Select all rows" checked={table.getIsAllRowsSelected()}
          ref={(el) => { if (el) el.indeterminate = table.getIsSomeRowsSelected(); }} onChange={table.getToggleAllRowsSelectedHandler()} />
      ),
      cell: ({ row }) => (
        <input type="checkbox" aria-label={`Select row ${row.original.id}`} checked={row.getIsSelected()} onChange={row.getToggleSelectedHandler()} />
      ),
    }, ...columns];
  }, [columns, selectable]);
  const table = useReactTable({
    data, columns: cols, state: { sorting, globalFilter, rowSelection }, getRowId: (r) => r.id,
    onSortingChange: setSorting, onGlobalFilterChange: setGlobalFilter, onRowSelectionChange: setRowSelection,
    getCoreRowModel: getCoreRowModel(), getSortedRowModel: getSortedRowModel(), getFilteredRowModel: getFilteredRowModel(),
    enableMultiSort: true, isMultiSortEvent: (e) => (e as React.MouseEvent).shiftKey,
  });
  const rows = table.getRowModel().rows;
  const selected = Object.keys(rowSelection).length;
  return (
    <div className="grid">
      <div className="grid-toolbar">
        <label className="grid-search">
          <span className="field-label">Search</span>
          <input className="field-input" type="search" value={globalFilter} onChange={(e) => setGlobalFilter(e.target.value)} placeholder="Filter rows" />
        </label>
        <p className="grid-status" role="status" aria-live="polite">
          {rows.length} of {data.length} rows{selected ? ` · ${selected} selected` : ''}
        </p>
      </div>
      <div className="grid-scroll">
        <table className="grid-table">
          <caption>{caption}</caption>
          <thead>
            {table.getHeaderGroups().map((hg) => (
              <tr key={hg.id}>
                {hg.headers.map((h) => {
                  const sorted = h.column.getIsSorted();
                  const canSort = h.column.getCanSort();
                  return (
                    <th key={h.id} scope="col" aria-sort={sorted === 'asc' ? 'ascending' : sorted === 'desc' ? 'descending' : canSort ? 'none' : undefined}>
                      {canSort ? (
                        <button type="button" className="grid-sort" onClick={h.column.getToggleSortingHandler()}>
                          {flexRender(h.column.columnDef.header, h.getContext())}
                          {sorted === 'asc' ? <ArrowUp aria-hidden="true" size={14} /> : sorted === 'desc' ? <ArrowDown aria-hidden="true" size={14} /> : <ArrowUpDown aria-hidden="true" size={14} className="grid-sort-idle" />}
                        </button>
                      ) : flexRender(h.column.columnDef.header, h.getContext())}
                    </th>
                  );
                })}
              </tr>
            ))}
          </thead>
          <tbody>
            {rows.length === 0 && (
              <tr><td colSpan={cols.length} className="grid-empty">No rows match “{globalFilter}”. Clear the search to see all rows.</td></tr>
            )}
            {rows.map((r) => (
              <tr key={r.id} aria-selected={selectable ? r.getIsSelected() : undefined}>
                {r.getVisibleCells().map((c) => (
                  <td key={c.id} data-label={typeof c.column.columnDef.header === 'string' ? c.column.columnDef.header : ''}>
                    {flexRender(c.column.columnDef.cell, c.getContext())}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
