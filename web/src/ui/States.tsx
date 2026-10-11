import { RotateCw, type LucideIcon } from 'lucide-react';
import type { LoadFailure } from '../lib/data';
import { Button } from './components/Button';
import { Art, type ArtKind } from './illustrations';

/**
 * The state kit (docs/05 §3, UX-005): skeletons shaped like the content, a composed empty state
 * with one next step, and an error with what happened plus a retry. Never a bare spinner.
 */

/** Row-shaped placeholders. `variant="table"` draws a header and cell bars. */
export function Skeleton({ rows = 3, variant = 'list', label = 'Loading' }: { rows?: number; variant?: 'list' | 'table' | 'doc' | 'tiles' | 'block'; label?: string }) {
  if (variant === 'tiles') {
    return (
      <div className="sk-tiles" role="status" aria-label={label}>
        {Array.from({ length: rows }, (_, i) => (
          <div key={i} className="sk-tile"><span className="sk sk-sub" style={{ width: '46%' }} /><span className="sk sk-figure" /><span className="sk" style={{ width: `${52 + ((i * 11) % 30)}%` }} /></div>
        ))}
      </div>
    );
  }
  if (variant === 'block') return <div className="sk sk-block" role="status" aria-label={label} />;
  if (variant === 'doc') {
    return (
      <div className="sk-doc" role="status" aria-label={label}>
        <span className="sk sk-line" style={{ width: '18%' }} />
        <span className="sk sk-title" />
        {[92, 100, 84, 96, 60].map((w, i) => <span key={i} className="sk sk-line" style={{ width: `${w}%` }} />)}
      </div>
    );
  }
  if (variant === 'table') {
    return (
      <div className="sk-table" role="status" aria-label={label}>
        <div className="sk-table-head"><span className="sk" /><span className="sk" /><span className="sk" /></div>
        {Array.from({ length: rows }, (_, i) => (
          <div key={i} className="sk-table-row">
            <span className="sk sk-icon" /><span className="sk" style={{ width: `${38 + ((i * 17) % 30)}%` }} /><span className="sk sk-chip" /><span className="sk sk-short" />
          </div>
        ))}
      </div>
    );
  }
  return (
    <div className="sk-list" role="status" aria-label={label}>
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="sk-list-row">
          <span className="sk sk-icon" />
          <span className="sk-lines"><span className="sk" style={{ width: `${44 + ((i * 13) % 36)}%` }} /><span className="sk sk-sub" style={{ width: `${24 + ((i * 7) % 20)}%` }} /></span>
        </div>
      ))}
    </div>
  );
}

/** The app frame while who-you-are loads: sidebar and a page of placeholders, never a blank screen. */
export function ShellSkeleton() {
  return (
    <div className="app" role="status" aria-label="Loading Procera">
      <aside className="sidebar sk-sidebar">
        <span className="sk sk-icon" />
        {[70, 56, 64, 48, 60, 52].map((w, i) => <span key={i} className="sk" style={{ width: `${w}%` }} />)}
      </aside>
      <div className="sk-main"><Skeleton variant="doc" label="Loading page" /></div>
    </div>
  );
}

/**
 * A composed empty state: an illustration (or icon), one line of what this is, one line of how it
 * fills, one action. `art` picks the drawing for the kind of emptiness; it wins over `icon`.
 */
export function Empty({ icon: Icon, art, title, children, action, compact = false }: { icon?: LucideIcon; art?: ArtKind; title: string; children?: React.ReactNode; action?: React.ReactNode; compact?: boolean }) {
  return (
    <div className={`empty${compact ? ' empty-compact' : ''}${art ? ' empty-art' : ''}`}>
      {art ? <Art kind={art} /> : Icon && <span className="empty-icon"><Icon aria-hidden="true" size="1.125rem" /></span>}
      <p className="empty-title">{title}</p>
      {children && <p className="empty-text">{children}</p>}
      {action && <div className="empty-action">{action}</div>}
    </div>
  );
}

/** A load failure in place of the content, with a retry. Permission errors explain who can help. */
export function LoadError({ error, onRetry, what = 'this page' }: { error: LoadFailure; onRetry?: () => void; what?: string }) {
  const denied = error.status === 403;
  return (
    <div className="load-error" role="alert">
      <p className="empty-title">{denied ? `You don’t have access to ${what}` : `Couldn’t load ${what}`}</p>
      <p className="empty-text">{denied ? 'Ask a workspace admin if you need it.' : error.message}</p>
      {!denied && onRetry && <div className="empty-action"><Button variant="secondary" onPress={onRetry}><RotateCw aria-hidden="true" size="0.875rem" /> Try again</Button></div>}
    </div>
  );
}
