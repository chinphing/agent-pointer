import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import path from 'node:path'

const host = process.env.TAURI_DEV_HOST

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
    // Local Tauri + same-origin server UI; gzip matters more than raw 500kB threshold.
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
