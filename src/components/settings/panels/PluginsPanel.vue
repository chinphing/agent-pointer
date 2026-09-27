<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { RefreshCw, Plus, Power, Trash2, Upload, FolderOpen, Download, ChevronDown, FileArchive } from 'lucide-vue-next'
import { open } from '@tauri-apps/plugin-dialog'
import { isTauriRuntime } from '../../../lib/runtime'
import { useSkillsStore } from '../../../stores/skills'
import {
  listPlugins,
  enablePlugin,
  disablePlugin,
  uninstallPlugin,
  importPlugin,
  importPluginZip,
  probeExternalPlugins,
  importExternalPlugin
} from '../../../lib/api'
import type { PluginView, ExternalPluginsProbeResult, ImportReport } from '../../../types/plugin'

const { t } = useI18n()

const plugins = ref<PluginView[]>([])
const loading = ref(false)
const busyId = ref('')
const message = ref('')
const sourcePath = ref('')
const showImport = ref(false)
const importing = ref(false)
const external = ref<ExternalPluginsProbeResult | null>(null)
const probingExternal = ref(false)
const skillsStore = useSkillsStore()
const zipInput = ref<HTMLInputElement | null>(null)
/** 当前展开详情的插件 id（空 = 全部收起）。 */
const expandedId = ref('')

function toggleDetails(id: string) {
  expandedId.value = expandedId.value === id ? '' : id
}

function statusLabel(status: string): string {
  const key = `settings.plugins.status.${status}`
  const labeled = t(key)
  return labeled !== key ? labeled : status
}

const enabledCount = computed(() => plugins.value.filter(p => p.isEnabled).length)

const isDesktop = isTauriRuntime()

/** 桌面端「导入插件」下拉菜单：合并目录导入与 zip 导入两个入口。 */
const importMenuOpen = ref(false)
const importTriggerRef = ref<HTMLButtonElement | null>(null)

/** 滚动时关闭菜单（fixed 菜单不跟随滚动，避免漂移/遮挡感）。 */
const onGlobalScroll = () => {
  importMenuOpen.value = false
}

onMounted(() => {
  void refresh()
  void probeExternal()
  document.addEventListener('click', onGlobalClick)
  document.addEventListener('keydown', onGlobalKeydown)
  document.addEventListener('scroll', onGlobalScroll, true)
})

onUnmounted(() => {
  document.removeEventListener('click', onGlobalClick)
  document.removeEventListener('keydown', onGlobalKeydown)
  document.removeEventListener('scroll', onGlobalScroll, true)
})

function toggleImportMenu() {
  importMenuOpen.value = !importMenuOpen.value
}

function closeImportMenu() {
  importMenuOpen.value = false
}

function onGlobalClick(event: MouseEvent) {
  if (!importMenuOpen.value) return
  const target = event.target as Element
  if (!target.closest('.plugin-import-trigger, .plugin-import-panel')) {
    importMenuOpen.value = false
  }
}

function onGlobalKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') importMenuOpen.value = false
}

/** 菜单尺寸：宽度自适应内容（w-max），估算高度与防右溢出上限（px）。 */
const IMPORT_MENU_WIDTH = 160
const IMPORT_MENU_HEIGHT = 80

const importMenuStyle = computed(() => {
  const rect = importTriggerRef.value?.getBoundingClientRect()
  if (!rect) return {}
  const gap = 8
  const viewH = window.innerHeight
  const viewW = window.innerWidth
  // 下方空间不足时向上弹出，避免菜单被推出屏幕
  const showBelow = viewH - rect.bottom >= IMPORT_MENU_HEIGHT + gap
  const left = Math.max(gap, Math.min(rect.left, viewW - IMPORT_MENU_WIDTH - gap))
  return showBelow
    ? { top: `${rect.bottom + gap}px`, left: `${left}px` }
    : { bottom: `${viewH - rect.top + gap}px`, left: `${left}px` }
})

async function refresh() {
  loading.value = true
  try {
    plugins.value = await listPlugins()
  } catch (err: unknown) {
    message.value = err instanceof Error ? err.message : String(err)
  } finally {
    loading.value = false
  }
}

/** 探测本机 Claude / Codex 插件来源（新装后主动提示可导入项）。 */
async function probeExternal() {
  probingExternal.value = true
  try {
    external.value = await probeExternalPlugins()
  } catch {
    external.value = null
  } finally {
    probingExternal.value = false
  }
}

