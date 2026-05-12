<script setup lang="ts">
import { computed, defineAsyncComponent, defineComponent, h } from 'vue'
import { storeToRefs } from 'pinia'
import Composer from './Composer.vue'
import { useChatStore } from '../../stores/chat'
import { Sparkles } from 'lucide-vue-next'

/** 避免异步分包未返回前主区域长时间空白（Windows 杀毒/冷盘常见）。 */
const MessageListSkeleton = defineComponent({
  name: 'MessageListSkeleton',
  setup() {
    return () =>
      h(
        'div',
        {
          class:
            'h-full flex flex-col items-center justify-center gap-3 text-slate-500 text-sm px-6 text-center'
        },
        [
          h('div', {
            class:
              'h-9 w-9 rounded-full border-2 border-primary/25 border-t-primary-cyan animate-spin shrink-0'
          }),
          h('span', {}, '加载消息列表…')
        ]
      )
  }
})

const MessageList = defineAsyncComponent({
  loader: () => import('./MessageList.vue'),
  loadingComponent: MessageListSkeleton,
  delay: 0
})

const chat = useChatStore()
const { uiToast } = storeToRefs(chat)
const empty = computed(() => !chat.current || chat.current.messages.length === 0)

const toastClass = computed(() => {
  const t = uiToast.value
  if (!t) return ''
  if (t.level === 'error') return 'border-red-500/40 bg-red-950/90 text-red-100'
  if (t.level === 'warning') return 'border-amber-500/40 bg-amber-950/85 text-amber-50'
  return 'border-emerald-500/35 bg-emerald-950/80 text-emerald-50'
})
</script>

<template>
  <div class="flex-1 flex flex-col min-h-0">
    <Transition name="toast-fade">
      <div
        v-if="uiToast"
        class="pointer-events-none fixed top-4 left-1/2 z-[300] flex max-w-[min(90vw,28rem)] -translate-x-1/2 justify-center px-4"
        role="status"
      >
        <div
          class="pointer-events-auto rounded-xl border px-4 py-2.5 text-[13px] leading-snug shadow-xl backdrop-blur-md"
          :class="toastClass"
        >
          {{ uiToast.message }}
        </div>
      </div>
    </Transition>
    <div class="flex-1 overflow-hidden relative">
      <div v-if="empty" class="h-full flex flex-col items-center justify-center px-8 text-center">
        <div class="w-14 h-14 rounded-2xl bg-gradient-to-br from-primary via-primary-fuchsia to-primary-cyan flex items-center justify-center shadow-xl shadow-primary/30 mb-4">
          <Sparkles class="w-7 h-7 text-white" />
        </div>
        <h1 class="text-2xl font-bold gradient-text mb-1.5">你好，欢迎来到 Pointer</h1>
        <p class="text-slate-400 max-w-md text-sm leading-6">
          你的 AI 智能助手，可以回答问题、写作、分析数据、执行任务。
        </p>
      </div>

      <MessageList v-else />
    </div>
    <Composer />
  </div>
</template>

<style scoped>
.toast-fade-enter-active,
.toast-fade-leave-active {
  transition: opacity 0.22s ease;
}
.toast-fade-enter-from,
.toast-fade-leave-to {
  opacity: 0;
}
</style>
