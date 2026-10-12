import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// In development the API runs separately (`cargo run -p social`); proxying keeps it
// same-origin so the session cookie (SameSite=Strict) works without CORS.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: { '/api': process.env.API_URL ?? 'http://127.0.0.1:3000' },
  },
});
