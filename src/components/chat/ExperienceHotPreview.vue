<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  formatExperienceCardExcerpt,
  getExperienceDetail,
  listExperienceHome,
  truncateExperienceCardTitle,
} from '../../lib/experiences'
import type { ExperienceListItem } from '../../types/experience'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'

const HOT_LIMIT = 5

const chat = useChatStore()
const platformAuth = usePlatformAuthStore()
const items = ref<ExperienceListItem[]>([])
const loadingSlug = ref<string | null>(null)

const visible = computed(() => items.value.length > 0)

onMounted(async () => {
  if (platformAuth.isStandalone) return
  try {
    const home = await listExperienceHome()
    items.value = (home.featured ?? []).slice(0, HOT_LIMIT)
  } catch (e) {
    console.warn('experience hot preview load failed', e)
  }
})

function resolveAgentId(raw: string | undefined | null): string {
  const id = (raw ?? 'general').trim()
  if (id === 'coder' || id === 'computer') return id
  return 'general'
}

function chipHint(item: ExperienceListItem): string {
  return formatExperienceCardExcerpt(item.excerpt)
}

async function onSelect(item: ExperienceListItem) {
  if (loadingSlug.value) return
  loadingSlug.value = item.slug
  try {
    const detail = await getExperienceDetail(item.slug)
    const agentId = resolveAgentId(detail.agent_id ?? item.agent_id)
    if (!chat.current) chat.newConversation()
    chat.setConversationAgent(agentId, 'single')
    const prompt = detail.prompt_text?.trim()
    if (prompt) {
      chat.prefillComposer(prompt)
    }
  } catch (e) {
    console.warn('experience detail load failed', e)
    chat.showUiToast('经验加载失败，请稍后重试', 'warning')
  } finally {
    loadingSlug.value = null
  }
}
</script>

<template>
  <div v-if="visible" class="flex flex-wrap gap-2">
    <span
      v-for="item in items"
      :key="item.id"
      class="group/chip relative inline-flex max-w-full"
    >
      <button
        type="button"
        class="inline-flex max-w-full rounded-full border border-border/50 bg-accent/5 px-3 py-1.5 text-left text-xs font-medium text-muted transition-colors hover:border-border/70 hover:bg-accent/10 hover:text-foreground/80 disabled:cursor-wait disabled:opacity-60"
        :disabled="loadingSlug === item.slug"
        :aria-describedby="chipHint(item) ? `experience-hot-hint-${item.id}` : undefined"
        @click="onSelect(item)"
      >
        <span class="min-w-0 truncate">{{ truncateExperienceCardTitle(item.title) }}</span>
      </button>
      <span
        v-if="chipHint(item).trim()"
        :id="`experience-hot-hint-${item.id}`"
        role="tooltip"
        class="experience-hot-hint pointer-events-none absolute bottom-[calc(100%+6px)] left-1/2 z-20 w-max max-w-[16rem] -translate-x-1/2 rounded-lg border border-border/60 bg-card px-2.5 py-1.5 text-[11px] font-normal leading-snug text-muted opacity-0 shadow-sm transition-opacity duration-150 group-hover/chip:opacity-100"
      >
        {{ chipHint(item) }}
      </span>
    </span>
  </div>
</template>

<style scoped>
.experience-hot-hint {
  box-shadow: 0 4px 12px hsl(var(--foreground) / 0.06);
}
</style>
