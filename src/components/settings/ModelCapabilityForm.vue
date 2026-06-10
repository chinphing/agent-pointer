<script setup lang="ts">
import { computed } from 'vue'
import type { ProviderConfig } from '../../types/chat'
import { resolvedModelCapabilities } from '../../lib/modelCapabilities'

const props = defineProps<{
  provider: ProviderConfig
  modelId: string
}>()

const caps = computed(() =>
  resolvedModelCapabilities([props.provider], props.provider.id, props.modelId)
)

function patch(flag: 'supportsVision' | 'canGenerateImage' | 'canGenerateVideo', value: boolean) {
  const configs = { ...(props.provider.modelConfigs ?? {}) }
  const prev = configs[props.modelId] ?? {}
  configs[props.modelId] = { ...prev, [flag]: value }
  props.provider.modelConfigs = configs
}
</script>

<template>
  <div class="rounded-lg border border-border bg-[hsl(var(--card-elevated))] p-3 space-y-2">
    <p class="text-[11px] text-muted">模型能力（用于下拉过滤与多媒体路由）</p>
    <label class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-foreground">
      <input
        type="checkbox"
        class="rounded border-border bg-card text-accent focus:ring-accent/40"
        :checked="caps.supportsVision"
        @change="patch('supportsVision', ($event.target as HTMLInputElement).checked)"
      />
      支持视觉理解
    </label>
    <label class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-foreground">
      <input
        type="checkbox"
        class="rounded border-border bg-card text-accent focus:ring-accent/40"
        :checked="caps.canGenerateImage"
        @change="patch('canGenerateImage', ($event.target as HTMLInputElement).checked)"
      />
      可生成图片
    </label>
    <label class="inline-flex items-center gap-2 cursor-pointer text-[12px] text-foreground">
      <input
        type="checkbox"
        class="rounded border-border bg-card text-accent focus:ring-accent/40"
        :checked="caps.canGenerateVideo"
        @change="patch('canGenerateVideo', ($event.target as HTMLInputElement).checked)"
      />
      可生成视频
    </label>
  </div>
</template>
