# Vite frontend chunking

English | [简体中文](../../zh-CN/developer/vite-chunking.md)

Production builds use `manualChunks` to split heavy dependencies out of the main entry, and pair them with async components / dynamic imports to defer loading.

## Vendor chunks (`vite.config.ts`)

| Chunk | Package |
|-------|-----|
| `vue-vendor` | `vue` / `@vue/*` / `pinia` |
| `tauri-vendor` | `@tauri-apps/*` |
| `lucide-vendor` | `lucide-vue-next` |
| `marked-vendor` | `marked` |
| `chart-vendor` | `chart.js` (loaded only when a chart is actually mounted) |
| `virtual-vendor` | `@tanstack/*` |

There is also route-level lazy loading: `MessageList`, `SettingsDialog`, `WorkspacePanel`, the `xterm` terminal, etc.

## On-demand loading conventions

- **Chart.js**: `import('chart.js/auto')` inside `useMarkdownCharts`; the first screen does not carry the chart runtime.
- **WorkspacePanel**: `defineAsyncComponent` in `AppShell`; downloaded only when the workspace is opened (including Diff / Markdown preview).

`chunkSizeWarningLimit` is 700 (watch gzip; the raw threshold is only a hint).
