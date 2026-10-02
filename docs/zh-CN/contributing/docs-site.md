# 文档站规则

[English](../../en/contributing/docs-site.md) | 简体中文

`docs/` 就是站点源码，`docs-site/` 是 VitePress 工程。**发布规则只有一处事实源**：`docs-site/site-map.mjs`。

## 一句话规则

**黑名单机制**：`docs/**/*.md` 里的页面，**只要不在 `SITE_EXCLUDES` 里就自动发布**。新增文档不需要「注册」就能上站 —— 但要自己补**侧边栏顺序**和**索引页**（见「新增一篇文档」）。

## 三层分区

| 层 | 目录 | 上站 | 侧边栏位置 |
|----|------|------|-----------|
| tier 1 | `user/` `developer/` `contributing/` `deploy/` | ✅ | 各自一个一级分区 |
| tier 2 | `internals/` `agents/` `llm/` `ui/` | ✅ | 合并进「维护者笔记」分区，各成一个折叠组，并自动加**维护者笔记横幅** |
| tier 3 | `design/` `plans/` `settings-refactor/` `superpowers/` | ❌ | 不上站 |

> 「tier」只是 `site-map.mjs` 注释里的约定，**代码里没有 tier 常量**。真正生效的是下面的 `SITE_EXCLUDES`。

## 不上站清单

```js
// docs-site/site-map.mjs
export const SITE_EXCLUDES = [
  'README.md', // docs/ language picker — collides with the zh-CN index route
  'zh-CN/design',
  'zh-CN/plans',
  'zh-CN/settings-refactor',
  'zh-CN/superpowers',
  'en/design',
];
```

- **匹配规则**：精确相等，或以该前缀开头的目录（`p === ex || p.startsWith(ex + '/')`）—— 所以写目录名就排除整个目录
- `SRC_EXCLUDE_GLOBS` 是它的 **VitePress 镜像**（喂给 `srcExclude`），改一边必须同步改另一边
- 指向不上站页面的相对链接，会在构建期被改写成 **GitHub blob 链接**（`doc-links.mts`），不会变成死链

## 语言与路由

| 事实 | 值 |
|------|-----|
| **默认语言** | **英文** —— `docs/en/**` 的 `en/` 前缀被剥掉，路由落在根（`docs/en/user/skills.md` → `/user/skills`） |
| 中文 | 保留 `zh-CN/` 前缀（→ `/zh-CN/user/skills`） |
| 目录索引 | 每个目录的 `README.md` → 该目录的 index 路由（`/zh-CN/user/`） |
| URL 形态 | `cleanUrls: true` → URL **不带 `.html`**；产物仍是 `foo.html` / `index.html` |
| base | `/agent-pointer/`（GitHub Pages project site，由 `GITHUB_REPO` 推出） |

## 侧边栏怎么来的

`sidebar.mts` 从**已发布集合**反向生成侧边栏：

- 新文档**自动出现**在侧栏，不用改配置
- 不上站的页面**永远不会**进侧栏，也不会变成死链
- 标题取文件**第一个 H1**（跳过代码块），没有 H1 时回落文件名

分区（section）在 `config.mts` 里声明。中文 6 个：快速开始 / 使用指南 / **开发** / 部署 / 参与贡献 / 维护者笔记；英文同样是 6 个（Getting Started / Guides / Development / Deployment / Contributing / Internals），只是内容更少。

一个分区可以用三种方式组装：

| 字段 | 含义 |
|------|------|
| `files` | 手挑的页面列表，**按给定顺序**（如「开发 → 概念」组列出 6 篇 developer 页） |
| `dirs` | 二级分组。可以是整目录列举（`dir` + `exclude`，exclude 写 `docs/` 相对路径，因此能从**别的目录**里挑走页面）、手挑列表（只给 `files`、省略 `dir`），或两者并用；带 `label` 才渲染成折叠分组 |
| `items` | 手写条目（极少用） |

**顺序规则**（`sidebar.mts` 的 `PRIORITY`）：列在数组里的按数组下标排，没列出的排在后面、按文件名 `localeCompare` 排。

### 归位改配置，不改路径

页面物理位置和侧栏归属**可以不一致**。例：`contributing/macos-window-chrome.md` 与 `contributing/web-media-and-desktop-snapshot.md` 讲的是 UI 实现，所以侧栏把它们归到「维护者笔记 → UI 笔记」：

