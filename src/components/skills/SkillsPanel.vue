<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Search, Wrench, Upload } from 'lucide-vue-next'
import { useSkillsStore } from '../../stores/skills'
import { listPlugins } from '../../lib/api'
import type { SkillDef } from '../../types/chat'

const { t } = useI18n()

const AGENT_TABS = [
  { id: 'general', label: () => t('skills.agentTabs.general') },
  { id: 'coder', label: () => t('skills.agentTabs.coder') },
  { id: 'computer', label: () => t('skills.agentTabs.computer') }
] as const

const skills = useSkillsStore()
const q = ref('')
const importing = ref(false)
const loading = ref(false)
const importMessage = ref('')
const fileInput = ref<HTMLInputElement | null>(null)
const selectedAgentId = ref<string>('general')
/** 来源筛选：all / user / system / external / plugin */
const sourceFilter = ref<string>('all')
/** 插件 id → 插件名（用于来源标注）。 */
const pluginNames = ref<Record<string, string>>({})

const currentAgentId = computed(() => selectedAgentId.value)

onMounted(() => {
  void refreshSkills()
  void loadPluginNames()
})

/** 拉取插件列表，构建 pluginId → 插件名 映射，供技能来源徽标显示插件名。 */
async function loadPluginNames() {
  try {
    const plugins = await listPlugins()
    pluginNames.value = Object.fromEntries(plugins.map(p => [p.pluginId, p.name]))
  } catch {
    pluginNames.value = {}
  }
}

/** 技能来源：插件 > 内置 > 外部(兼容) > 用户。 */
function skillSource(skill: SkillDef): { label: string; kind: string; title: string } {
  if (skill.pluginId) {
    const name = pluginNames.value[skill.pluginId] ?? skill.pluginId
    return {
      label: t('skills.sourceLabels.pluginNamed', { name }),
      kind: 'plugin',
      title: t('skills.sourceTitles.plugin', { pluginId: skill.pluginId })
    }
  }
  if (skill.builtin || skill.provenance === 'system') {
    return { label: t('skills.sourceLabels.builtin'), kind: 'system', title: t('skills.sourceTitles.builtin') }
  }
  if (skill.provenance === 'external') {
    return { label: t('skills.sourceLabels.external'), kind: 'external', title: t('skills.sourceTitles.external') }
  }
  return { label: t('skills.sourceLabels.user'), kind: 'user', title: t('skills.sourceTitles.user') }
}

function sourceClass(kind: string): string {
  switch (kind) {
    case 'plugin':
      return 'bg-accent/15 text-accent'
    case 'system':
      return 'bg-[hsl(var(--code-bg))] text-muted'
    case 'external':
      return 'bg-[hsl(var(--code-bg))] text-foreground/80'
    default:
      return 'bg-[hsl(var(--code-bg))] text-accent'
  }
}

async function refreshSkills() {
  loading.value = true
  try {
    await skills.load({ rescan: true })
  } finally {
    loading.value = false
  }
}

const filtered = computed(() => {
  const keyword = q.value.trim().toLowerCase()
  return skills.skills.filter(skill => {
    if (sourceFilter.value !== 'all' && skillSource(skill).kind !== sourceFilter.value) return false
    if (!keyword) return true
    return (
      skill.name.toLowerCase().includes(keyword) ||
      skill.description.toLowerCase().includes(keyword) ||
      skill.tags.join(' ').toLowerCase().includes(keyword)
    )
  })
})

/** 来源筛选 chips 配置（全部 + 四类来源）。 */
const SOURCE_FILTERS = [
  { id: 'all', label: () => t('skills.filters.all') },
  { id: 'user', label: () => t('skills.filters.user') },
  { id: 'system', label: () => t('skills.filters.system') },
  { id: 'external', label: () => t('skills.filters.external') },
  { id: 'plugin', label: () => t('skills.filters.plugin') }
] as const

function skillEnabled(skillId: string): boolean {
  return skills.enabledIdsForAgent(currentAgentId.value).includes(skillId)
}

const enabledCount = computed(() => filtered.value.filter(skill => skillEnabled(skill.id)).length)

/** 统计某个来源（含 all）的技能数，供筛选 chips 显示。 */
function sourceCount(id: string): number {
  if (id === 'all') return skills.skills.length
  return skills.skills.filter(s => skillSource(s).kind === id).length
}

/** 插件技能只能由插件 enable/disable 生命周期管理，UI 不可手动切换。 */
function isPluginSkill(skill: SkillDef): boolean {
  return !!skill.pluginId
}

function toggleSkill(skill: SkillDef) {
  if (isPluginSkill(skill)) return
  skills.toggleForAgent(currentAgentId.value, skill.id)
}

async function onImportFile(event: Event) {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return

  importing.value = true
  importMessage.value = ''
  try {
    const result = await skills.importZip(file)
    const names = result.imported.map(skill => skill.name).join('、')
    importMessage.value = result.imported.length
      ? `已导入并启用 ${result.imported.length} 个技能：${names}`
      : '未导入任何技能'
    if (result.skipped.length) {
      importMessage.value += `；跳过 ${result.skipped.length} 项`
      const detail = result.skipped.slice(0, 3).join('；')
      if (detail) importMessage.value += `（${detail}${result.skipped.length > 3 ? '…' : ''}）`
    }
  } catch (err: unknown) {
    importMessage.value = err instanceof Error ? err.message : String(err)
  } finally {
    importing.value = false
    input.value = ''
  }
}
</script>

