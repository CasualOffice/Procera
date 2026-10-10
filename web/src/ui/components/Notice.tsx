import { CircleAlert, CircleCheck, Info, TriangleAlert } from 'lucide-react';
import './components.css';

type Tone = 'info' | 'success' | 'warning' | 'danger';
const ICON = { info: Info, success: CircleCheck, warning: TriangleAlert, danger: CircleAlert } as const;
const LABEL = { info: 'Information', success: 'Success', warning: 'Warning', danger: 'Error' } as const;

/** Status message: colour + icon + text, never colour alone (WCAG 1.4.1). Errors announce assertively. */
export function Notice({ tone = 'info', title, children }: { tone?: Tone; title: string; children?: React.ReactNode }) {
  const Icon = ICON[tone];
  return (
    <div className={`notice notice-${tone}`} role={tone === 'danger' ? 'alert' : 'status'}>
      <Icon aria-hidden="true" size={20} className="notice-icon" />
      <div>
        <p className="notice-title"><span className="visually-hidden">{LABEL[tone]}: </span>{title}</p>
        {children && <div className="notice-body">{children}</div>}
      </div>
    </div>
  );
}
