import { defineConfig } from 'vitepress';
import { docLinks, strayHtml } from './doc-links.mts';
import { buildSidebar, type SectionSpec } from './sidebar.mts';
import {
  GITHUB_OWNER,
  GITHUB_REPO,
  SITE_BASE,
  SRC_DIR,
  SRC_EXCLUDE_GLOBS,
  alternateLocale,
  buildSiteIndex,
  rewritePage,
} from '../site-map.mjs';

/** Published file path for every page. `en/` is stripped, `README.md` → `index.md`. */
const rewrites = (page: string) => rewritePage(page);

/** Published page index, built once — powers the language-switch targets below. */
const siteIndex = buildSiteIndex();

/** Tier-2 banner text, per locale; the docs carry no frontmatter of their own. */
const maintainerNotes = {
  root: '**Maintainer notes** — implementation details for core maintainers, published with the repository. Not a user tutorial.',
  'zh-CN': '**维护者笔记 · Maintainer notes** — 面向核心维护者的实现细节，随仓库公开，不是用户教程。',
} as const;

/** Tier 1 + tier 2 sections; tier 3 (`design/`, `plans/`, `settings-refactor/`, `superpowers/`) stays off the site. */
const zhSections: SectionSpec[] = [
  {
    text: '使用指南',
    dirs: [{ dir: 'zh-CN/user' }],
  },
  {
    text: '部署',
    dirs: [{ dir: 'zh-CN/deploy' }],
  },
  {
    text: '开发者',
    dirs: [{ dir: 'zh-CN/developer' }],
  },
  {
    text: '参与贡献',
    dirs: [{ dir: 'zh-CN/contributing' }],
  },
  {
    text: '维护者笔记',
    dirs: [
      { dir: 'zh-CN/internals', label: '运行时内部机制' },
      { dir: 'zh-CN/agents', label: '子代理提示词' },
      { dir: 'zh-CN/llm', label: 'LLM 接入' },
      { dir: 'zh-CN/ui', label: 'UI 笔记' },
    ],
  },
];

const enSections: SectionSpec[] = [
  { text: 'Guides', dirs: [{ dir: 'en/user' }] },
  { text: 'Deploy', dirs: [{ dir: 'en/deploy' }] },
  {
    text: 'Developers',
    items: [{ text: 'Development & debugging', link: '/DEVELOPMENT' }],
    dirs: [{ dir: 'en/developer' }],
  },
  { text: 'Contributing', dirs: [{ dir: 'en/contributing' }] },
  {
    text: 'Maintainer notes',
    dirs: [
      { dir: 'en/internals', label: 'Runtime internals' },
      { dir: 'en/agents', label: 'Sub-agent prompts' },
      { dir: 'en/llm', label: 'LLM integration' },
      { dir: 'en/ui', label: 'UI notes' },
    ],
  },
];

/** Curated first screen; these pages are skipped by the generated sections below. */
const zhGettingStarted = [
  { text: '文档索引', link: '/zh-CN/' },
  { text: '安装与第一次对话', link: '/zh-CN/user/getting-started' },
  { text: '选哪个构建（账号与版本）', link: '/zh-CN/user/which-build' },
  { text: '云端主机', link: '/zh-CN/user/cloud-host' },
];

const enGettingStarted = [
  { text: 'Docs index', link: '/' },
  { text: 'Install & first chat', link: '/user/getting-started' },
  { text: 'Which build', link: '/user/which-build' },
  { text: 'Cloud host', link: '/user/cloud-host' },
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
  },

  transformPageData(pageData) {
    const rel = pageData.relativePath;
    // Maintainer-notes banner (tier 2), injected from config — no frontmatter in the docs.
    if (/^(zh-CN\/)?(internals|agents|llm|ui)\//.test(rel)) {
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
      pageData.frontmatter.hero = {
        name: 'Pointer',
        text: 'Documentation',
        tagline: 'Desktop / web / server agent application',
        actions: [
          { theme: 'brand', text: 'Install & first chat', link: '/user/getting-started' },
          { theme: 'alt', text: 'Developer guide', link: '/developer/' },
        ],
      };
      pageData.frontmatter.features = [
        { title: 'Guides', details: 'Skills, IM channels, sub-agents, MCP', link: '/user/' },
        { title: 'Deploy', details: 'Packaging flavours × runtime shapes', link: '/deploy/' },
        { title: 'Developers', details: 'Architecture, native tool calling, extension hooks', link: '/developer/' },
      ];
    } else if (rel === 'zh-CN/index.md') {
      // Chinese — the complete tree, served under `/zh-CN/`.
      pageData.frontmatter.layout = 'home';
      pageData.frontmatter.hero = {
        name: 'Pointer',
        text: '文档',
        tagline: '桌面 / Web / 服务端一体的 Agent 应用',
        actions: [
          { theme: 'brand', text: '安装与第一次对话', link: '/zh-CN/user/getting-started' },
          { theme: 'alt', text: '开发者指南', link: '/zh-CN/developer/' },
        ],
      };
      pageData.frontmatter.features = [
        { title: '使用指南', details: 'Skills、IM 通道、子代理、MCP、自建服务端', link: '/zh-CN/user/' },
        { title: '开发者', details: '架构、原生工具调用协议、扩展钩子', link: '/zh-CN/developer/' },
        { title: '部署', details: '打包口味 × 运行形态', link: '/zh-CN/deploy/' },
        { title: '参与贡献', details: '跨平台构建、版本与提交规范', link: '/zh-CN/contributing/' },
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
          { text: 'Guides', link: '/user/' },
          { text: 'Developers', link: '/developer/' },
          { text: 'Deploy', link: '/deploy/' },
          { text: 'Contributing', link: '/contributing/' },
          { text: 'Maintainer notes', link: '/internals/' },
        ],
        sidebar: [
          { text: 'Getting started', collapsed: false, items: enGettingStarted },
          ...buildSidebar(enSections, ['en/user/getting-started.md', 'en/user/which-build.md', 'en/user/cloud-host.md']),
        ],
      },
    },
    'zh-CN': {
      label: '简体中文',
      lang: 'zh-CN',
      link: '/zh-CN/',
      description: 'Pointer 文档：安装、使用、开发与部署',
      themeConfig: {
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
          { text: '使用指南', link: '/zh-CN/user/' },
          { text: '开发者', link: '/zh-CN/developer/' },
          { text: '部署', link: '/zh-CN/deploy/' },
          { text: '参与贡献', link: '/zh-CN/contributing/' },
          { text: '维护者笔记', link: '/zh-CN/internals/' },
        ],
        sidebar: [
          { text: '快速开始', collapsed: false, items: zhGettingStarted },
          ...buildSidebar(zhSections, ['zh-CN/user/getting-started.md', 'zh-CN/user/which-build.md', 'zh-CN/user/cloud-host.md']),
        ],
      },
    },
  },

  // Shared chrome, merged into every locale; the `zh-CN` locale overrides the
  // labels below in its own `themeConfig`.
  themeConfig: {
    logo: undefined,
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
