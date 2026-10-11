/**
 * Completion by group as a sortable-looking table with a horizontal bar per row (docs/05 §3.24):
 * the numbers are always printed, so the bar is never the only carrier of meaning.
 */
export function Bars({ caption, rows, unit }: { caption: string; rows: { label: string; value: number; total: number }[]; unit: string }) {
  const sorted = [...rows].sort((a, b) => b.total - a.total);
  return (
    <table className="bars">
      <caption>{caption}</caption>
      <thead className="visually-hidden"><tr><th scope="col">Group</th><th scope="col">Progress</th><th scope="col">Count</th></tr></thead>
      <tbody>
        {sorted.map((r) => {
          const p = r.total ? Math.round((r.value / r.total) * 100) : 0;
          return (
            <tr key={r.label}>
              <th scope="row">{r.label}</th>
              <td className="bars-track"><span className="bars-rail"><span className="bars-fill" style={{ width: `${p}%` }} /></span></td>
              <td className="bars-num">{p}% <span className="muted">· {r.value} of {r.total} {unit}</span></td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
