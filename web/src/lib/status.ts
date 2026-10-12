import { useCallback, useEffect, useState } from 'react';
import { api } from './api';
import type { OutOfOffice } from '../pages/outOfOffice';

/** Fired after out of office is set or ended anywhere, so every status display refreshes. */
export const statusChanged = () => window.dispatchEvent(new Event('procera:status-changed'));

/**
 * The signed-in person's availability (BPMS-086): out of office (with dates and delegate) or available.
 * One small read shared by the account button, the account menu and Home.
 */
export function useMyStatus(enabled = true): { ooo: OutOfOffice | null; canSet: boolean; loaded: boolean; reload: () => void } {
  const [state, setState] = useState<{ ooo: OutOfOffice | null; canSet: boolean; loaded: boolean }>({ ooo: null, canSet: false, loaded: false });
  const load = useCallback(() => {
    if (!enabled) return;
    api<{ out_of_office: OutOfOffice | null; can_set: boolean }>('/api/v1/me/out-of-office')
      .then((r) => setState({ ooo: r.out_of_office, canSet: r.can_set, loaded: true }))
      .catch(() => setState((s) => ({ ...s, loaded: true })));
  }, [enabled]);
  useEffect(() => {
    load();
    window.addEventListener('procera:status-changed', load);
    return () => window.removeEventListener('procera:status-changed', load);
  }, [load]);
  return { ...state, reload: load };
}
