import path from 'node:path';
import type { Plugin } from 'vite';
import { defineConfig } from 'vitepress';
import { docLinks, strayHtml } from './doc-links.mts';
import { buildSidebar, type SectionSpec } from './sidebar.mts';
import {
  GITHUB_OWNER,
  GITHUB_REPO,
  SITE_BASE,
  SITE_DIR,
  SRC_DIR,
  SRC_EXCLUDE_GLOBS,
  alternateLocale,
  buildSiteIndex,
  rewritePage,
} from '../site-map.mjs';

/** Published file path for every page. `en/` is stripped, `README.md` → `index.md`. */
const rewrites = (page: string) => rewritePage(page);

/**
 * Resolve `vue` / `vue/*` from the docs-site workspace.
 *
 * The markdown sources live in `../docs/`, i.e. *outside* this workspace, so
 * Node's upward `node_modules` lookup starts from the importer's own directory.
 * Locally that escapes to the repository root and silently bundles the
 * **application's** Vue (3.5.34 here, not this workspace's 3.5.43); on CI the
 * repository root has no `node_modules` at all — only `docs-site/` is installed
 * — and the build dies with
 * `failed to resolve import "vue/server-renderer"`.
 *
 * Re-resolving through Vite's own resolver keeps the package `exports` map and
 * the active conditions intact, so `vue`, `vue/server-renderer` and
 * `vue/compiler-sfc` land on exactly the files they resolve to for every
 * importer inside `docs-site/`. (`resolve.alias` would bypass `exports` and
 * change which build of `vue/compiler-sfc` the Vue plugin loads;
 * `resolve.dedupe` cannot help either, because VitePress sets Vite's `root` to
 * `docs/`, which is still outside this workspace.)
 */
function vueFromSiteRoot(): Plugin {
  return {
    name: 'docs-site:vue-from-workspace',
    enforce: 'pre',
    resolveId(source, _importer, options) {
      if (source !== 'vue' && !source.startsWith('vue/')) return null;
      return this.resolve(source, path.join(SITE_DIR, 'package.json'), { ...options, skipSelf: true });
    },
  };
}

/** Published page index, built once — powers the language-switch targets below. */
const siteIndex = buildSiteIndex();

/** Tier-2 banner text, per locale; the docs carry no frontmatter of their own. */
const maintainerNotes = {
  root: '**Maintainer notes** — implementation details for core maintainers, published with the repository. Not a user tutorial.',
  'zh-CN': '**维护者笔记 · Maintainer notes** — 面向核心维护者的实现细节，随仓库公开，不是用户教程。',
} as const;

/**
 * Pages listed under 维护者笔记 in the sidebar that physically live elsewhere
 * (`zhSections` below places them by config). The banner is injected by path,
 * so such pages must be named here explicitly.
 */
const MAINTAINER_NOTE_EXTRA = [
  'zh-CN/contributing/macos-window-chrome.md',
  'zh-CN/contributing/web-media-and-desktop-snapshot.md',
  'en/contributing/macos-window-chrome.md',
  'en/contributing/web-media-and-desktop-snapshot.md',
];

/**
 * `zh-CN/developer/` pages, grouped inside the 开发 section (sidebar order).
 *
 * Placement lives here — the files never move, so every URL stays stable.
 * The 其他 group is generated from the directory minus everything listed
 * below, so a new page shows up there instead of dropping out of the sidebar.
 */
const ZH_DEV_CONCEPTS = [
  'zh-CN/developer/architecture.md',
  'zh-CN/developer/workspace-root.md',
  'zh-CN/developer/attachment-storage.md',
  'zh-CN/developer/agent-extension-hooks.md',
  'zh-CN/developer/skills-compatibility.md',
  'zh-CN/developer/skills-persistence.md',
];

const ZH_DEV_TOOLS = [
  'zh-CN/developer/native-tool-calling-protocol.md',
  'zh-CN/developer/pointer-run-subagent.md',
  'zh-CN/developer/file-tool-output-limits.md',
  'zh-CN/developer/file-tool-write-scope.md',
  'zh-CN/developer/web-search-tool.md',
  'zh-CN/developer/web-fetch-tool.md',
  'zh-CN/developer/terminal-environment-variables.md',
  'zh-CN/developer/terminal-interactive-input.md',
  'zh-CN/developer/session-search-output-limits.md',
  'zh-CN/developer/logging.md',
];

