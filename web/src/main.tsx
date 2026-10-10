import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './ui/base.css';
import './app.css';
import { AppShell } from './ui/AppShell';
import { DesignGallery } from './pages/DesignGallery';

const root = document.getElementById('root');
if (root) {
  createRoot(root).render(
    <StrictMode>
      <AppShell current="Home">
        <DesignGallery />
      </AppShell>
    </StrictMode>,
  );
}
