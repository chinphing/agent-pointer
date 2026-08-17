<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { Sparkles, Search, Wrench, Upload } from 'lucide-vue-next'
import { useSkillsStore } from '../../stores/skills'

const AGENT_TABS = [
  { id: 'general', label: '通用助手' },
  { id: 'coder', label: '氛围编程' },
  { id: 'computer', label: '电脑操控' }
] as const

const skills = useSkillsStore()
const q = ref('')
const importing = ref(false)
const loading = ref(false)
const importMessage = ref('')
const fileInput = ref<HTMLInputElement | null>(null)
const selectedAgentId = ref<string>('general')

const currentAgentId = computed(() => selectedAgentId.value)

onMounted(() => {
  void refreshSkills()
})

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
  if (!keyword) return skills.skills
  return skills.skills.filter(skill =>
    skill.name.toLowerCase().includes(keyword) ||
    skill.description.toLowerCase().includes(keyword) ||
    skill.tags.join(' ').toLowerCase().includes(keyword)
  )
})

function skillEnabled(skillId: string): boolean {
  return skills.enabledIdsForAgent(currentAgentId.value).includes(skillId)
}

const enabledCount = computed(() => filtered.value.filter(skill => skillEnabled(skill.id)).length)

function toggleSkill(skillId: string) {
  skills.toggleForAgent(currentAgentId.value, skillId)
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
    <div class="flex items-start justify-between gap-4">
      <div>
        <h3 class="flex items-center gap-2 text-lg font-semibold tracking-tight text-foreground">
          <Sparkles class="h-5 w-5 text-accent" aria-hidden="true" />
          技能
        </h3>
        <p class="mt-1 text-sm text-muted">为不同智能体启用技能，扩展可用工具与工作流程</p>
      </div>
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
        class="rounded-xl border p-4 text-left transition-all cursor-pointer"
        :class="skillEnabled(skill.id)
          ? 'border-accent/30 bg-accent/5'
          : 'border-border bg-card hover:bg-hover'"
        @click="toggleSkill(skill.id)"
      >
        <div class="flex items-center gap-2 min-w-0">
          <span class="truncate text-[15px] font-semibold text-foreground">{{ skill.name }}</span>
          <span class="shrink-0 rounded bg-[hsl(var(--code-bg))] px-1.5 py-0.5 text-[10px] text-muted">
            {{ skill.builtin ? '内置' : '外部' }}
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
