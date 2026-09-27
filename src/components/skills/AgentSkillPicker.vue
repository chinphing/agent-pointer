<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { RotateCcw, Search, Sparkles, Wrench, X } from 'lucide-vue-next'
import { useSkillsStore } from '../../stores/skills'
import type { AgentDef } from '../../types/chat'

const { t } = useI18n()

const props = defineProps<{
  agent: AgentDef
  agentName: string
}>()

defineEmits<{ (e: 'close'): void }>()

const skillsStore = useSkillsStore()
const query = ref('')
const loading = ref(false)
const savingId = ref<string | null>(null)

const availableSkills = computed(() => skillsStore.skills)
const enabledIds = computed(() => new Set(skillsStore.enabledIdsForAgent(props.agent.id)))
const enabledCount = computed(() =>
  availableSkills.value.filter(skill => enabledIds.value.has(skill.id)).length
)
const filteredSkills = computed(() => {
  const keyword = query.value.trim().toLowerCase()
  if (!keyword) return availableSkills.value
  return availableSkills.value.filter(skill =>
    skill.name.toLowerCase().includes(keyword) ||
    skill.description.toLowerCase().includes(keyword) ||
    skill.tags.join(' ').toLowerCase().includes(keyword)
  )
})

onMounted(async () => {
  if (skillsStore.loaded) return
  loading.value = true
  try {
    await skillsStore.load()
  } finally {
    loading.value = false
  }
})

async function toggleSkill(skillId: string) {
  if (savingId.value) return
  savingId.value = skillId
  try {
    await skillsStore.toggleForAgent(props.agent.id, skillId)
  } finally {
    savingId.value = null
  }
}

async function resetOverride() {
  if (savingId.value) return
  savingId.value = '__reset__'
  try {
    await skillsStore.resetAgentOverride(props.agent.id)
  } finally {
    savingId.value = null
  }
}
</script>

<template>
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-foreground/32 backdrop-blur-sm"
    @click.self="$emit('close')"
  >
    <div class="w-[720px] max-w-[92vw] max-h-[80vh] glass-strong rounded-xl border border-border shadow-2xl flex flex-col overflow-hidden">
      <header class="px-5 h-14 flex items-center gap-3 border-b border-border shrink-0">
        <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center shrink-0">
          <Sparkles class="w-4 h-4 text-accent" />
        </div>
        <div class="min-w-0">
          <h2 class="text-base font-semibold text-foreground truncate">{{ t('skills.configureFor', { agent: agentName }) }}</h2>
          <p class="text-[11px] text-muted">
            {{ t('skills.enabledCount', { enabled: enabledCount, total: availableSkills.length }) }}
            <span v-if="!skillsStore.hasAgentOverride(agent.id)"> {{ t('skills.inheritsGlobal') }}</span>
          </p>
        </div>
        <div class="flex-1" />
        <button
          type="button"
          class="h-8 px-3 rounded-lg border border-border bg-card hover:bg-hover text-xs text-foreground flex items-center gap-1.5 transition-colors disabled:opacity-50"
          :disabled="!skillsStore.hasAgentOverride(agent.id) || Boolean(savingId)"
          @click="resetOverride"
        >
          <RotateCcw class="w-3.5 h-3.5" />
          {{ t('skills.resetToDefault') }}
        </button>
        <button
          type="button"
          class="p-2 rounded-lg hover:bg-hover transition-colors"
          :aria-label="t('skills.close')"
          @click="$emit('close')"
        >
          <X class="w-4 h-4 text-accent" />
        </button>
      </header>

      <div class="px-5 pt-3 shrink-0">
        <div class="flex items-center gap-2 h-10 px-3 rounded-lg border border-border bg-card">
          <Search class="w-4 h-4 text-accent shrink-0" />
          <input
            v-model="query"
            type="text"
            :placeholder="t('skills.searchPlaceholder')"
            class="flex-1 bg-transparent border-0 outline-none text-sm text-foreground placeholder:text-muted"
          />
        </div>
      </div>

      <div class="flex-1 overflow-y-auto p-5 grid grid-cols-1 md:grid-cols-2 gap-3 min-h-0">
        <div v-if="loading" class="col-span-full text-center text-muted py-12 text-sm">
          {{ t('skills.loadingList') }}
        </div>
        <template v-else>
          <button
            v-for="skill in filteredSkills"
            :key="skill.id"
            type="button"
            class="text-left rounded-lg p-4 border transition-colors disabled:opacity-60"
            :class="enabledIds.has(skill.id)
              ? 'border-accent/30 bg-accent/5'
              : 'border-border bg-card hover:bg-hover'"
            :disabled="Boolean(savingId)"
            @click="toggleSkill(skill.id)"
          >
            <div class="flex items-center gap-2 min-w-0">
              <span class="text-[14px] font-semibold text-foreground truncate">{{ skill.name }}</span>
              <span class="text-[10px] px-1.5 py-0.5 rounded bg-[hsl(var(--code-bg))] text-muted shrink-0">
                {{ skill.builtin ? t('skills.builtinShort') : t('skills.externalShort') }}
              </span>
              <span class="ml-auto inline-flex items-center gap-2 shrink-0">
                <span class="text-[10px]" :class="enabledIds.has(skill.id) ? 'text-accent' : 'text-muted'">{{ enabledIds.has(skill.id) ? t('skills.enabled') : t('skills.disabled') }}</span>
                <span
                  class="relative w-8 h-[18px] rounded-full transition-colors"
                  :class="enabledIds.has(skill.id) ? 'bg-accent' : 'bg-[hsl(var(--code-bg))]'"
                >
                  <span
                    class="absolute top-0.5 w-3.5 h-3.5 rounded-full bg-white transition-transform"
                    :class="enabledIds.has(skill.id) ? 'translate-x-[16px]' : 'translate-x-0.5'"
                  />
                </span>
              </span>
            </div>
            <p class="mt-1.5 text-[12px] text-muted leading-5 line-clamp-3">{{ skill.description }}</p>
            <div v-if="skill.tags.length" class="mt-2 flex flex-wrap gap-1.5">
              <span
                v-for="tag in skill.tags"
                :key="tag"
                class="text-[10px] px-1.5 py-0.5 rounded bg-[hsl(var(--code-bg))] text-foreground/80"
              >{{ tag }}</span>
            </div>
            <div v-if="skill.toolNames.length" class="mt-2 flex items-center gap-1 text-[11px] text-muted min-w-0">
              <Wrench class="w-3 h-3 text-accent shrink-0" />
              <span class="truncate">{{ skill.toolNames.join(' · ') }}</span>
            </div>
          </button>
          <div v-if="!filteredSkills.length" class="col-span-full text-center text-muted py-12 text-sm">
            {{ t('skills.noMatches') }}
          </div>
        </template>
      </div>
    </div>
  </div>
</template>
