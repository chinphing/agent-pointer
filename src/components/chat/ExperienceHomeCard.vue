<script setup lang="ts">
import { MousePointerClick } from 'lucide-vue-next'
import { formatExperienceCardExcerpt, truncateExperienceCardTitle } from '../../lib/experiences'
import type { ExperienceListItem } from '../../types/experience'

defineProps<{
  item: ExperienceListItem
  tone: string
  disabled?: boolean
}>()

const emit = defineEmits<{
  select: [item: ExperienceListItem]
}>()
</script>

<template>
  <button
    type="button"
    class="experience-card group text-left transition-all disabled:cursor-wait disabled:opacity-60"
    :class="tone"
    :disabled="disabled"
    @click="emit('select', item)"
  >
    <div class="experience-card__title-bar">
      <div class="experience-card__title-wrap">
        <div class="experience-card__title">{{ truncateExperienceCardTitle(item.title) }}</div>
      </div>
      <img
        v-if="item.cover_image_url"
        :src="item.cover_image_url"
        alt=""
        class="experience-card__badge"
        loading="lazy"
      />
    </div>
    <p class="experience-card__body">{{ formatExperienceCardExcerpt(item.excerpt) }}</p>
    <span class="experience-card__hint" aria-hidden="true">
      点击使用
      <MousePointerClick class="h-3.5 w-3.5 shrink-0" />
    </span>
  </button>
</template>

<style scoped>
.experience-card {
  --experience-hint-gap: 3px;
  --experience-hint-row: 14px;
  @apply relative flex h-[8.5rem] w-full flex-col rounded-2xl border border-border bg-card px-4 pt-4;
  padding-bottom: calc(var(--experience-hint-gap) + var(--experience-hint-row) + var(--experience-hint-gap));
}

.experience-card__title-bar {
  @apply grid w-full min-w-0 items-center gap-2;
  grid-template-columns: minmax(0, 1fr) auto;
}

.experience-card__title-wrap {
  @apply flex min-w-0 items-center;
  min-height: 1.5rem;
}

.experience-card__badge {
  @apply h-6 w-6 shrink-0 rounded-md bg-card object-contain p-0.5 ring-1 ring-border;
}

.experience-card__title {
  @apply w-full text-sm font-semibold leading-6 text-foreground line-clamp-2;
}

.experience-card__body {
  @apply mt-2 min-h-0 flex-1 text-xs leading-relaxed text-muted;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 4;
  overflow: hidden;
}

.experience-card__hint {
  @apply pointer-events-none absolute right-3 z-10 flex h-[14px] items-center gap-1 text-[11px] leading-none text-muted opacity-0 transition-opacity duration-200 group-hover:opacity-100;
  bottom: var(--experience-hint-gap);
}

.experience-card:hover {
  background: hsl(var(--hover));
}

@media (hover: none) {
  .experience-card__hint {
    @apply opacity-100;
  }
}
</style>
