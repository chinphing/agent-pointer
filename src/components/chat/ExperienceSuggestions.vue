<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  formatExperienceCardExcerpt,
  listPinnedExperiences,
  getExperienceDetail,
} from '../../lib/experiences'
import type { ExperienceListItem } from '../../types/experience'
import { useChatStore } from '../../stores/chat'

const chat = useChatStore()
const items = ref<ExperienceListItem[]>([])
const loadingSlug = ref<string | null>(null)

const visible = computed(() => items.value.length > 0)

const CARD_TONES = [
  'experience-card--sky',
  'experience-card--rose',
  'experience-card--sand',
] as const

onMounted(async () => {
  try {
    items.value = await listPinnedExperiences(3)
  } catch (e) {
    console.warn('experience suggestions load failed', e)
  }
})

function cardTone(index: number): string {
  return CARD_TONES[index % CARD_TONES.length]
}

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
    chat.showUiToast('经验加载失败，请稍后重试', 'warning')
  } finally {
    loadingSlug.value = null
  }
}
</script>

<template>
  <section v-if="visible" class="w-full">
    <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
      <button
        v-for="(item, index) in items"
        :key="item.id"
        type="button"
        class="experience-card group text-left transition-all disabled:cursor-wait disabled:opacity-60"
        :class="cardTone(index)"
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
  @apply flex h-full flex-col rounded-2xl px-4 py-5;
}

.experience-card__title {
  @apply shrink-0 text-sm font-semibold leading-[1.375] text-foreground line-clamp-2;
  height: 2.75rem;
}

.experience-card__body {
  @apply mt-2.5 min-h-[3.75rem] text-xs leading-relaxed text-muted;
}

.experience-card--sky {
  background: #eef4ff;
}
.experience-card--rose {
  background: #fff0f3;
}
.experience-card--sand {
  background: #fff8eb;
}

.experience-card:hover {
  filter: brightness(0.98);
  transform: translateY(-1px);
}

html.dark .experience-card--sky {
  background: hsl(220 60% 18% / 0.55);
}
html.dark .experience-card--rose {
  background: hsl(350 45% 18% / 0.55);
}
html.dark .experience-card--sand {
  background: hsl(38 45% 16% / 0.55);
}

html.dark .experience-card:hover {
  filter: brightness(1.08);
}
</style>
