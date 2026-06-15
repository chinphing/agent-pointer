<script setup lang="ts">
import { computed, useAttrs } from 'vue'
import type { WindowDragRegionId } from '../../lib/windowDragRegions'
import { WINDOW_DRAG_INTERACTIVE_SELECTOR, WINDOW_DRAG_REGION_POLICIES } from '../../lib/windowDragRegions'
import { useWindowDragRegion } from '../../composables/useWindowDragRegion'

defineOptions({ inheritAttrs: false })

const props = withDefaults(
  defineProps<{
    region: WindowDragRegionId
    as?: string
  }>(),
  { as: 'div' }
)

const attrs = useAttrs()
const policy = computed(() => WINDOW_DRAG_REGION_POLICIES[props.region])
const { onMouseDown, onDoubleClick } = useWindowDragRegion()

const regionClass = computed(() => [
  attrs.class,
  policy.value.draggable ? 'window-drag-region' : 'window-no-drag-region'
])

const regionAttrs = computed(() => {
  const extra = { ...attrs, class: undefined }
  if (!policy.value.draggable) {
    return { ...extra, 'data-tauri-drag-region': 'false', 'data-window-drag-region': props.region }
  }
  return { ...extra, 'data-window-drag-region': props.region }
})

function onRegionMouseDown(e: MouseEvent) {
  const target = e.target as HTMLElement | null
  if (target?.closest(WINDOW_DRAG_INTERACTIVE_SELECTOR)) return

  if (!policy.value.draggable) {
    e.preventDefault()
    e.stopPropagation()
    return
  }
  onMouseDown(e, policy.value)
}

function onRegionDoubleClick(e: MouseEvent) {
  const target = e.target as HTMLElement | null
  if (target?.closest(WINDOW_DRAG_INTERACTIVE_SELECTOR)) return
  if (!policy.value.draggable) return
  onDoubleClick(e, policy.value)
}
</script>

<template>
  <component
    :is="as"
    :class="regionClass"
    v-bind="regionAttrs"
    @mousedown="onRegionMouseDown"
    @dblclick="onRegionDoubleClick"
  >
    <slot />
  </component>
</template>
