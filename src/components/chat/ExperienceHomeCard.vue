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
    <div class="experience-card__title">{{ truncateExperienceCardTitle(item.title) }}</div>
    <div class="experience-card__body-zone">
      <template v-if="item.cover_image_url">
        <div class="experience-card__cover-spacer" aria-hidden="true" />
        <img
          :src="item.cover_image_url"
          alt=""
          class="experience-card__cover"
          loading="lazy"
        />
      </template>
      <p class="experience-card__body">{{ formatExperienceCardExcerpt(item.excerpt) }}</p>
    </div>
    <span class="experience-card__hint" aria-hidden="true">
      点击使用
      <MousePointerClick class="h-3.5 w-3.5 shrink-0" />
    </span>
  </button>
</template>

<style scoped>
.experience-card {
  /* 3px above hint + hint row (14px) + 3px from card bottom */
  --experience-hint-gap: 3px;
  --experience-hint-row: 14px;
  @apply relative flex h-[8.5rem] w-full flex-col rounded-2xl px-4 pt-4;
  padding-bottom: calc(var(--experience-hint-gap) + var(--experience-hint-row) + var(--experience-hint-gap));
}

.experience-card__title {
  @apply shrink-0 text-sm font-semibold leading-[1.375] text-foreground line-clamp-2;
}

.experience-card__body-zone {
  @apply relative mt-2 min-h-0 flex-1 overflow-hidden;
}

.experience-card__body-zone::after {
  content: '';
  display: table;
  clear: both;
}

.experience-card__cover-spacer {
  float: right;
  width: 4.5rem;
  height: calc(100% - 4.5rem);
}

.experience-card__cover {
  @apply ml-2 aspect-square w-[4.5rem] rounded-lg bg-black/5 object-cover;
  float: right;
  clear: right;
  shape-outside: margin-box;
}

.experience-card__body {
  @apply text-xs leading-relaxed text-muted;
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 4;
  overflow: hidden;
}

.experience-card__hint {
  @apply pointer-events-none absolute right-3 z-10 flex h-[14px] items-center gap-1 text-[11px] leading-none text-muted opacity-0 transition-opacity duration-200 group-hover:opacity-100;
  bottom: var(--experience-hint-gap);
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