const ZH_DEV_INTEGRATION = [
  'zh-CN/developer/channel-integration.md',
  'zh-CN/developer/feishu-cli-integration-sop.md',
  'zh-CN/developer/lark-cli-quickstart.md',
  'zh-CN/developer/cloud-host-integration.md',
  'zh-CN/developer/desktop-oauth-web-integration.md',
  'zh-CN/developer/mcp.md',
  'zh-CN/developer/webhook-api.md',
];

const ZH_DEV_TROUBLESHOOTING = [
  'zh-CN/developer/chat-run-errors.md',
  'zh-CN/developer/chat-stream-resync.md',
  'zh-CN/developer/platform-auth-refresh-errors.md',
  'zh-CN/developer/rust-text-truncation.md',
  'zh-CN/developer/vite-chunking.md',
  'zh-CN/developer/session-user-id.md',
  'zh-CN/developer/turn-file-baseline-review.md',
  'zh-CN/developer/conversation-message-position-collision.md',
];

/** Every page placed by the groups above — the 其他 group lists the rest. */
const ZH_DEV_PLACED = [
  ...ZH_DEV_CONCEPTS,
  ...ZH_DEV_TOOLS,
  ...ZH_DEV_INTEGRATION,
  ...ZH_DEV_TROUBLESHOOTING,
];

/**
 * The same grouping for `en/developer/` (sidebar order).
 *
 * Only pages that already have an English counterpart are listed; the rest
 * still live on the Chinese tree and are linked from the index page. The Other
 * group is generated from the directory minus everything below, so a new
 * English page shows up there instead of dropping out of the sidebar.
 */
const EN_DEV_CONCEPTS = [
  'en/developer/architecture.md',
  'en/developer/workspace-root.md',
  'en/developer/attachment-storage.md',
  'en/developer/agent-extension-hooks.md',
  'en/developer/skills-compatibility.md',
  'en/developer/skills-persistence.md',
];

const EN_DEV_TOOLS = [
  'en/developer/native-tool-calling-protocol.md',
  'en/developer/pointer-run-subagent.md',
  'en/developer/file-tool-output-limits.md',
  'en/developer/file-tool-write-scope.md',
  'en/developer/web-search-tool.md',
  'en/developer/web-fetch-tool.md',
  'en/developer/terminal-environment-variables.md',
  'en/developer/terminal-interactive-input.md',
  'en/developer/session-search-output-limits.md',
  'en/developer/logging.md',
];

const EN_DEV_INTEGRATION = [
  'en/developer/channel-integration.md',
  'en/developer/feishu-cli-integration-sop.md',
  'en/developer/lark-cli-quickstart.md',
  'en/developer/cloud-host-integration.md',
  'en/developer/desktop-oauth-web-integration.md',
  'en/developer/mcp.md',
  'en/developer/webhook-api.md',
];

const EN_DEV_TROUBLESHOOTING = [
  'en/developer/chat-run-errors.md',
  'en/developer/chat-stream-resync.md',
  'en/developer/platform-auth-refresh-errors.md',
  'en/developer/rust-text-truncation.md',
  'en/developer/vite-chunking.md',
  'en/developer/session-user-id.md',
  'en/developer/turn-file-baseline-review.md',
  'en/developer/conversation-message-position-collision.md',
];

/** Every page placed by the groups above — the Other group lists the rest. */
const EN_DEV_PLACED = [
  ...EN_DEV_CONCEPTS,
  ...EN_DEV_TOOLS,
  ...EN_DEV_INTEGRATION,
  ...EN_DEV_TROUBLESHOOTING,
];

/**
 * Sidebar sections, in nav order. Tier 3 (`design/`, `plans/`,
 * `settings-refactor/`, `superpowers/`) stays off the site.
 *
 * A section is either an explicit `files` list (a hand-picked subset of a
 * directory) or a whole `dirs` listing with `exclude`d pages, or both.
 */
