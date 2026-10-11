import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './ui/base.css';
import './app.css';
import { App } from './App';
import { SessionProvider } from './lib/session';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <SessionProvider>
        <App />
      </SessionProvider>
    </StrictMode>,
  );
}
