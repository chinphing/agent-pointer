import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import path from 'node:path'

const host = process.env.TAURI_DEV_HOST

/** `tauri dev` sets these when it launches Vite via beforeDevCommand. */
const isTauriCli =
  process.env.TAURI_ENV_PLATFORM != null || process.env.TAURI_PLATFORM != null

/**
 * Same-origin `/api` → pointer-server.
 * Default on for web:dev (CORS is off by default on pointer-server).
 * Off during tauri:dev — desktop uses IPC; a leftover browser tab hitting
 * /api would otherwise spam ECONNREFUSED when pointer-server is down.
 */
const webApiBaseEnv = process.env.VITE_WEB_API_BASE
const webApiProxyTarget =
  process.env.VITE_DEV_API_PROXY ||
  (webApiBaseEnv === '' || (webApiBaseEnv === undefined && !isTauriCli)
    ? 'http://127.0.0.1:8787'
    : undefined)

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src')
    }
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts']
  },
  clearScreen: false,
  build: {
    // Local Tauri + same-origin server UI; gzip matters more than raw threshold.
    chunkSizeWarningLimit: 700,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return
          if (/[/\\]node_modules[/\\](?:vue|@vue|pinia)[/\\]/.test(id)) {
            return 'vue-vendor'
          }
          if (id.includes(`${path.sep}@tauri-apps${path.sep}`) || id.includes('/@tauri-apps/')) {
            return 'tauri-vendor'
          }
          if (id.includes(`${path.sep}lucide-vue-next${path.sep}`) || id.includes('/lucide-vue-next/')) {
            return 'lucide-vendor'
          }
          if (id.includes(`${path.sep}marked${path.sep}`) || id.includes('/marked/')) {
            return 'marked-vendor'
          }
          if (id.includes(`${path.sep}chart.js${path.sep}`) || id.includes('/chart.js/')) {
            return 'chart-vendor'
          }
          if (id.includes(`${path.sep}@tanstack${path.sep}`) || id.includes('/@tanstack/')) {
            return 'virtual-vendor'
          }
        }
      }
    }
  },
  server: {
    host: host || '0.0.0.0',
    port: 1420,
    strictPort: true,
    allowedHosts: true,
    hmr: host
      ? { protocol: 'ws', host, port: 1421 }
      : undefined,
    watch: { ignored: ['**/src-tauri/**', '**/target/**'] },
    // Same-origin /api proxy for web:dev; skipped under tauri CLI (see above).
    ...(webApiProxyTarget
      ? {
          proxy: {
            '/api': {
              target: webApiProxyTarget,
              changeOrigin: true
            }
          }
        }
      : {}),
    /** Pre-transform entry + shell so first browser request returns faster in dev. */
    warmup: {
      clientFiles: [
        './src/main.ts',
        './src/styles/globals.css',
        './src/App.vue',
        './src/components/layout/AppShell.vue',
        './src/components/chat/ChatView.vue',
        './src/components/chat/Composer.vue'
      ]
    }
  }
})
