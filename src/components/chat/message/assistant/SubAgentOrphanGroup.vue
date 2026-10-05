<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

defineProps<{ count: number }>()
</script>

<template>
  <!--
    ② 渲染兜底：宿主行被折叠隐藏或本回合根本没有宿主行时，子帧过去被**无声**追加在
    消息末尾，看起来像 lead 正文的一部分。显式标注这一组，错位一眼可见。
  -->
  <div class="space-y-1">
    <p
      class="px-3 text-[11px] leading-4 text-amber-600/80 dark:text-amber-400/80"
      data-testid="sub-agent-orphan-label"
    >
      {{ t('chat.unlinkedSubAgents', { count }) }}
    </p>
    <slot />
  </div>
</template>