async function toggle(p: PluginView) {
  busyId.value = p.pluginId
  message.value = ''
  try {
    if (p.isEnabled) {
      await disablePlugin(p.pluginId)
    } else if (p.status === 'rejected') {
      message.value = t('settings.plugins.validationFailed', { name: p.name })
    } else {
      // needs_reauth / discovered / disabled → 授权并启用
      await enablePlugin(p.pluginId)
    }
    await refresh()
    // 插件启用/禁用会增删技能注册 → 同步技能面板
    await skillsStore.load({ rescan: true }).catch(() => {})
  } catch (err: unknown) {
    message.value = err instanceof Error ? err.message : String(err)
  } finally {
    busyId.value = ''
  }
}

async function onUninstall(p: PluginView) {
  const ok = window.confirm(t('settings.plugins.confirmUninstall', { name: p.name }))
  if (!ok) return
  busyId.value = p.pluginId
  message.value = ''
  try {
    await uninstallPlugin(p.pluginId)
    await refresh()
    await skillsStore.load({ rescan: true }).catch(() => {})
  } catch (err: unknown) {
    message.value = err instanceof Error ? err.message : String(err)
  } finally {
    busyId.value = ''
  }
}

/** 选目录并自动导入：桌面端弹出目录选择器，选中顶层目录后按顺序自动导入全部插件候选。 */
async function pickAndImport() {
  if (!isDesktop) {
    showImport.value = !showImport.value
    return
  }
  try {
    const selected = await open({ directory: true, title: t('settings.plugins.pickDirectoryTitle') })
    if (typeof selected === 'string' && selected) {
      await doImport(selected)
    }
  } catch (err: unknown) {
    message.value = err instanceof Error ? err.message : String(err)
  }
}

/** 桌面端：选择 zip 文件导入（一个 zip 可含单个或多个插件）。 */
async function pickZipAndImport() {
  if (!isDesktop) return
  try {
    const selected = await open({
      multiple: false,
      filters: [{ name: t('settings.plugins.zipFilterName'), extensions: ['zip'] }],
      title: t('settings.plugins.pickZipTitle')
    })
    if (typeof selected === 'string' && selected) {
      await doImport(selected)
    }
  } catch (err: unknown) {
    message.value = err instanceof Error ? err.message : String(err)
  }
}

async function onImport() {
  const source = sourcePath.value.trim()
  if (!source) return
  await doImport(source)
}

/** Web 端：隐藏 file input 选择 zip → 上传导入。 */
async function onPickZipFile() {
  zipInput.value?.click()
}

async function onZipFileChange(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  input.value = ''
  if (!file) return
  await doImportZip(file)
}

async function doImportZip(file: File) {
  importing.value = true
  message.value = ''
  try {
    const reports = await importPluginZip(file)
    if (reports.length === 0) {
      message.value = t('settings.plugins.noMetadataInFile', { name: file.name })
      return
    }
    const lines = reports.map(formatReportLine)
    message.value = t('settings.plugins.importedCount', { count: reports.length, lines: lines.join('\n') })
    showImport.value = false
    await refresh()
    await skillsStore.load({ rescan: true }).catch(() => {})
  } catch (err: unknown) {
    message.value = err instanceof Error ? err.message : String(err)
  } finally {
    importing.value = false
  }
}

function formatReportLine(r: ImportReport): string {
  const sep = t('automation.cron.channelSep')
  const converted = r.converted.join(sep)
  const skipped = r.skipped.length ? t('settings.plugins.skippedSuffix', { items: r.skipped.join(sep) }) : ''
  const unmapped = r.unmapped.length ? t('settings.plugins.unmappedSuffix', { count: r.unmapped.length }) : ''
  return `「${r.pluginName}」（${r.pluginId}）：${converted}${skipped}${unmapped}`
}

/** 批量导入：后端自动按 Pointer → Codex → Claude 顺序逐个导入目录/zip 中的插件候选。 */
async function doImport(source: string) {
  importing.value = true
  message.value = ''
  try {
    const reports = await importPlugin(source)
    if (reports.length === 0) {
      message.value = t('settings.plugins.noMetadataInSource', { source })
      return
    }
    const lines = reports.map(formatReportLine)
    message.value = t('settings.plugins.importedCount', { count: reports.length, lines: lines.join('\n') })
    sourcePath.value = ''
    showImport.value = false
    await refresh()
    await skillsStore.load({ rescan: true }).catch(() => {})
  } catch (err: unknown) {
    message.value = err instanceof Error ? err.message : String(err)
  } finally {
    importing.value = false
  }
}

