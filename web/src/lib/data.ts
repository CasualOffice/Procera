import { useEffect, useState } from 'react';
import { ApiError, api } from './api';

export interface Member { id: string; user_id: string; email: string; display_name: string; status: string; title: string | null; roles: string[]; joined_at: string }
export interface Artifact { id: string; kind: string; key: string; title: string; state: string; criticality: string; owner_membership_id: string | null; current_published_version_id: string | null; created_at: string; revision: number }

/** A load failure the page can show and retry (docs/05 §3: error state with retry). */
export interface LoadFailure { message: string; code?: string; status?: number }

export function toFailure(e: unknown): LoadFailure {
  if (e instanceof ApiError) return { message: e.problem.detail ?? e.problem.title, code: e.code, status: e.status };
  return { message: 'The server could not be reached. Check your connection.' };
}

/**
 * Loads `path` (JSON `{items}`) once per `deps`. Returns null while loading; on failure the items
 * are [] and the third value describes the error so the page can show it instead of an empty state.
 */
export function useItems<T>(path: string | null, deps: unknown[] = []): [T[] | null, () => void, LoadFailure | null] {
  const [items, setItems] = useState<T[] | null>(null);
  const [error, setError] = useState<LoadFailure | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!path) { setItems([]); setError(null); return; }
    let live = true;
    api<{ items: T[] }>(path)
      .then((r) => { if (live) { setItems(r.items); setError(null); } })
      .catch((e: unknown) => { if (live) { setItems([]); setError(toFailure(e)); } });
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, tick, ...deps]);
  return [items, () => setTick((t) => t + 1), error];
}

/** Loads one JSON object; null while loading. */
export function useResource<T>(path: string | null, deps: unknown[] = []): [T | null, () => void, LoadFailure | null] {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<LoadFailure | null>(null);
  const [tick, setTick] = useState(0);
  useEffect(() => {
    if (!path) return;
    let live = true;
    api<T>(path)
      .then((r) => { if (live) { setData(r); setError(null); } })
      .catch((e: unknown) => { if (live) setError(toFailure(e)); });
    return () => { live = false; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path, tick, ...deps]);
  return [data, () => setTick((t) => t + 1), error];
}

export function memberMap(members: Member[] | null): Map<string, Member> {
  return new Map((members ?? []).map((m) => [m.id, m]));
}
