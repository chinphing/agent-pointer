<script setup lang="ts">
import { computed, defineAsyncComponent, defineComponent, h } from 'vue'
import { storeToRefs } from 'pinia'
import Composer from './Composer.vue'
import TaskBoardPanel from './TaskBoardPanel.vue'
import PlatformLoginActions from '../auth/PlatformLoginActions.vue'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import { useLeadAgentUi } from '../../composables/useAgentUi'
import { hasTaskBoardContent } from '../../lib/taskBoard'
import { isTauriRuntime } from '../../lib/runtime'
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
const platformAuth = usePlatformAuthStore()
const { uiToast } = storeToRefs(chat)
const { leadUi } = useLeadAgentUi()
const empty = computed(() => !chat.current || chat.current.messages.length === 0)
const needsPlatformLogin = computed(() => isTauriRuntime() && !platformAuth.session.logged_in)

async function onPlatformLogin() {
  try {
    await platformAuth.login()
  } catch {
    /* error in store */
  }
}

function onPlatformLoginCancel() {
  void platformAuth.cancelLogin()
}

const taskBoardState = computed(() =>
  chat.currentId ? chat.taskBoardForConversation(chat.currentId) : null
)

const showTaskBoardPanel = computed(() => {
  if (!leadUi.value.showTaskBoardPanel) return false
  if (!chat.currentId) return false
  const state = taskBoardState.value
  if (!state) return false
  if (hasTaskBoardContent(state.parent)) return true
  return Object.values(state.children ?? {}).some(hasTaskBoardContent)
})

const toastClass = computed(() => {
  const t = uiToast.value
  if (!t) return ''
  if (t.level === 'error') return 'border-danger/40 bg-danger/10 text-danger'
  if (t.level === 'warning') return 'border-warning/40 bg-warning/10 text-warning'
  return 'border-success/35 bg-success/10 text-success'
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
          class="pointer-events-auto rounded-xl border px-4 py-2.5 text-[13px] leading-snug shadow-lg panel"
          :class="toastClass"
        >
          {{ uiToast.message }}
        </div>
      </div>
    </Transition>
    <div class="flex-1 overflow-hidden relative">
      <div v-if="empty" class="h-full flex flex-col items-center justify-center px-8 text-center">
        <div class="w-14 h-14 rounded-2xl bg-accent/15 border border-border flex items-center justify-center mb-4">
          <Sparkles class="w-7 h-7 text-accent" />
        </div>
        <h1 class="text-2xl font-bold brand-text mb-1.5">你好，欢迎来到 Pointer</h1>
        <p class="text-muted max-w-md text-sm leading-6">
          你的 AI 智能助手，可以操控电脑、编写代码。
        </p>
        <div v-if="needsPlatformLogin" class="mt-6 flex w-full max-w-sm flex-col items-center">
          <div class="mb-5 h-px w-full bg-border" />
          <PlatformLoginActions
            variant="hero"
            :loading="platformAuth.loading"
            :error="platformAuth.error"
            @login="onPlatformLogin"
            @cancel="onPlatformLoginCancel"
          />
        </div>
      </div>

      <div v-else class="h-full flex flex-col min-h-0">
        <div class="shrink-0 px-6 md:px-10 pt-4">
          <div class="max-w-3xl mx-auto">
            <TaskBoardPanel
              v-if="showTaskBoardPanel"
              :document="taskBoardState?.parent ?? null"
              :child-boards="taskBoardState?.children"
            />
          </div>
        </div>
        <div class="flex-1 min-h-0 overflow-hidden">
          <MessageList />
        </div>
      </div>
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
