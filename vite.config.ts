import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import path from 'node:path'
import { applyPointerLocalEnvToProcess } from './scripts/lib/load-pointer-local-env.mjs'

// Per-machine control-plane defaults (gitignored pointer.local.env).
applyPointerLocalEnvToProcess({ logPrefix: '[vite]' })

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

/**
 * Mermaid ships ~40 diagram types; this build renders six of them (see
 * `src/lib/markdownMermaid.ts`). Stub the rest so Rollup does not emit their
 * chunks — the render-time gate lives in the app code, this only trims size.
 */
const UNSUPPORTED_MERMAID_CHUNKS = [
  'abnf',
  'architecture',
  'block',
  'c4',
  'cynefin',
  'ebnf',
  'eventmodeling',
  'gitGraph',
  'info',
  'ishikawa',
  'journey',
  'kanban',
  'mindmap',
  'packet',
  'peg',
  'pie',
  'quadrant',
  'radar',
  'railroad',
  'requirement',
  'sankey',
  'swimlanes',
  'timeline',
  'treeView',
  'treemap',
  'venn',
  'wardley',
  'xychart'
]

const DISABLED_MERMAID_MODULE = '\0pointer:disabled-mermaid-diagram'
const disabledMermaidChunkRe = new RegExp(
  `chunks/mermaid\\.core/(?:${UNSUPPORTED_MERMAID_CHUNKS.join('|')})[^/]*$`
)

function dropUnsupportedMermaidDiagrams() {
  return {
    name: 'pointer:drop-unsupported-mermaid-diagrams',
    enforce: 'pre' as const,
    resolveId(source: string) {
      return disabledMermaidChunkRe.test(source) ? DISABLED_MERMAID_MODULE : null
    },
    load(id: string) {
      return id === DISABLED_MERMAID_MODULE ? 'export {}' : null
    }
  }
}

export default defineConfig({
  plugins: [dropUnsupportedMermaidDiagrams(), vue()],
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
