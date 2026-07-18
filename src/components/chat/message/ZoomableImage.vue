<script setup lang="ts">
import { ref, onMounted, onBeforeUnmount } from 'vue'

const props = defineProps<{
  src: string
  alt?: string
}>()

const open = ref(false)

function openPreview() {
  open.value = true
}

function closePreview() {
  open.value = false
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && open.value) closePreview()
}

onMounted(() => document.addEventListener('keydown', onKeydown))
onBeforeUnmount(() => document.removeEventListener('keydown', onKeydown))
</script>

<template>
  <!-- thumbnail -->
  <img
    :src="src"
    :alt="alt"
    class="max-h-64 max-w-full rounded-xl border border-border object-contain cursor-zoom-in"
    @click="openPreview"
  />

  <!-- full-screen overlay -->
  <Teleport to="body">
    <div
      v-if="open"
      class="fixed inset-0 z-[9999] flex items-center justify-center bg-black/70 backdrop-blur-sm p-4"
      @click="closePreview"
    >
      <img
        :src="src"
        :alt="alt"
        class="max-h-[90vh] max-w-[90vw] object-contain rounded-lg shadow-2xl select-none"
        @click.stop
      />
    </div>
  </Teleport>
</template>
