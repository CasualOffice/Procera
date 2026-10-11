import { useSyncExternalStore } from 'react';

/** A minimal history router: `useRoute()` re-renders on navigation; `navigate()` pushes a path. */
const listeners = new Set<() => void>();
function subscribe(fn: () => void) {
  listeners.add(fn);
  window.addEventListener('popstate', fn);
  return () => {
    listeners.delete(fn);
    window.removeEventListener('popstate', fn);
  };
}

export function navigate(to: string, { replace = false } = {}) {
  if (replace) window.history.replaceState(null, '', to);
  else window.history.pushState(null, '', to);
  listeners.forEach((l) => l());
  // Move focus to the page so screen readers announce the change (WCAG 2.4.3).
  queueMicrotask(() => document.getElementById('main')?.focus());
}

export function useRoute(): { path: string; query: URLSearchParams } {
  const href = useSyncExternalStore(subscribe, () => window.location.pathname + window.location.search);
  const url = new URL(href, window.location.origin);
  return { path: url.pathname, query: url.searchParams };
}

/** Matches `/library/:id` style patterns; returns params or null. */
export function match(pattern: string, path: string): Record<string, string> | null {
  const p = pattern.split('/').filter(Boolean);
  const s = path.split('/').filter(Boolean);
  if (p.length !== s.length) return null;
  const out: Record<string, string> = {};
  for (let i = 0; i < p.length; i++) {
    const want = p[i] ?? '';
    const got = s[i] ?? '';
    if (want.startsWith(':')) out[want.slice(1)] = decodeURIComponent(got);
    else if (want !== got) return null;
  }
  return out;
}

/** An in-app link: plain <a> for accessibility, client-side navigation on click. */
export function Link({ to, children, ...rest }: { to: string; children: React.ReactNode } & Omit<React.AnchorHTMLAttributes<HTMLAnchorElement>, 'href'>) {
  return (
    <a
      {...rest}
      href={to}
      onClick={(e) => {
        if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
        e.preventDefault();
        navigate(to);
      }}
    >
      {children}
    </a>
  );
}