<template>
  <div class="space-y-5">
    <div class="flex items-start justify-end gap-4">
      <input ref="fileInput" type="file" accept=".zip,application/zip" class="hidden" @change="onImportFile" />
      <button
        type="button"
        class="h-9 shrink-0 rounded-lg border border-border bg-card px-3 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors disabled:opacity-50"
        :disabled="importing"
        @click="fileInput?.click()"
      >
        <span class="inline-flex items-center gap-1.5">
          <Upload class="h-3.5 w-3.5 text-accent" aria-hidden="true" />
          {{ importing ? '导入中…' : '导入 zip' }}
        </span>
      </button>
    </div>

    <div
      v-if="importMessage"
      class="rounded-xl border border-border bg-accent-muted/50 px-3 py-2 text-sm text-foreground"
    >
      {{ importMessage }}
    </div>

    <div class="rounded-2xl border border-border panel p-5">
      <div class="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
        <div class="w-full sm:max-w-md">
          <p class="mb-2 text-xs font-medium uppercase tracking-wide text-muted">应用范围</p>
          <div class="flex items-center gap-1 rounded-xl bg-[hsl(var(--code-bg))] p-0.5">
            <button
              v-for="tab in AGENT_TABS"
              :key="tab.id"
              type="button"
              class="h-9 flex-1 rounded-lg text-sm font-medium transition-colors"
              :class="selectedAgentId === tab.id
                ? 'bg-card text-foreground shadow-sm'
                : 'text-muted hover:text-foreground'"
              @click="selectedAgentId = tab.id"
            >
              {{ tab.label }}
            </button>
          </div>
        </div>
        <p class="text-sm text-muted tabular-nums">已启用 {{ enabledCount }} / {{ filtered.length }}</p>
      </div>

      <div class="mt-3 flex flex-wrap items-center gap-2">
        <button
          v-for="f in SOURCE_FILTERS"
          :key="f.id"
          type="button"
          class="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] transition-colors cursor-pointer"
          :class="sourceFilter === f.id
            ? 'bg-accent/15 text-accent'
            : 'text-muted hover:bg-hover hover:text-foreground'"
          @click="sourceFilter = f.id"
        >
          {{ f.label }}
          <span class="tabular-nums">{{ sourceCount(f.id) }}</span>
        </button>
      </div>

      <div class="mt-4 flex h-10 items-center gap-2 rounded-xl border border-border bg-card px-3">
        <Search class="h-4 w-4 shrink-0 text-muted" aria-hidden="true" />
        <input
          v-model="q"
          type="text"
          placeholder="搜索技能名、说明或标签"
          class="flex-1 border-0 bg-transparent text-sm text-foreground outline-none placeholder:text-muted"
        />
      </div>
    </div>

    <div v-if="loading" class="py-12 text-center text-sm text-muted">正在刷新技能列表…</div>
    <div v-else class="grid grid-cols-1 gap-3 xl:grid-cols-2">
      <button
        v-for="skill in filtered"
        :key="skill.id"
        type="button"
        :aria-disabled="isPluginSkill(skill)"
        :title="isPluginSkill(skill) ? '由插件管理，无法手动切换' : undefined"
        class="rounded-xl border p-4 text-left transition-all"
        :class="[
          isPluginSkill(skill) ? 'cursor-default' : 'cursor-pointer',
          skillEnabled(skill.id)
            ? 'border-accent/30 bg-accent/5'
            : 'border-border bg-card hover:bg-hover'
        ]"
        @click="toggleSkill(skill)"
      >
        <div class="flex items-center gap-2 min-w-0">
          <span class="truncate text-[15px] font-semibold text-foreground">{{ skill.name }}</span>
          <span
            class="shrink-0 rounded px-1.5 py-0.5 text-[10px]"
            :class="sourceClass(skillSource(skill).kind)"
            :title="skillSource(skill).title"
          >
            {{ skillSource(skill).label }}
          </span>
          <span
            class="ml-auto shrink-0 rounded-full px-2 py-0.5 text-[10px]"
            :class="skillEnabled(skill.id) ? 'bg-accent/15 text-accent' : 'bg-[hsl(var(--code-bg))] text-muted'"
          >
            {{ skillEnabled(skill.id) ? '已启用' : '未启用' }}
          </span>
        </div>
        <p class="mt-1.5 line-clamp-3 text-[12px] leading-5 text-muted">{{ skill.description }}</p>
        <div v-if="skill.tags.length" class="mt-2 flex flex-wrap gap-1.5">
          <span
            v-for="tag in skill.tags"
            :key="tag"
            class="rounded bg-[hsl(var(--code-bg))] px-1.5 py-0.5 text-[10px] text-foreground/80"
          >
            {{ tag }}
          </span>
        </div>
        <div v-if="skill.toolNames.length" class="mt-2 flex min-w-0 items-center gap-1 text-[11px] text-muted">
          <Wrench class="h-3 w-3 shrink-0 text-accent" aria-hidden="true" />
          <span class="truncate">{{ skill.toolNames.join(' · ') }}</span>
        </div>
      </button>
      <div v-if="!filtered.length" class="py-12 text-center text-sm text-muted xl:col-span-2">
        没有匹配的技能
      </div>
    </div>
  </div>
</template>
