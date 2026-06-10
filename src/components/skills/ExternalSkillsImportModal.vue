<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Download, X } from 'lucide-vue-next'
import type { ExternalSkillSource } from '../../types/chat'

const open = defineModel<boolean>('open', { required: true })

const props = defineProps<{
  sources: ExternalSkillSource[]
  totalSkills: number
  importing?: boolean
}>()

const emit = defineEmits<{
  (e: 'import', sourceIds: string[]): void
  (e: 'dismiss'): void
}>()

const selected = ref<Set<string>>(new Set())

watch(
  () => props.sources,
  sources => {
    selected.value = new Set(sources.map(s => s.id))
  },
  { immediate: true }
)

const allSelected = computed(() =>
  props.sources.length > 0 && props.sources.every(s => selected.value.has(s.id))
)

function toggle(id: string) {
  const next = new Set(selected.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  selected.value = next
}

function toggleAll() {
  if (allSelected.value) {
    selected.value = new Set()
  } else {
    selected.value = new Set(props.sources.map(s => s.id))
  }
}

function onImport() {
  emit('import', [...selected.value])
}

function onDismiss() {
  emit('dismiss')
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') onDismiss()
}

watch(
  open,
  v => {
    if (v) document.addEventListener('keydown', onKeydown)
    else document.removeEventListener('keydown', onKeydown)
  },
  { immediate: true }
)
</script>

<template>
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[220] flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      aria-label="导入外部 Skills"
      @click.self="onDismiss"
    >
      <div class="relative w-full max-w-lg rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl overflow-hidden">
        <header class="flex items-start gap-3 px-5 pt-5 pb-3 border-b border-border">
          <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
            <Download class="w-4 h-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1 pr-8">
            <h2 class="text-base font-semibold text-foreground">发现外部 Skills</h2>
            <p class="text-[13px] text-muted mt-1 leading-relaxed">
              检测到本机其他 Agent 的 Skills 目录，共 {{ totalSkills }} 个可导入项。导入后将保存到
              <code class="text-xs">~/.pointer/skills</code>，系统内置 Skills 仍保留在应用数据目录且不可修改。
            </p>
          </div>
          <button
            type="button"
            class="absolute top-4 right-4 p-2 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors"
            title="关闭 (Esc)"
            @click="onDismiss"
          >
            <X class="w-4 h-4" />
          </button>
        </header>

        <div class="px-5 py-4 max-h-[50vh] overflow-y-auto space-y-2">
          <label class="flex items-center gap-2 text-sm text-foreground cursor-pointer">
            <input
              type="checkbox"
              class="rounded border-border"
              :checked="allSelected"
              @change="toggleAll"
            />
            全选
          </label>
          <label
            v-for="source in sources"
            :key="source.id"
            class="flex items-start gap-3 p-3 rounded-xl border border-border hover:bg-hover/40 cursor-pointer"
          >
            <input
              type="checkbox"
              class="mt-1 rounded border-border"
              :checked="selected.has(source.id)"
              @change="toggle(source.id)"
            />
            <div class="min-w-0 flex-1">
              <div class="text-sm font-medium text-foreground">{{ source.label }}</div>
              <div class="text-xs text-muted mt-0.5 truncate">{{ source.path }}</div>
              <div class="text-xs text-muted mt-1">{{ source.skillCount }} 个 Skill 可导入</div>
            </div>
          </label>
        </div>

        <footer class="flex justify-end gap-2 px-5 py-4 border-t border-border">
          <button
            type="button"
            class="h-9 px-4 rounded-lg bg-hover hover:opacity-90 text-sm text-foreground cursor-pointer transition-opacity disabled:opacity-50"
            :disabled="importing"
            @click="onDismiss"
          >
            暂不导入
          </button>
          <button
            type="button"
            class="h-9 px-4 rounded-lg bg-accent text-white text-sm font-medium cursor-pointer hover:opacity-95 transition-opacity disabled:opacity-50"
            :disabled="importing || selected.size === 0"
            @click="onImport"
          >
            {{ importing ? '导入中…' : '一键导入' }}
          </button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>
