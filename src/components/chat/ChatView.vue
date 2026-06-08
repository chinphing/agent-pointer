<script setup lang="ts">
import { computed, defineAsyncComponent, defineComponent, h } from 'vue'
import { storeToRefs } from 'pinia'
import Composer from './Composer.vue'
import ExperienceHomePanel from './ExperienceHomePanel.vue'
import PlatformLoginActions from '../auth/PlatformLoginActions.vue'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import TerminalLiveOutputModal from './TerminalLiveOutputModal.vue'

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
const { uiToast, terminalLivePopup } = storeToRefs(chat)
const empty = computed(() => !chat.current || chat.current.messages.length === 0)
const needsPlatformLogin = computed(() => isTauriRuntime() && !platformAuth.session.logged_in)

async function onPlatformLogin() {
  try {
    await platformAuth.login()
    chat.clearPlatformLoginErrorMessages()
  } catch {
    /* error in store */
  }
}

function onPlatformLoginCancel() {
  void platformAuth.cancelLogin()
}

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
    <TerminalLiveOutputModal
      v-if="terminalLivePopup"
      :command="terminalLivePopup.command"
      :output="terminalLivePopup.output"
      @close="chat.dismissTerminalLivePopup()"
    />
    <div class="flex-1 overflow-hidden relative">
      <div v-if="empty" class="chat-scroll-area h-full overflow-y-auto chat-shell pb-2">
        <div class="chat-column flex min-h-full items-center justify-center py-6">
          <div class="flex w-full flex-col items-center text-center">
            <div class="mb-3 flex items-center justify-center gap-2.5">
              <div class="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl border border-border/80 bg-accent/10">
                <Sparkles class="h-5 w-5 text-accent" />
              </div>
              <h1 class="brand-text text-2xl font-semibold leading-none tracking-tight md:text-[1.75rem]">
                Pointer
              </h1>
            </div>
            <p class="text-[13px] leading-relaxed text-muted">
              你的 AI 智能助理：你说，我做，就这么简单！
            </p>

            <div class="mt-8 w-full">
              <ExperienceHomePanel />
            </div>

            <div v-if="needsPlatformLogin" class="mt-8 flex w-full max-w-sm flex-col items-center">
              <div class="mb-4 h-px w-full bg-border" />
              <PlatformLoginActions
                variant="hero"
                :loading="platformAuth.loading"
                :error="platformAuth.error"
                @login="onPlatformLogin"
                @cancel="onPlatformLoginCancel"
              />
            </div>
          </div>
        </div>
      </div>

      <div v-else class="h-full flex flex-col min-h-0">
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
