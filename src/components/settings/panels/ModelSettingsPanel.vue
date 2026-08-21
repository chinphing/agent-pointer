<script setup lang="ts">
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import ModelServiceSection from '../ModelServiceSection.vue'

const props = defineProps<{
  form: SettingsDialogForm
}>()
</script>

<template>
  <div class="flex-1 flex flex-col gap-5">
    <ModelServiceSection :form="form" />
  </div>
</template>
