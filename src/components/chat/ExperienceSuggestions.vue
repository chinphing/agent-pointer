<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import {
  formatExperienceCardExcerpt,
  listPinnedExperiences,
  getExperienceDetail,
} from '../../lib/experiences'
import type { ExperienceListItem } from '../../types/experience'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'

const { t } = useI18n()
const chat = useChatStore()
const platformAuth = usePlatformAuthStore()
const items = ref<ExperienceListItem[]>([])
const loadingSlug = ref<string | null>(null)

const visible = computed(() => items.value.length > 0)

onMounted(async () => {
  if (platformAuth.isStandalone) return
  try {
    items.value = await listPinnedExperiences(3)
  } catch (e) {
    console.warn('experience suggestions load failed', e)
  }
})

async function onSelect(item: ExperienceListItem) {
  if (loadingSlug.value) return
  loadingSlug.value = item.slug
  try {
    const detail = await getExperienceDetail(item.slug)
    const prompt = detail.prompt_text?.trim()
    if (prompt) {
      chat.prefillComposer(prompt)
    }
  } catch (e) {
    console.warn('experience detail load failed', e)
    chat.showUiToast(t('chat.experienceLoadFailedRetry'), 'warning')
  } finally {
    loadingSlug.value = null
  }
}
</script>

<template>
  <section v-if="visible" class="w-full">
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
      <button
        v-for="item in items"
        :key="item.id"
        type="button"
        class="experience-card group text-left transition-all disabled:cursor-wait disabled:opacity-60"
        :disabled="loadingSlug === item.slug"
        @click="onSelect(item)"
      >
        <div class="experience-card__title">
          {{ item.title }}
        </div>
        <div class="experience-card__body">
          <p class="line-clamp-3">
            {{ formatExperienceCardExcerpt(item.excerpt) }}
          </p>
        </div>
      </button>
    </div>
  </section>
</template>

<style scoped>
.experience-card {
  @apply flex h-full flex-col rounded-2xl border border-border bg-card px-4 py-5;
}

.experience-card__title {
  @apply shrink-0 text-sm font-semibold leading-[1.375] text-foreground line-clamp-2;
  height: 2.75rem;
}

.experience-card__body {
  @apply mt-2.5 min-h-[3.75rem] text-xs leading-relaxed text-muted;
}

.experience-card:hover {
  background: hsl(var(--hover));
}
</style>