const zhSections: SectionSpec[] = [
  {
    text: '快速开始',
    files: ['zh-CN/user/getting-started.md', 'zh-CN/user/which-build.md'],
  },
  {
    text: '使用指南',
    files: ['zh-CN/user/README.md'],
    dirs: [
      {
        dir: 'zh-CN/user',
        exclude: ['zh-CN/user/README.md', 'zh-CN/user/getting-started.md', 'zh-CN/user/which-build.md'],
      },
    ],
  },
  {
    text: '开发',
    // Index page first, then the hand-picked groups. 其他 is generated from the
    // directory minus everything placed above, so a new developer page shows up
    // there instead of dropping out of the sidebar. `standalone-deployment.md`
    // lives under 部署 instead.
    files: ['zh-CN/developer/README.md'],
    dirs: [
      { label: '概念', files: ZH_DEV_CONCEPTS },
      { label: '工具与协议', files: ZH_DEV_TOOLS },
      { label: '集成', files: ZH_DEV_INTEGRATION },
      { label: '排障', files: ZH_DEV_TROUBLESHOOTING },
      {
        dir: 'zh-CN/developer',
        label: '其他',
        exclude: [...ZH_DEV_PLACED, 'zh-CN/developer/README.md', 'zh-CN/developer/standalone-deployment.md'],
      },
    ],
  },
  {
    text: '部署',
    files: ['zh-CN/developer/standalone-deployment.md'],
    dirs: [{ dir: 'zh-CN/deploy' }],
  },
  {
    text: '参与贡献',
    // The two UI-implementation pages are listed under 维护者笔记 → UI 笔记
    // instead. Placement is expressed here; the files never move, because their
    // URLs are linked from ui/visual-theme.md and from source comments.
    dirs: [
      {
        dir: 'zh-CN/contributing',
        exclude: [
          'zh-CN/contributing/macos-window-chrome.md',
          'zh-CN/contributing/web-media-and-desktop-snapshot.md',
        ],
      },
    ],
  },
  {
    text: '维护者笔记',
    dirs: [
      { dir: 'zh-CN/internals', label: '运行时机制' },
      { dir: 'zh-CN/agents', label: '子代理提示词' },
      { dir: 'zh-CN/llm', label: 'LLM 接入' },
      {
        dir: 'zh-CN/ui',
        label: 'UI 笔记',
        files: [
          'zh-CN/contributing/macos-window-chrome.md',
          'zh-CN/contributing/web-media-and-desktop-snapshot.md',
        ],
      },
    ],
  },
];

const enSections: SectionSpec[] = [
  {
    text: 'Getting Started',
    files: ['en/user/getting-started.md', 'en/user/which-build.md'],
  },
  {
    text: 'Guides',
    files: ['en/user/README.md'],
    dirs: [
      {
        dir: 'en/user',
        exclude: ['en/user/README.md', 'en/user/getting-started.md', 'en/user/which-build.md'],
      },
    ],
  },
  {
    text: 'Development',
    // Mirrors the Chinese 开发 section: index page first, then the hand-picked
    // groups, then an Other group generated from the directory minus everything
    // placed above (so a new English page shows up there instead of dropping out
    // of the sidebar). `standalone-deployment.md` lives under Deployment; pages
    // still missing an English counterpart stay on the Chinese tree and are
    // linked from the index page.
    files: ['en/developer/README.md'],
    dirs: [
      { label: 'Concepts', files: EN_DEV_CONCEPTS },
      { label: 'Tools and protocols', files: EN_DEV_TOOLS },
      { label: 'Integration', files: EN_DEV_INTEGRATION },
      { label: 'Troubleshooting', files: EN_DEV_TROUBLESHOOTING },
      {
        dir: 'en/developer',
        label: 'Other',
        exclude: [...EN_DEV_PLACED, 'en/developer/README.md', 'en/developer/standalone-deployment.md'],
      },
    ],
  },
  {
    text: 'Deployment',
    files: ['en/developer/standalone-deployment.md'],
    dirs: [{ dir: 'en/deploy' }],
  },
  {
    text: 'Contributing',
    // The two UI-implementation pages are listed under Internals → UI notes
    // instead (mirrors the Chinese 参与贡献 section). Placement is expressed here;
    // the files never move, because their URLs are linked from ui/visual-theme.md
    // and from source comments.
    files: ['en/DEVELOPMENT.md'],
    dirs: [
      {
        dir: 'en/contributing',
        exclude: [
          'en/contributing/macos-window-chrome.md',
          'en/contributing/web-media-and-desktop-snapshot.md',
        ],
      },
    ],
  },
  {
    text: 'Internals',
    dirs: [
      { dir: 'en/internals', label: 'Runtime internals' },
      { dir: 'en/agents', label: 'Sub-agent prompts' },
      { dir: 'en/llm', label: 'LLM integration' },
      {
        dir: 'en/ui',
        label: 'UI notes',
        files: [
          'en/contributing/macos-window-chrome.md',
          'en/contributing/web-media-and-desktop-snapshot.md',
        ],
      },
    ],
  },
];

