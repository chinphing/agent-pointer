<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { X, Sparkles, Search, Wrench, Upload } from 'lucide-vue-next'
import { useSkillsStore } from '../../stores/skills'

defineEmits<{ (e: 'close'): void }>()

const AGENT_TABS = [
  { id: 'general', label: '通用助手' },
  { id: 'coder', label: '氛围编程' },
  { id: 'computer', label: '电脑操控' },
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
  const k = q.value.trim().toLowerCase()
  const list = skills.skills
  const filteredList = k
    ? list.filter(s =>
        s.name.toLowerCase().includes(k) ||
        s.description.toLowerCase().includes(k) ||
        s.tags.join(' ').toLowerCase().includes(k)
      )
    : list
  return filteredList
})

function skillEnabled(skillId: string): boolean {
  return skills.enabledIdsForAgent(currentAgentId.value).includes(skillId)
}

const enabledCount = computed(() => filtered.value.filter(s => skillEnabled(s.id)).length)

function toggleSkill(skillId: string) {
  skills.toggleForAgent(currentAgentId.value, skillId)
}

async function onImportFile(e: Event) {
  const input = e.target as HTMLInputElement
  const file = input.files?.[0]
  if (!file) return
  importing.value = true
  importMessage.value = ''
  try {
    const result = await skills.importZip(file)
    const names = result.imported.map(s => s.name).join('、')
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
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
    @click.self="$emit('close')"
  >
    <div class="w-[720px] max-w-[92vw] max-h-[80vh] glass-strong rounded-2xl border border-border shadow-2xl flex flex-col overflow-hidden">
      <header class="px-5 h-14 flex items-center gap-3 border-b border-border shrink-0">
        <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
          <Sparkles class="w-4 h-4 text-accent" />
        </div>
        <div class="min-w-0">
          <h2 class="text-base font-semibold text-foreground">技能库</h2>
          <p class="text-[11px] text-muted truncate">启用技能可扩展 AI 的能力</p>
        </div>
        <div class="flex-1" />
        <input ref="fileInput" type="file" accept=".zip,application/zip" class="hidden" @change="onImportFile" />
        <button
          type="button"
          class="h-8 px-3 rounded-lg border border-border bg-card hover:bg-hover cursor-pointer text-xs text-foreground flex items-center gap-1.5 transition-colors disabled:opacity-50"
          :disabled="importing"
          @click="fileInput?.click()"
        >
          <Upload class="w-3.5 h-3.5 text-accent" />
          {{ importing ? '导入中…' : '导入 zip' }}
        </button>
        <button
          type="button"
          class="p-2 rounded-lg hover:bg-hover cursor-pointer transition-colors"
          @click="$emit('close')"
        >
          <X class="w-4 h-4 text-muted" />
        </button>
      </header>

      <div class="px-5 pt-3 shrink-0">
        <div
          v-if="importMessage"
          class="mb-3 rounded-xl border border-border bg-accent-muted/50 px-3 py-2 text-xs text-foreground"
        >
          {{ importMessage }}
        </div>
        <!-- Agent tabs -->
        <div class="flex items-center gap-1 mb-3 p-0.5 rounded-xl bg-[hsl(var(--code-bg))]">
          <button
            v-for="tab in AGENT_TABS"
            :key="tab.id"
            type="button"
            class="flex-1 h-8 rounded-lg text-xs font-medium transition-colors"
            :class="selectedAgentId === tab.id
              ? 'bg-card text-foreground shadow-sm'
              : 'text-muted hover:text-foreground'"
            @click="selectedAgentId = tab.id"
          >{{ tab.label }}</button>
        </div>
        <div class="flex items-center gap-2 mb-3 text-[11px] text-muted">
          <span>已启用 {{ enabledCount }} / {{ filtered.length }}</span>
        </div>
        <div class="flex items-center gap-2 h-10 px-3 rounded-xl border border-border bg-card">
          <Search class="w-4 h-4 text-muted shrink-0" />
          <input
            v-model="q"
            type="text"
            placeholder="搜索技能名、说明或标签"
            class="flex-1 bg-transparent border-0 outline-none text-sm text-foreground placeholder:text-muted"
          />
        </div>
      </div>

      <div class="flex-1 overflow-y-auto p-5 grid grid-cols-1 md:grid-cols-2 gap-3 min-h-0">
        <div v-if="loading" class="col-span-full text-center text-muted py-12 text-sm">
          正在刷新技能列表…
        </div>
        <template v-else>
          <div
            v-for="s in filtered"
            :key="s.id"
            class="rounded-xl p-4 border transition-all cursor-pointer"
            :class="skillEnabled(s.id)
              ? 'border-accent/30 bg-accent/5 shadow-sm'
              : 'border-border bg-hover/40 hover:border-border hover:bg-hover/60'"
            @click="toggleSkill(s.id)"
          >
            <div class="flex items-center gap-2 min-w-0">
              <div class="text-[15px] font-semibold text-foreground truncate">{{ s.name }}</div>
              <span class="text-[10px] px-1.5 py-0.5 rounded bg-[hsl(var(--code-bg))] text-muted shrink-0">
                {{ s.builtin ? '内置' : '外部' }}
              </span>
              <span
                v-if="skillEnabled(s.id)"
                class="ml-auto text-[10px] px-2 py-0.5 rounded-full bg-accent/15 text-accent shrink-0"
              >已启用</span>
              <span
                v-else
                class="ml-auto text-[10px] px-2 py-0.5 rounded-full bg-[hsl(var(--code-bg))] text-muted shrink-0"
              >未启用</span>
            </div>
            <p class="mt-1.5 text-[12px] text-muted leading-5 line-clamp-3">{{ s.description }}</p>
            <div v-if="s.tags.length" class="mt-2 flex flex-wrap gap-1.5">
              <span
                v-for="t in s.tags"
                :key="t"
                class="text-[10px] px-1.5 py-0.5 rounded bg-[hsl(var(--code-bg))] text-foreground/80"
              >{{ t }}</span>
            </div>
            <div v-if="s.toolNames.length" class="mt-2 flex items-center gap-1 text-[11px] text-muted min-w-0">
              <Wrench class="w-3 h-3 text-accent shrink-0" />
              <span class="truncate">{{ s.toolNames.join(' · ') }}</span>
            </div>
            <div v-if="s.resourceFiles.length" class="mt-1 text-[11px] text-muted">
              资源文件：{{ s.resourceFiles.length }} 个，按需读取
            </div>
          </div>
          <div v-if="!filtered.length" class="col-span-full text-center text-muted py-12 text-sm">
            没有匹配的技能
          </div>
        </template>
      </div>
    </div>
  </div>
</template>
