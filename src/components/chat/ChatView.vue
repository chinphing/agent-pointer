<script setup lang="ts">
import { computed, defineAsyncComponent, defineComponent, h, ref } from 'vue'
import { storeToRefs } from 'pinia'
import Composer from './Composer.vue'
import ExperienceHomePanel from './ExperienceHomePanel.vue'
import ExperienceHotPreview from './ExperienceHotPreview.vue'
import PlatformLoginActions from '../auth/PlatformLoginActions.vue'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import TerminalLiveOutputModal from './TerminalLiveOutputModal.vue'

import { isTauriRuntime } from '../../lib/runtime'
import { ChevronDown } from 'lucide-vue-next'

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
const experienceSectionExpanded = ref(false)

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
      <div v-if="empty" class="chat-scroll-area h-full overflow-y-auto chat-shell">
        <div class="chat-column w-full translate-y-[90px] pb-10">
          <div class="mx-auto flex w-full max-w-[42rem] min-h-[clamp(7rem,calc(50vh-4.5rem),14rem)] flex-col items-center justify-end">
            <h1 class="mb-[30px] max-w-[22rem] text-center text-[1.375rem] font-semibold leading-snug tracking-tight text-foreground sm:max-w-none sm:text-[1.625rem] md:text-[1.75rem]">
              <span class="brand-text">Pointer</span>：你说，我做，就这么简单！
            </h1>
          </div>

          <div class="w-full shrink-0">
            <Composer placement="inline" />
          </div>

          <div
            class="flex w-full flex-col"
            :class="experienceSectionExpanded ? 'min-h-[clamp(8rem,calc(50vh-4.5rem),16rem)] pt-12' : 'pt-10'"
          >
            <div class="mb-3 flex items-center gap-2">
              <div class="min-w-0 flex-1">
                <ExperienceHotPreview />
              </div>
              <button
                type="button"
                class="inline-flex shrink-0 cursor-pointer items-center gap-1 border-0 bg-transparent p-0 text-left outline-none focus-visible:ring-2 focus-visible:ring-accent/30"
                :aria-expanded="experienceSectionExpanded"
                @click="experienceSectionExpanded = !experienceSectionExpanded"
              >
                <span class="text-xs font-medium text-muted">更多经验</span>
                <ChevronDown
                  class="h-3.5 w-3.5 shrink-0 text-muted transition-transform duration-200"
                  :class="experienceSectionExpanded ? 'rotate-180' : ''"
                  aria-hidden="true"
                />
              </button>
            </div>

            <Transition name="experience-section-expand">
              <div v-if="experienceSectionExpanded" class="w-full">
                <ExperienceHomePanel />
              </div>
            </Transition>

            <div v-if="needsPlatformLogin" class="mt-10 flex w-full max-w-sm flex-col items-center">
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
    <Composer v-if="!empty" />
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

.experience-section-expand-enter-active,
.experience-section-expand-leave-active {
  overflow: hidden;
  transition: max-height 0.24s ease, opacity 0.2s ease;
}

.experience-section-expand-enter-from,
.experience-section-expand-leave-to {
  max-height: 0;
  opacity: 0;
}

.experience-section-expand-enter-to,
.experience-section-expand-leave-from {
  max-height: 32rem;
  opacity: 1;
}
</style>