export default defineConfig({
  // The site mounts the existing `docs/` tree in place — nothing is copied or moved.
  srcDir: SRC_DIR,
  srcExclude: SRC_EXCLUDE_GLOBS,
  rewrites,
  base: SITE_BASE,
  cleanUrls: true, // GitHub Pages serves `/foo` from `foo.html` natively
  title: 'Pointer',
  description: 'Pointer documentation: installation, usage, development and deployment',
  lastUpdated: false,
  ignoreDeadLinks: false,
  sitemap: { hostname: 'https://chinphing.github.io/agent-pointer/' },

  markdown: {
    config: (md) => {
      strayHtml(md);
      docLinks(md);
    },
  },

  vite: {
    // The docs site is a standalone VitePress project: do not inherit the app's
    // repo-root `postcss.config.js` (Tailwind + autoprefixer).
    css: { postcss: { plugins: [] } },
    // `vue` must come from this workspace, not from the importer's directory —
    // the markdown sources live outside `docs-site/`. See `vueFromSiteRoot`.
    plugins: [vueFromSiteRoot()],
  },

  transformPageData(pageData) {
    const rel = pageData.relativePath;
    // Maintainer-notes banner (tier 2), injected from config — no frontmatter in the docs.
    // `MAINTAINER_NOTE_EXTRA` covers pages placed under 维护者笔记 by config only.
    if (/^(zh-CN\/)?(internals|agents|llm|ui)\//.test(rel) || MAINTAINER_NOTE_EXTRA.includes(rel)) {
      pageData.frontmatter.maintainerNote = rel.startsWith('zh-CN/')
        ? maintainerNotes['zh-CN']
        : maintainerNotes.root;
    }
    // Language-switch target: the translated counterpart when it is published,
    // otherwise the other locale's landing page — never a link to a 404.
    const altLocale = alternateLocale(rel, siteIndex);
    if (altLocale) pageData.frontmatter.altLocale = altLocale;
    // Hero landing page for both locale indexes, also injected at build time.
    if (rel === 'index.md') {
      // English — the default locale, served at `/`.
      pageData.frontmatter.layout = 'home';
      // The hero carries the product positioning sentence from the root README.
      // The landing pages have no H1 of their own, so the `<title>` is set here.
      pageData.frontmatter.title = 'Documentation';
      pageData.title = 'Documentation';
      pageData.frontmatter.hero = {
        name: 'Pointer',
        tagline:
          'An out-of-the-box, ultra-low-resource desktop agent foundation for building digital employees. Built for developers and the enterprise.',
        actions: [
          { theme: 'brand', text: 'Install & first chat', link: '/user/getting-started' },
          { theme: 'alt', text: 'Developer guide', link: '/developer/' },
        ],
      };
      pageData.frontmatter.features = [
        { title: 'Guides', details: 'Chat, Skills, IM channels, MCP, plugins, sub-agents', link: '/user/' },
        {
          title: 'Development',
          details: 'Architecture, tool-calling protocol, integrations, troubleshooting',
          link: '/developer/',
        },
        { title: 'Deployment', details: 'Packaging flavours × runtime shapes, platform matrix', link: '/deploy/' },
        { title: 'Contributing', details: 'Cross-platform builds, versioning, packaging, dev setup', link: '/DEVELOPMENT' },
        { title: 'Internals', details: 'Runtime mechanics, sub-agent prompts, LLM integration, UI notes', link: '/internals/' },
      ];
    } else if (rel === 'zh-CN/index.md') {
      // Chinese — the complete tree, served under `/zh-CN/`.
      pageData.frontmatter.layout = 'home';
      pageData.frontmatter.title = '文档';
      pageData.title = '文档';
      pageData.frontmatter.hero = {
        name: 'Pointer',
        tagline: '开箱即用、超级低资源消耗的桌面智能体基座，用来打造数字员工。面向开发者和企业。',
        actions: [
          { theme: 'brand', text: '安装与第一次对话', link: '/zh-CN/user/getting-started' },
          { theme: 'alt', text: '开发者指南', link: '/zh-CN/developer/' },
        ],
      };
      pageData.frontmatter.features = [
        { title: '使用指南', details: '对话、Skills、IM 通道、MCP、插件、子代理', link: '/zh-CN/user/' },
        { title: '开发', details: '架构与概念、工具与协议、集成、排障', link: '/zh-CN/developer/' },
        { title: '部署', details: '打包口味 × 运行形态、平台矩阵', link: '/zh-CN/deploy/' },
        { title: '参与贡献', details: '跨平台构建、版本规范、打包、开发环境', link: '/zh-CN/contributing/' },
        { title: '维护者笔记', details: '运行时机制、子代理提示词、LLM 接入、UI 笔记', link: '/zh-CN/internals/' },
      ];
    }
  },

  // VitePress reads the locale config from the **top level** (`locales`), not
  // from `themeConfig.locales`. Nesting it under `themeConfig` is silently
  // ignored: `site.locales` stays empty, so every page resolves to the root
  // locale — `<html lang>` stays `en-US` and the per-locale nav, sidebar and
  // UI labels never reach the built site.
  locales: {
    root: {
      label: 'English',
      // Keep VitePress' default language tag for the English tree.
      lang: 'en-US',
      link: '/',
      description: 'Pointer documentation: installation, usage, development and deployment',
      themeConfig: {
        nav: [
          { text: 'Getting Started', link: '/user/getting-started' },
          { text: 'Guides', link: '/user/' },
          { text: 'Development', link: '/developer/' },
          { text: 'Deployment', link: '/deploy/' },
          { text: 'Contributing', link: '/contributing/' },
          { text: 'Internals', link: '/internals/' },
        ],
        sidebar: buildSidebar(enSections),
      },
    },
    'zh-CN': {
      label: '简体中文',
      lang: 'zh-CN',
      link: '/zh-CN/',
      description: 'Pointer 文档：安装、使用、开发与部署',
      themeConfig: {
        footer: {
          message:
            '英文树仍在补全：目前覆盖部署入口、各入口索引页与部分用户 / 开发者指南；完整树就是中文文档（/zh-CN/）。',
        },
        outline: { level: [2, 3], label: '本页目录' },
        editLink: {
          pattern: `https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}/edit/main/docs/:path`,
          text: '在 GitHub 上编辑此页',
        },
        docFooter: { prev: '上一篇', next: '下一篇' },
        darkModeSwitchLabel: '外观',
        returnToTopLabel: '回到顶部',
        sidebarMenuLabel: '目录',
        nav: [
          { text: '快速开始', link: '/zh-CN/user/getting-started' },
          { text: '使用指南', link: '/zh-CN/user/' },
          { text: '开发', link: '/zh-CN/developer/' },
          { text: '部署', link: '/zh-CN/deploy/' },
          { text: '参与贡献', link: '/zh-CN/contributing/' },
          { text: '维护者笔记', link: '/zh-CN/internals/' },
        ],
        sidebar: buildSidebar(zhSections),
      },
    },
  },

  // Shared chrome, merged into every locale; the `zh-CN` locale overrides the
  // labels below in its own `themeConfig`.
  themeConfig: {
    logo: undefined,
    // Only the landing pages are sidebar-less, so this footer — and its
    // "English tree still being filled in" note — shows there alone.
    footer: {
      message:
        'The English tree is still being filled in: it covers the deployment entry, the audience index pages and part of the user and developer guides. The complete tree is the Chinese documentation under /zh-CN/.',
    },
    // Deliberately no English `outline.label`: the top-level `themeConfig` is
    // serialised into `__VP_SITE_DATA__` on *every* page, so an English label
    // here leaks "On this page" into the Chinese HTML. English keeps
    // VitePress' own default ('On this page'); `zh-CN` overrides it per locale.
    outline: { level: [2, 3] },
    search: { provider: 'local' },
    socialLinks: [{ icon: 'github', link: `https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}` }],
    editLink: {
      pattern: `https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}/edit/main/docs/:path`,
      text: 'Edit this page on GitHub',
    },
    lastUpdated: undefined,
    docFooter: { prev: 'Previous', next: 'Next' },
    darkModeSwitchLabel: 'Appearance',
    returnToTopLabel: 'Return to top',
    sidebarMenuLabel: 'Menu',
    externalLinkIcon: true,
    // The visible language switcher is our own (Layout.vue + the build-time
    // `alternateLocale` frontmatter); the built-in menu is hidden in
    // custom.css. Turning off `i18nRouting` keeps that hidden menu's links on
    // the locale landing pages instead of pointing at untranslated pages.
    i18nRouting: false,
  },
});
