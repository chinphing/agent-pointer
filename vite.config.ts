import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import legacy from '@vitejs/plugin-legacy'
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
  'cose-bilkent',
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
const disabledMermaidOutputRe = new RegExp(
  `^(?:${UNSUPPORTED_MERMAID_CHUNKS.join('|')})[^/]*\\.js$`
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
    },
    /**
     * Belt and braces: some diagrams (and the cose-bilkent layout) are reached
     * through shared mermaid chunks, so also drop the emitted files.
     */
    generateBundle(_options: unknown, bundle: Record<string, unknown>) {
      for (const fileName of Object.keys(bundle)) {
        const base = fileName.slice(fileName.lastIndexOf('/') + 1)
        if (disabledMermaidOutputRe.test(base)) delete bundle[fileName]
      }
    }
  }
}

/**
 * 纯 Web 构建的控制面地址：和桌面端共用同一个 `POINTER_API_BASE`。
 *
 * 只用 `define` 内联这一个值。**不要**改成 `envPrefix: ['POINTER_']`——
 * 那会把 `pointer.local.env` 里所有 `POINTER_*`（可能含密钥）打进公开产物。
 */
const webControlPlaneBase = (process.env.POINTER_API_BASE ?? '')
  .trim()
  .replace(/\/+$/, '')

export default defineConfig({
  plugins: [
    dropUnsupportedMermaidDiagrams(),
    vue(),
    // 医院环境还在用 Chrome 63：语法由 esbuild 降到 chrome63（见 build.target），
    // 这里只负责补 Chrome 63 缺失的运行时 API（core-js + web 平台补丁）。
    // Chrome 63 已支持 ESM，不需要 SystemJS 老包，所以关掉 legacy chunk。
    legacy({
      targets: ['chrome >= 63'],
      // 现代包也按 chrome63 降级：Chrome 63 支持 ESM，会走现代包，
      // 所以真正决定「能不能解析」的是 modernTargets，不是 targets。
      modernTargets: ['chrome >= 63'],
      renderLegacyChunks: false,
      modernPolyfills: true
    })
  ],
  define: {
    __POINTER_API_BASE__: JSON.stringify(webControlPlaneBase),
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, 'src'),
      // ELK 布局不打包（mermaid 12 硬依赖 elkjs，1.4 MB）。渲染前已改写成 dagre，
      // 这里换成本地桩，elk 的代码不会进产物。见 `src/lib/elkStub.ts`。
      'elkjs/lib/elk.bundled.js': path.resolve(__dirname, 'src/lib/elkStub.ts')
    }
  },
  test: {
    environment: 'node',
    // Pins host capabilities (logical core count) so the same test cannot pass on
    // one machine and fail on another. See `src/test/setup.ts`.
    setupFiles: ['src/test/setup.ts'],
    // `scripts/**` covers the build-script guardrails (plain .mjs, no DOM).
    include: ['src/**/*.test.ts', 'scripts/**/*.test.mjs', 'admin/web/src/**/*.test.ts']
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
