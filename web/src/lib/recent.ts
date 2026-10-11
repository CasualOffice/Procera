/** Recently opened items for the command palette, per workspace, kept in this browser only. */
export interface RecentItem { kind: string; id: string; title: string; to: string }

const KEY = (ws: string) => `procera.recent.${ws}`;
const MAX = 6;

export function recentItems(ws: string | undefined): RecentItem[] {
  if (!ws) return [];
  try {
    const v = JSON.parse(localStorage.getItem(KEY(ws)) ?? '[]');
    return Array.isArray(v) ? (v as RecentItem[]).slice(0, MAX) : [];
  } catch {
    return [];
  }
}

export function rememberRecent(ws: string | undefined, item: RecentItem) {
  if (!ws) return;
  try {
    const next = [item, ...recentItems(ws).filter((r) => r.to !== item.to)].slice(0, MAX);
    localStorage.setItem(KEY(ws), JSON.stringify(next));
  } catch {
    /* storage unavailable: no recents */
  }
}
