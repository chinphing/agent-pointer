# Vite 前端分包

生产构建用 `manualChunks` 把重型依赖拆出主入口，并配合异步组件 / 动态 import 推迟加载。

## Vendor chunks（`vite.config.ts`）

| Chunk | 包 |
|-------|-----|
| `vue-vendor` | `vue` / `@vue/*` / `pinia` |
| `tauri-vendor` | `@tauri-apps/*` |
| `lucide-vendor` | `lucide-vue-next` |
| `marked-vendor` | `marked` |
| `chart-vendor` | `chart.js`（仅在真正挂载图表时加载） |
| `virtual-vendor` | `@tanstack/*` |

另有路由级懒加载：`MessageList`、`SettingsDialog`、`WorkspacePanel`、`xterm` 终端等。

## 按需加载约定

- **Chart.js**：`useMarkdownCharts` 内 `import('chart.js/auto')`，首屏不带图表运行时。
- **WorkspacePanel**：`AppShell` 里 `defineAsyncComponent`；仅在打开工作区时下载（含 Diff / Markdown 预览）。

`chunkSizeWarningLimit` 为 700（关注 gzip；原始阈值仅作提示）。
