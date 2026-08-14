<script setup lang="ts">
import type { SettingsDialogForm } from '../../../composables/useSettingsDialogForm'
import { Cpu } from 'lucide-vue-next'
import ModelServiceSection from '../ModelServiceSection.vue'

const props = defineProps<{
  form: SettingsDialogForm
}>()
</script>

<template>
  <div class="flex-1 flex flex-col gap-5">
    <div>
      <h3 class="text-sm font-semibold text-foreground flex items-center gap-2">
        <Cpu class="w-4 h-4 text-accent" />模型配置
      </h3>
      <p class="mt-0.5 text-xs text-muted">服务商、模型与场景档位映射</p>
    </div>

    <ModelServiceSection :form="form" />
  </div>
</template>
