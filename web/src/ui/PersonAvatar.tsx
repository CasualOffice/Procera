import { useId } from 'react';
import type { Member } from '../lib/data';
import { dayText } from '../pages/outOfOffice';
import { Avatar } from './Avatar';
import './shell.css';

/**
 * A person's avatar with their availability (BPMS-086): an orange dot while out of office, and a
 * card on hover or keyboard focus with their title and, when away, until when and who covers.
 */
export function PersonAvatar({ member, size = '1.75rem', card = true }: { member: Pick<Member, 'display_name' | 'title' | 'email' | 'away'>; size?: string; card?: boolean }) {
  const id = useId();
  const away = member.away;
  const avatar = (
    <span className="avatar-status">
      <Avatar name={member.display_name} size={size} />
      {away && <span className="status-dot is-away" aria-hidden="true" />}
    </span>
  );
  if (!card) return avatar;
  return (
    <span className="person-hover" tabIndex={0} aria-describedby={id}>
      {avatar}
      <span id={id} role="tooltip" className="person-card">
        <strong className="person-card-name">{member.display_name}</strong>
        {member.title && <span className="person-card-sub">{member.title}</span>}
        {away
          ? <span className="person-card-away"><span className="status-dot is-away" aria-hidden="true" /> Out of office until {dayText(away.until)} · {away.delegate} covers</span>
          : <span className="person-card-on"><span className="status-dot is-on" aria-hidden="true" /> Available</span>}
      </span>
    </span>
  );
}
