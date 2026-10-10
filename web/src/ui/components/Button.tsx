import { Button as AriaButton, type ButtonProps as AriaButtonProps } from 'react-aria-components';
import './components.css';

/**
 * Buttons (docs/05 §7.1.1).
 * - `primary`: ink/charcoal — the default primary action (never brand red: an approvals product
 *   must not make "Approve" look destructive).
 * - `secondary`: outlined neutral. Use for Reject in approvals.
 * - `danger`: distinct crimson; always pair with an icon and an explicit verb, and confirm destructive actions.
 * - `accent`: optional brand-strong style (Deep Red), only when the workspace enables it.
 */
export type ButtonVariant = 'primary' | 'secondary' | 'danger' | 'accent' | 'ghost';

export interface ButtonProps extends AriaButtonProps {
  variant?: ButtonVariant;
  size?: 'md' | 'lg';
}

export function Button({ variant = 'primary', size = 'md', className, ...rest }: ButtonProps) {
  return <AriaButton {...rest} className={`btn btn-${variant} btn-${size} ${typeof className === 'string' ? className : ''}`} />;
}
