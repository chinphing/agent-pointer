import { defineConfig } from 'vite'
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
  clearScreen: false,
  server: {
    host: host || '0.0.0.0',
    port: 1420,
    strictPort: true,
    allowedHosts: true,
    hmr: host
      ? { protocol: 'ws', host, port: 1421 }
      : undefined,
    watch: { ignored: ['**/src-tauri/**'] },
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
