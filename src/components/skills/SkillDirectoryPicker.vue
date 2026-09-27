<script setup lang="ts">
import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { FolderOpen, Search } from 'lucide-vue-next'
import { useSkillsStore } from '../../stores/skills'
import { useChatStore } from '../../stores/chat'
import { userSkillDirectoryCandidates } from '../../lib/skillDirectories'

export interface SkillDirectoryOption {
  name: string
  path: string
  description?: string
}

const props = withDefaults(
  defineProps<{
    /** Group title; shown on the same row as the search box. */
    title?: string
    /** grid: icon cards (new project dialog); list: compact (Composer dropdown). */
    variant?: 'grid' | 'list'
    searchable?: boolean
    disabled?: boolean
  }>(),
  { variant: 'grid', searchable: true, disabled: false }
)

const emit = defineEmits<{ (e: 'select', dir: SkillDirectoryOption): void }>()

const { t } = useI18n()
const skillsStore = useSkillsStore()
const chat = useChatStore()
const query = ref('')

const candidates = computed(() => {
  const base = userSkillDirectoryCandidates(skillsStore.skills, chat.projects)
  const keyword = query.value.trim().toLocaleLowerCase()
  if (!keyword) return base
  return base.filter(
    dir =>
      dir.name.toLocaleLowerCase().includes(keyword) ||
      dir.path.toLocaleLowerCase().includes(keyword)
  )
})

function descriptionFor(path: string): string | undefined {
  return skillsStore.skills.find(s => s.source?.trim() === path)?.description
}
</script>

<template>
  <div class="min-w-0">
    <div class="flex items-center gap-2">
      <span v-if="props.title" class="text-[10px] text-muted font-medium whitespace-nowrap">{{ props.title }}</span>
      <div v-if="props.searchable" class="relative min-w-0 flex-1">
        <Search class="pointer-events-none absolute left-2 top-1/2 h-3 w-3 -translate-y-1/2 text-muted" />
        <input
          v-model="query"
          type="text"
          :placeholder="t('skills.searchDirectory')"
          :aria-label="t('skills.searchDirectory')"
          class="w-full rounded-lg border border-border bg-transparent py-1 pl-6 pr-2 text-xs text-foreground outline-none transition-colors placeholder:text-muted focus:border-accent/60"
        >
      </div>
    </div>

    <div
      v-if="variant === 'grid'"
      class="mt-2 grid max-h-56 gap-1.5 overflow-y-auto pr-0.5"
      style="grid-template-columns: repeat(2, minmax(0, 1fr))"
    >
      <button
        v-for="dir in candidates"
        :key="dir.path"
        type="button"
        class="flex min-w-0 flex-col gap-1 overflow-hidden rounded-xl border border-border bg-card p-2.5 text-left transition-colors hover:border-accent/50 hover:bg-hover cursor-pointer disabled:cursor-not-allowed disabled:opacity-60"
        :title="dir.path"
        :disabled="props.disabled"
        @click="emit('select', dir)"
      >
        <span class="flex min-w-0 items-center gap-1.5 text-xs font-medium text-foreground">
          <FolderOpen class="h-3.5 w-3.5 shrink-0 text-accent" />
          <span class="min-w-0 truncate">{{ dir.name }}</span>
        </span>
        <span
          v-if="descriptionFor(dir.path)"
          :title="descriptionFor(dir.path)"
          class="block min-w-0 truncate text-[10px] leading-relaxed text-muted"
        >
          {{ descriptionFor(dir.path) }}
        </span>
      </button>
      <div v-if="candidates.length === 0" class="col-span-2 px-1 py-3 text-center text-[11px] text-muted">
        {{ t('skills.noMatchDirectory') }}
      </div>
    </div>

    <div v-else class="mt-1 max-h-40 space-y-0.5 overflow-y-auto">
      <button
        v-for="dir in candidates"
        :key="dir.path"
        type="button"
        class="composer-dropdown-item composer-dropdown-item--compact cursor-pointer"
        :title="dir.path"
        :disabled="props.disabled"
        @click="emit('select', dir)"
      >
        <FolderOpen class="w-3 h-3 shrink-0" />
        <span class="flex-1 truncate">{{ dir.name }}</span>
      </button>
      <div v-if="candidates.length === 0" class="px-3 py-2 text-center text-[11px] text-muted">
        {{ t('skills.noMatchDirectory') }}
      </div>
    </div>
  </div>
</template>