```ts
// config.mts
{ text: '参与贡献', dirs: [{ dir: 'zh-CN/contributing', exclude: [ /* 两篇 UI 文档 */ ] }] },
{ text: '维护者笔记', dirs: [ /* … */, { dir: 'zh-CN/ui', label: 'UI 笔记', files: [ /* 两篇 UI 文档 */ ] }] },
```

> **为什么不动文件**：移动会改 URL、打断所有入链（这两篇有 6 处入链，其中一处是 `Composer.vue` 里的源码注释），收益只是目录好看。所以约定是 **归位改配置，不改路径**。

⚠️ 一个坑：**维护者笔记横幅是按路径注入的**（`config.mts` 里的正则匹配 `internals|agents|llm|ui`）。把页面「归位」进维护者笔记分区时，记得同时把它的路径加进那个正则，否则会出现「在维护者分区里但没有横幅」。

### 例：「开发」分区的四组

`zh-CN/developer/` 的 35 篇按主题分成 4 组 + 一个兜底组，篇目写在 `config.mts` 的 `ZH_DEV_*` 常量里：

| 组 | 篇数 | 内容 |
|----|:---:|------|
| **概念** | 6 | 架构、工作区根、附件存储、扩展钩子、Skills 兼容与持久化 |
| **工具与协议** | 10 | 工具调用协议、`run_subagent`、file / web / terminal / session 工具、logging |
| **集成** | 7 | IM 通道、飞书与 Lark CLI、云主机、桌面 OAuth、MCP、Webhook |
| **排障** | 8 | 流式错误与重连、认证刷新、文本截断、chunking、session id、轮次基线、消息位置碰撞 |
| **其他** | 兜底 | `cli.md`、`standalone-local-login.md`，以及**未列入上面四组的新页** |

兜底组用 `dir` + `exclude` 生成 —— 新加一篇 developer 文档时，即使忘了归组也会出现在「其他」里，**不会掉出侧栏**。`standalone-deployment.md` 是例外：它挂在「部署」分区。

分区第一项是 [`developer/README.md`](../developer/README.md)（索引页，用 `files` 显式指定）。

## 新增一篇文档

1. 写 `docs/zh-CN/<分区>/<名字>.md` —— **不在 `SITE_EXCLUDES` 里就自动发布**
2. 要控制侧栏位置 → 加进 `sidebar.mts` 的 `PRIORITY['zh-CN/<分区>']`
3. 加进该目录 `README.md` 的索引表
4. `node scripts/check-doc-links.mjs` → 0 断链
5. `npm run docs:build` 本地构建一次
6. 需要看效果 → 预览（见下）

## 本地预览与构建

```bash
npm run docs:install   # 首次：npm --prefix docs-site install
npm run docs:dev       # 开发服务器（默认 5173，热更新）
npm run docs:build     # 构建 → docs-site/.vitepress/dist
npm run docs:preview   # 预览构建产物（默认 4173）
```

### ⚠️ preview 不要传路径参数

```bash
cd docs-site && npx vitepress preview docs-site   # ❌ ENOENT
cd docs-site && npx vitepress preview             # ✅
```

报错原文：

```
failed to start server. error:
ENOENT: no such file or directory, open '…/docs-site/docs-site/.vitepress/dist/404.html'
```

原因：`vitepress preview` 的**位置参数会被当成 VitePress root**，产物目录按 `<root>/.vitepress/dist` 算 —— 多传一个 `docs-site` 就多套一层，于是找不到 `404.html`。

- 在 `docs-site/` 里跑 → **不要**给路径
- 在仓库根跑 → 用 `npm run docs:preview`（脚本会切到 `docs-site/`）

## 链接校验

```bash
node scripts/check-doc-links.mjs
```

- **零依赖**：CI 里不装任何包，直接 `node` 跑
- 扫描**全部** Markdown（**包含不上站的 tier 3**），检查相对链接与锚点，跳过代码块与行内代码
- 用 `site-map.mjs` 的已发布集合解析 `/route` 形式的链接 —— 指向不上站页面的路由链接**算断链**
- 有断链退出码 `1`，用法错误 `2`

CI 里由 `docs-links.yml` 跑，构建与发布由 `docs-site.yml` 跑，见 [CI 与发布流程](ci.md)。

## 相关

- [CI 与发布流程](ci.md) —— `docs-links.yml` / `docs-site.yml` 的触发与命令
- [跨平台开发与打包](cross-platform-build.md) —— 平台环境准备
- [命令行与脚本参考](../developer/cli.md) —— `docs:*` 脚本逐个说明
- 站点工程说明：[`docs-site/README.md`](../../../docs-site/README.md)
