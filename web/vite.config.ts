/// <reference types="vitest/config" />
import { fileURLToPath, URL } from 'node:url'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'
import type { ProxyOptions } from 'vite'

const BACKEND = process.env.SERVICEHUB_BACKEND ?? 'http://127.0.0.1:8080'

/**
 * Dev proxy scheme (production serves everything from one origin, so this only matters for `npm run dev`):
 * - `/api/**`                → backend (JSON API).
 * - `/mock/<svc>/api/**`     → backend (JSON APIs of the mock services, e.g. DemoPay).
 * - any non-GET `/mock/**`   → backend (form posts such as DemoPay "Pay now", `POST /mock/mail/send`).
 * - GET `/mock/**` pages     → backend too, EXCEPT `/mock/mail`, which is a SPA page in this app
 *                              (DemoMail reads `/api/demo/mailbox`). Server-rendered mock consoles
 *                              (e.g. `/mock/pay/checkout/…`) keep working in dev.
 */
const mockProxy: ProxyOptions = {
  target: BACKEND,
  changeOrigin: false,
  bypass(req) {
    const path = (req.url ?? '').split('?')[0] ?? ''
    const isSpaPage = req.method === 'GET' && (path === '/mock/mail' || path === '/mock/mail/')
    // Returning a path tells Vite to serve the SPA instead of proxying.
    return isSpaPage ? '/index.html' : undefined
  },
}

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': fileURLToPath(new URL('./src', import.meta.url)) },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': { target: BACKEND, changeOrigin: false },
      '/mock': mockProxy,
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts', 'src/**/*.test.tsx'],
  },
})
