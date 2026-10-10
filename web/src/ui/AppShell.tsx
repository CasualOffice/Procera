import { useState } from 'react';
import './shell.css';

const NAV = ['Home', 'Library', 'Requests', 'My Work', 'Announcements', 'Reports', 'Admin'] as const;

/** Workspace app shell (docs/05 §2.1): skip link, landmark nav with an accessible current-page marker. */
export function AppShell({ children, current = 'Home' }: { children: React.ReactNode; current?: (typeof NAV)[number] }) {
  const [theme, setTheme] = useState<'auto' | 'light' | 'dark'>('auto');
  const cycle = () => {
    const next = theme === 'auto' ? 'light' : theme === 'light' ? 'dark' : 'auto';
    setTheme(next);
    if (next === 'auto') document.documentElement.removeAttribute('data-theme');
    else document.documentElement.setAttribute('data-theme', next);
  };
  return (
    <>
      <a className="skip-link" href="#main">Skip to main content</a>
      <header className="shell-header">
        <a className="shell-brand" href="/">
          <img src="/procera-symbol.png" alt="" width={24} height={24} />
          <span>Procera</span>
        </a>
        <button type="button" className="shell-theme" onClick={cycle} aria-label={`Theme: ${theme}. Change theme`}>
          Theme: {theme}
        </button>
      </header>
      <div className="shell-body">
        <nav className="shell-nav" aria-label="Main">
          <ul>
            {NAV.map((item) => (
              <li key={item}>
                <a href="#" aria-current={item === current ? 'page' : undefined}>{item}</a>
              </li>
            ))}
          </ul>
        </nav>
        <main id="main" className="shell-main" tabIndex={-1}>{children}</main>
      </div>
    </>
  );
}
