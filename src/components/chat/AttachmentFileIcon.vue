<script setup lang="ts">
import { computed } from 'vue'
import { attachmentFileBadge } from '../../lib/attachmentFileIcon'

const props = defineProps<{
  fileName?: string | null
  mimeType?: string | null
}>()

const badge = computed(() => attachmentFileBadge(props.fileName, props.mimeType))

const toneClass = computed(() => {
  switch (badge.value.tone) {
    case 'sheet':
      return 'att-file-badge--sheet'
    case 'word':
      return 'att-file-badge--word'
    case 'slides':
      return 'att-file-badge--slides'
    case 'pdf':
      return 'att-file-badge--pdf'
    case 'zip':
      return 'att-file-badge--zip'
    case 'code':
      return 'att-file-badge--code'
    case 'text':
      return 'att-file-badge--text'
    default:
      return 'att-file-badge--file'
  }
})
</script>

<template>
  <span
    class="att-file-badge shrink-0"
    :class="toneClass"
    :title="badge.label"
    aria-hidden="true"
  >
    <span class="att-file-badge__label">{{ badge.label }}</span>
  </span>
</template>

<style scoped>
.att-file-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 1.125rem;
  height: 1.125rem;
  border-radius: 0.25rem;
  color: #fff;
  line-height: 1;
  user-select: none;
}

.att-file-badge__label {
  font-size: 0.5rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  transform: scale(0.92);
  white-space: nowrap;
}

/* Familiar file-type hues (Excel green / Word blue / PPT orange / PDF red). */
.att-file-badge--sheet {
  background: hsl(142 52% 38%);
}
.att-file-badge--word {
  background: hsl(211 90% 46%);
}
.att-file-badge--slides {
  background: hsl(16 78% 48%);
}
.att-file-badge--pdf {
  background: hsl(0 72% 48%);
}
.att-file-badge--zip {
  background: hsl(36 70% 42%);
}
.att-file-badge--code {
  background: hsl(199 55% 40%);
}
.att-file-badge--text {
  background: hsl(220 12% 46%);
}
.att-file-badge--file {
  background: hsl(220 10% 42%);
}

:global(html.dark) .att-file-badge--sheet {
  background: hsl(142 40% 42%);
}
:global(html.dark) .att-file-badge--word {
  background: hsl(211 80% 52%);
}
:global(html.dark) .att-file-badge--slides {
  background: hsl(16 70% 52%);
}
:global(html.dark) .att-file-badge--pdf {
  background: hsl(0 65% 54%);
}
:global(html.dark) .att-file-badge--zip {
  background: hsl(36 65% 48%);
}
:global(html.dark) .att-file-badge--code {
  background: hsl(199 45% 48%);
}
:global(html.dark) .att-file-badge--text,
:global(html.dark) .att-file-badge--file {
  background: hsl(220 10% 48%);
}
</style>
