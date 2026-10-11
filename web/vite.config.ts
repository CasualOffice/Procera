import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// The app talks to the API same-origin (the refresh cookie is SameSite=Strict, HttpOnly): /api and
// /sysapi are proxied to the procera server in dev and preview. PROCERA_API_URL overrides the target.
const api = process.env.PROCERA_API_URL ?? 'http://127.0.0.1:8080';
const proxy = { '/api': { target: api, changeOrigin: false }, '/sysapi': { target: api, changeOrigin: false } };

export default defineConfig({
  plugins: [react()],
  server: { port: 5173, strictPort: true, proxy },
  preview: { proxy },
  build: { sourcemap: true },
});
