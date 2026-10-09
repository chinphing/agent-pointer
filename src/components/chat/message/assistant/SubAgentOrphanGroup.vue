<script setup lang="ts">
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

/**
 * `showLabel: false` 只保留容器与槽位：
 * 宿主行**存在**、只是被折叠投影隐藏（`unplaced`）的帧不该报「未关联」——
 * 那是显示选择，不是数据异常；只有宿主行真的缺失（`unbound`）才需要这行告警。
 */
withDefaults(defineProps<{ count: number; showLabel?: boolean }>(), { showLabel: true })
</script>

<template>
  <!--
    ② 渲染兜底：宿主行被折叠隐藏或本回合根本没有宿主行时，子帧过去被**无声**追加在
    消息末尾，看起来像 lead 正文的一部分。显式标注这一组，错位一眼可见。
  -->
  <div class="space-y-1">
    <p
      v-if="showLabel !== false"
      class="px-3 text-[11px] leading-4 text-amber-600/80 dark:text-amber-400/80"
      data-testid="sub-agent-orphan-label"
    >
      {{ t('chat.unlinkedSubAgents', { count }) }}
    </p>
    <slot />
  </div>
</template>