/** 一键导入外部探测到的插件来源。 */
async function onImportExternal(sourceId: string) {
  busyId.value = sourceId
  message.value = ''
  try {
    const report = await importExternalPlugin(sourceId)
    message.value = t('settings.plugins.importedExternal', { name: report.pluginName, id: report.pluginId, converted: report.converted.join(t('automation.cron.channelSep')) })
    await refresh()
    await probeExternal()
  } catch (err: unknown) {
    message.value = err instanceof Error ? err.message : String(err)
  } finally {
    busyId.value = ''
  }
}

async function dismissExternal() {
  external.value = null
}
</script>

<template>
  <div class="space-y-5">
    <div class="flex items-start justify-end gap-4">
      <div class="flex shrink-0 items-center gap-2">
        <button
          type="button"
          class="h-9 rounded-lg border border-border bg-card px-3 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
          :disabled="loading"
          @click="refresh"
        >
          <span class="inline-flex items-center gap-1.5">
            <RefreshCw class="h-3.5 w-3.5 text-accent" aria-hidden="true" />
            {{ t('common.refresh') }}
          </span>
        </button>
        <button
          v-if="isDesktop"
          ref="importTriggerRef"
          type="button"
          class="plugin-import-trigger h-9 rounded-lg border border-border bg-card px-3 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
          :disabled="importing"
          :aria-expanded="importMenuOpen"
          aria-haspopup="menu"
          @click="toggleImportMenu"
        >
          <span class="inline-flex items-center gap-1.5">
            <Upload class="h-3.5 w-3.5 text-accent" aria-hidden="true" />
            {{ t('settings.plugins.importPlugins') }}
            <ChevronDown
              class="h-3.5 w-3.5 text-muted transition-transform"
              :class="importMenuOpen ? 'rotate-180' : ''"
              aria-hidden="true"
            />
          </span>
        </button>
        <button
          v-else
          type="button"
          class="h-9 rounded-lg border border-border bg-card px-3 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
          :disabled="importing"
          @click="pickAndImport"
        >
          <span class="inline-flex items-center gap-1.5">
            <Plus class="h-3.5 w-3.5 text-accent" aria-hidden="true" />
            {{ t('settings.plugins.import') }}
          </span>
        </button>
        <Teleport to="body">
          <div
            v-if="importMenuOpen && isDesktop"
            class="plugin-import-panel fixed z-[310] w-max min-w-36 rounded-xl border border-border bg-card p-1 shadow-xl"
            :style="importMenuStyle"
            role="menu"
            :aria-label="t('settings.plugins.importPlugins')"
          >
            <button
              type="button"
              role="menuitem"
              class="w-full flex items-center gap-2 rounded-lg px-2 py-1.5 text-left text-[13px] text-foreground hover:bg-hover transition-colors cursor-pointer disabled:opacity-50"
              :disabled="importing"
              @click="closeImportMenu(); pickAndImport()"
            >
              <FolderOpen class="h-4 w-4 shrink-0 text-muted" aria-hidden="true" />
              {{ t('settings.plugins.importDirectory') }}
            </button>
            <button
              type="button"
              role="menuitem"
              class="w-full flex items-center gap-2 rounded-lg px-2 py-1.5 text-left text-[13px] text-foreground hover:bg-hover transition-colors cursor-pointer disabled:opacity-50"
              :disabled="importing"
              @click="closeImportMenu(); pickZipAndImport()"
            >
              <FileArchive class="h-4 w-4 shrink-0 text-muted" aria-hidden="true" />
              {{ t('settings.plugins.importZip') }}
            </button>
          </div>
        </Teleport>
      </div>
    </div>

    <div v-if="message" class="rounded-xl border border-border bg-accent-muted/50 px-3 py-2 text-sm text-foreground whitespace-pre-line">
      {{ message }}
    </div>

    <!-- 外部插件探测横幅（新装后主动提示） -->
    <div
      v-if="external && external.total > 0"
      class="rounded-2xl border border-border bg-accent-muted/30 p-4"
    >
      <div class="flex items-start justify-between gap-3">
        <div class="flex items-center gap-2">
          <Download class="h-4 w-4 text-accent" aria-hidden="true" />
          <p class="text-sm font-medium text-foreground">
            {{ t('settings.plugins.externalDetected', { total: external.total }) }}
          </p>
        </div>
        <button
          type="button"
          class="text-xs text-muted hover:text-foreground cursor-pointer"
          @click="dismissExternal"
        >
          {{ t('settings.plugins.dismiss') }}
        </button>
      </div>
      <div class="mt-3 space-y-2">
        <div
          v-for="src in external.sources"
          :key="src.id"
          class="flex items-center justify-between gap-3 rounded-xl border border-border bg-card px-3 py-2"
        >
          <div class="min-w-0">
            <p class="truncate text-sm text-foreground">{{ src.pluginName }} <span class="text-muted">v{{ src.pluginVersion }}</span></p>
            <p class="truncate text-xs text-muted">{{ src.path }}</p>
            <p v-if="src.description" class="truncate text-xs text-muted">{{ src.description }}</p>
          </div>
          <button
            type="button"
            class="inline-flex h-8 shrink-0 items-center gap-1.5 rounded-lg bg-accent px-3 text-xs font-medium text-accent-foreground hover:opacity-90 cursor-pointer transition-opacity disabled:opacity-50"
            :disabled="busyId === src.id"
            @click="onImportExternal(src.id)"
          >
            <Download class="h-3.5 w-3.5" aria-hidden="true" />
            {{ busyId === src.id ? t('settings.plugins.importing') : t('settings.plugins.import') }}
          </button>
        </div>
      </div>
    </div>
    <div v-else-if="probingExternal" class="text-xs text-muted">{{ t('settings.plugins.probingExternal') }}</div>

    <!-- Web 端路径/zip 导入降级 -->
    <div v-if="showImport && !isDesktop" class="rounded-2xl border border-border panel p-5">
      <p class="mb-2 text-xs font-medium uppercase tracking-wide text-muted">{{ t('settings.plugins.importWebHeading') }}</p>
      <div class="flex flex-col gap-2 sm:flex-row">
        <input
          v-model="sourcePath"
          type="text"
          :placeholder="t('settings.plugins.pathPlaceholder')"
          class="h-9 flex-1 rounded-lg border border-border bg-card px-3 text-sm text-foreground outline-none focus:border-accent"
          @keyup.enter="onImport"
        />
        <button
          type="button"
          class="h-9 shrink-0 rounded-lg bg-accent px-4 text-sm font-medium text-accent-foreground hover:opacity-90 cursor-pointer transition-opacity disabled:opacity-50"
          :disabled="importing || !sourcePath.trim()"
          @click="onImport"
        >
          <span class="inline-flex items-center gap-1.5">
            <Upload class="h-3.5 w-3.5" aria-hidden="true" />
            {{ importing ? t('settings.plugins.importing') : t('settings.plugins.import') }}
          </span>
        </button>
        <button
          type="button"
          class="h-9 shrink-0 rounded-lg border border-border bg-card px-4 text-sm text-foreground hover:bg-hover cursor-pointer transition-opacity disabled:opacity-50"
          :disabled="importing"
          @click="onPickZipFile"
        >
          <span class="inline-flex items-center gap-1.5">
            <Upload class="h-3.5 w-3.5 text-accent" aria-hidden="true" />
            {{ importing ? t('settings.plugins.importing') : t('settings.plugins.uploadZip') }}
          </span>
        </button>
        <input ref="zipInput" type="file" accept=".zip,application/zip" class="hidden" @change="onZipFileChange" />
      </div>
    </div>

    <div v-if="loading" class="py-8 text-center text-sm text-muted">{{ t('common.loading') }}</div>

    <div v-else-if="plugins.length === 0" class="rounded-2xl border border-dashed border-border py-10 text-center text-sm text-muted">
      {{ t('settings.plugins.empty') }} <code class="rounded bg-[hsl(var(--code-bg))] px-1.5 py-0.5 text-xs">~/.pointer/plugins</code>.
    </div>

    <div v-else class="space-y-3">
      <div class="flex items-center justify-between text-xs text-muted">
        <span>{{ t('settings.plugins.pluginCount', { count: plugins.length, enabled: enabledCount }) }}</span>
      </div>
      <div
        v-for="p in plugins"
        :key="p.pluginId"
        class="rounded-2xl border border-border panel p-4"
      >
        <div class="flex items-start justify-between gap-3">
          <div class="min-w-0">
            <div class="flex items-center gap-2">
              <h4 class="truncate text-sm font-semibold text-foreground">{{ p.name }}</h4>
              <span
                class="shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium"
                :class="p.isEnabled ? 'bg-accent-muted text-accent' : 'bg-[hsl(var(--code-bg))] text-muted'"
              >
                {{ statusLabel(p.status) }}
              </span>
              <span v-if="!p.isUserLevel" class="shrink-0 rounded-full bg-[hsl(var(--code-bg))] px-2 py-0.5 text-[10px] text-muted">{{ t('settings.plugins.workspace') }}</span>
            </div>
            <p class="mt-1 text-xs text-muted">{{ p.description || t('settings.plugins.noDescription') }}</p>
            <p class="mt-1 font-mono text-[11px] text-muted">{{ p.pluginId }} · v{{ p.version }}</p>
            <p v-if="p.statusReason" class="mt-1 text-[11px] text-danger">{{ p.statusReason }}</p>
          </div>
          <div class="flex shrink-0 items-center gap-1.5">
            <button
              type="button"
              class="inline-flex h-8 items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 text-xs text-foreground hover:bg-hover cursor-pointer transition-colors"
              :aria-expanded="expandedId === p.pluginId"
              @click="toggleDetails(p.pluginId)"
            >
              <ChevronDown
                class="h-3.5 w-3.5 transition-transform"
                :class="expandedId === p.pluginId ? 'rotate-180' : ''"
                aria-hidden="true"
              />
              {{ t('settings.plugins.details') }}
            </button>
            <button
              type="button"
              class="inline-flex h-8 items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 text-xs text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
              :disabled="busyId === p.pluginId || p.status === 'rejected'"
              @click="toggle(p)"
            >
              <Power class="h-3.5 w-3.5" aria-hidden="true" />
              {{ p.isEnabled ? t('common.disable') : (p.status === 'rejected' ? t('settings.plugins.cannotEnable') : t('common.enable')) }}
            </button>
            <button
              type="button"
              class="inline-flex h-8 items-center gap-1.5 rounded-lg border border-border bg-card px-2.5 text-xs text-foreground hover:bg-danger/10 hover:text-danger cursor-pointer transition-colors disabled:opacity-50"
              :disabled="busyId === p.pluginId"
              @click="onUninstall(p)"
            >
              <Trash2 class="h-3.5 w-3.5" aria-hidden="true" />
              {{ t('settings.plugins.uninstall') }}
            </button>
          </div>
        </div>

        <!-- 详情：能力单元清单 + 元信息 -->
        <div
          v-if="expandedId === p.pluginId"
          class="mt-3 rounded-xl border border-border bg-[hsl(var(--code-bg))]/40 p-3"
        >
          <div class="flex flex-wrap items-center gap-2">
            <span class="text-[11px] font-medium text-muted">{{ t('settings.plugins.capabilities') }}</span>
            <span
              v-for="cap in p.capabilities"
              :key="cap"
              class="rounded-full bg-accent/10 px-2 py-0.5 text-[10px] font-medium text-accent"
            >
              {{ cap }}
            </span>
            <span v-if="!p.capabilities.length" class="text-[11px] text-muted">{{ t('settings.plugins.noCapabilities') }}</span>
          </div>
          <p class="mt-2 text-[11px] text-muted">
            {{ t('settings.plugins.locationPrefix') }}<code class="rounded bg-[hsl(var(--code-bg))] px-1 py-0.5">{{ p.isUserLevel ? '~/.pointer/plugins' : t('settings.plugins.workspacePluginsPath') }}/{{ p.pluginId }}</code>
            · {{ t('settings.plugins.authPrefix') }}{{ p.isAuthorized ? t('settings.plugins.authorized') : t('settings.plugins.unauthorized') }}
            · {{ t('settings.plugins.statusPrefix') }}{{ statusLabel(p.status) }}
          </p>
        </div>
      </div>
    </div>
  </div>
</template>
