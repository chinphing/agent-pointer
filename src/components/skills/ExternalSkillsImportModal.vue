<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { Download, X } from 'lucide-vue-next'
import type { ExternalSkillSource } from '../../types/chat'


const { t } = useI18n()
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
      class="fixed inset-0 z-[220] flex items-center justify-center bg-foreground/32 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
      :aria-label="t('skills.discoverTitle')"
      @click.self="onDismiss"
    >
      <div class="relative w-full max-w-lg rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl overflow-hidden">
        <header class="flex items-start gap-3 px-5 pt-5 pb-3 border-b border-border">
          <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
            <Download class="w-4 h-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1 pr-8">
            <h2 class="text-base font-semibold text-foreground">{{ t('skills.discoverTitle') }}</h2>
            <p class="text-[13px] text-muted mt-1 leading-relaxed">
              {{ t('skills.discoverDescription', { count: totalSkills }) }}
              <code class="text-xs">~/.pointer/skills</code>{{ t('skills.importNote') }}
            </p>
          </div>
          <button
            type="button"
            class="absolute top-4 right-4 p-2 rounded-lg hover:bg-hover text-muted cursor-pointer transition-colors"
            :title="t('workspace.filePreview.closeFindTitle')"
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
            {{ t('skills.selectAll') }}
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
              <div class="text-xs text-muted mt-1">{{ t('skills.skillCountSuffix', { count: source.skillCount }) }}</div>
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
            {{ t('skills.notNow') }}
          </button>
          <button
            type="button"
            class="h-9 px-4 rounded-lg bg-accent text-accent-foreground text-sm font-medium cursor-pointer hover:opacity-95 transition-opacity disabled:opacity-50"
            :disabled="importing || selected.size === 0"
            @click="onImport"
          >
            {{ importing ? t('skills.importing') : t('skills.importOneClick') }}
          </button>
        </footer>
      </div>
    </div>
  </Teleport>
</template>
