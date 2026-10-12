import { CalendarOff } from 'lucide-react';
import { api } from '../lib/api';
import { statusChanged, useMyStatus } from '../lib/status';
import { Button } from '../ui/components/Button';
import { dayText } from './outOfOffice';

/** Home reminder while you're out of office (BPMS-086): who covers, and a one-press way back. */
export function AwayBanner() {
  const { ooo, canSet } = useMyStatus();
  if (!ooo?.active) return null;
  const end = async () => { try { await api('/api/v1/me/out-of-office', { method: 'DELETE' }); } finally { statusChanged(); } };
  return (
    <div className="away-banner" role="status">
      <CalendarOff aria-hidden="true" size="1rem" />
      <p>You’re out of office until <strong>{dayText(ooo.ends_on)}</strong>. <strong>{ooo.delegate.name}</strong> is deciding your requests.</p>
      {canSet && <Button variant="secondary" className="btn-sm" onPress={() => void end()}>I’m back</Button>}
    </div>
  );
}
