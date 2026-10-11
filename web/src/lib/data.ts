import { useEffect, useState } from 'react';
import { api } from './api';

export interface Member { id: string; user_id: string; email: string; display_name: string; status: string; title: string | null; roles: string[]; joined_at: string }
export interface Artifact { id: string; kind: string; key: string; title: string; state: string; criticality: string; owner_membership_id: string | null; current_published_version_id: string | null; created_at: string; revision: number }

/** Loads `path` (JSON `{items}`) once per `deps`; returns null while loading, [] on error. */
export function useItems<T>(path: string | null, deps: unknown[] = []): [T[] | null, () => void] {
  const [items, setItems] = useState<T[] | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!path) { setItems([]); return; }
    let live = true;
    api<{ items: T[] }>(path).then((r) => live && setItems(r.items)).catch(() => live && setItems([]));
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, tick, ...deps]);
  return [items, () => setTick((t) => t + 1)];
}

export function memberMap(members: Member[] | null): Map<string, Member> {
  return new Map((members ?? []).map((m) => [m.id, m]));
}
