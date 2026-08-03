<script setup lang="ts">
import { computed, defineAsyncComponent, defineComponent, h, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import Composer from './Composer.vue'
import ChangeSummary from './ChangeSummary.vue'
import ExperienceHomePanel from './ExperienceHomePanel.vue'
import ExperienceHotPreview from './ExperienceHotPreview.vue'
import PlatformLoginActions from '../auth/PlatformLoginActions.vue'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import TerminalLiveOutputModal from './TerminalLiveOutputModal.vue'
import { findCurrentConversationMatches } from '../../lib/currentConversationSearch'
import { shouldShowMessageListPlaceholder, shouldShowWelcomeHome } from '../../lib/chatMainPane'
import {
  MOBILE_VIEWPORT_MEDIA_QUERY,
  shouldShowFooterComposer
} from '../../lib/mobileChat'

import { ChevronDown, ChevronUp, Search, X } from 'lucide-vue-next'

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
const isHydratingMessages = computed(() => chat.isCurrentConversationHydrating)
/**
 * After currentId changes, keep the main pane on a cheap skeleton for one frame
 * so the sidebar is-active paint is not blocked by MessageList layout.
 */
const deferMainPane = ref(false)
watch(
  () => chat.currentId,
  id => {
    if (!id) {
      deferMainPane.value = false
      return
    }
    deferMainPane.value = true
    requestAnimationFrame(() => {
      if (chat.currentId === id) deferMainPane.value = false
    })
  }
)
const showMessageListPlaceholder = computed(() =>
  shouldShowMessageListPlaceholder(
    chat.current !== null,
    isHydratingMessages.value,
    deferMainPane.value
  )
)
const conversationMessages = computed(() => chat.current?.messages ?? [])
const showWelcomeHome = computed(() =>
  shouldShowWelcomeHome(
    chat.current !== null,
    chat.current?.messages.length ?? 0,
    showMessageListPlaceholder.value
  )
)
const isMobileViewport = ref(
  typeof window !== 'undefined' && window.matchMedia(MOBILE_VIEWPORT_MEDIA_QUERY).matches
)
const showFooterComposer = computed(() =>
  shouldShowFooterComposer(
    showWelcomeHome.value,
    isMobileViewport.value,
    isHydratingMessages.value
  )
)
const needsPlatformLogin = computed(() => !platformAuth.session.logged_in)
const experienceSectionExpanded = ref(false)
let mobileMediaQuery: MediaQueryList | null = null

function updateMobileViewport() {
  isMobileViewport.value = window.matchMedia(MOBILE_VIEWPORT_MEDIA_QUERY).matches
}
const pageSearchOpen = ref(false)
const pageSearchQuery = ref('')
const debouncedPageSearchQuery = ref('')
const pageSearchIndex = ref(0)
const pageSearchInput = ref<HTMLInputElement | null>(null)
let pageSearchDebounceTimer: number | null = null
const pageSearchMatches = computed(() =>
  findCurrentConversationMatches(chat.current?.messages ?? [], debouncedPageSearchQuery.value)
)
const pageSearchMatchMessageIds = computed(() =>
  pageSearchMatches.value.filter(match => !match.toolCallId).map(match => match.messageId)
)
const pageSearchMatchToolCallIds = computed(() =>
  pageSearchMatches.value.flatMap(match => match.toolCallId ? [match.toolCallId] : [])
)
const activePageSearchMatch = computed(() =>
  pageSearchMatches.value[pageSearchIndex.value] ?? null
)
const activePageSearchMessageId = computed(() =>
  activePageSearchMatch.value?.messageId ?? null
)
const activePageSearchToolCallId = computed(() =>
  activePageSearchMatch.value?.toolCallId ?? null
)

function openPageSearch() {
  pageSearchOpen.value = true
  void nextTick(() => {
    pageSearchInput.value?.focus()
    pageSearchInput.value?.select()
  })
}

function closePageSearch() {
  pageSearchOpen.value = false
  pageSearchQuery.value = ''
  debouncedPageSearchQuery.value = ''
  if (pageSearchDebounceTimer != null) {
    window.clearTimeout(pageSearchDebounceTimer)
    pageSearchDebounceTimer = null
  }
  pageSearchIndex.value = 0
}

function flushPageSearchQuery() {
  if (pageSearchDebounceTimer != null) {
    window.clearTimeout(pageSearchDebounceTimer)
    pageSearchDebounceTimer = null
  }
  debouncedPageSearchQuery.value = pageSearchQuery.value
}

function movePageSearch(direction: 1 | -1) {
  flushPageSearchQuery()
  const count = pageSearchMatches.value.length
  if (count === 0) return
  pageSearchIndex.value = (pageSearchIndex.value + direction + count) % count
}

function onPageSearchKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    event.preventDefault()
    closePageSearch()
  } else if (event.key === 'Enter') {
    event.preventDefault()
    movePageSearch(event.shiftKey ? -1 : 1)
  }
}

function onGlobalFindShortcut(event: KeyboardEvent) {
  if (event.key.toLocaleLowerCase() !== 'f' || (!event.metaKey && !event.ctrlKey)) return
  // Workspace file tree / file preview may claim ⌘/Ctrl+F in the capture phase.
  if (event.defaultPrevented) return
  // Workspace terminal needs Ctrl+F (vim page-forward); do not open in-page search.
  const target = event.target as HTMLElement | null
  if (
    target?.closest?.('[data-workspace-terminal]') ||
    document.activeElement?.closest?.('[data-workspace-terminal]')
  ) {
    return
  }
  event.preventDefault()
  openPageSearch()
}

watch(pageSearchQuery, query => {
  if (pageSearchDebounceTimer != null) window.clearTimeout(pageSearchDebounceTimer)
  if (!query) {
    pageSearchDebounceTimer = null
    debouncedPageSearchQuery.value = ''
    return
  }
  pageSearchDebounceTimer = window.setTimeout(() => {
    pageSearchDebounceTimer = null
    debouncedPageSearchQuery.value = query
  }, 500)
})

watch(pageSearchMatches, matches => {
  if (matches.length === 0) pageSearchIndex.value = 0
  else if (pageSearchIndex.value >= matches.length) pageSearchIndex.value = 0
})

watch(() => chat.currentId, () => closePageSearch())

onMounted(() => {
  window.addEventListener('keydown', onGlobalFindShortcut)
  mobileMediaQuery = window.matchMedia(MOBILE_VIEWPORT_MEDIA_QUERY)
  updateMobileViewport()
  mobileMediaQuery.addEventListener('change', updateMobileViewport)
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onGlobalFindShortcut)
  mobileMediaQuery?.removeEventListener('change', updateMobileViewport)
  mobileMediaQuery = null
  if (pageSearchDebounceTimer != null) window.clearTimeout(pageSearchDebounceTimer)
})

async function onPlatformLogin() {
  try {
    await platformAuth.login()
    chat.clearPlatformLoginErrorMessages()
  } catch {
    /* error in store */
  }
}

function onLocalLoginSuccess() {
  chat.clearPlatformLoginErrorMessages()
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
      v-if="terminalLivePopup && !chat.terminalInputRequest"
      :command="terminalLivePopup.command"
      :output="terminalLivePopup.output"
      @close="chat.dismissTerminalLivePopup()"
    />
    <div class="flex-1 overflow-hidden relative">
      <div
        v-if="pageSearchOpen && !showWelcomeHome && !isHydratingMessages"
        class="absolute right-5 top-3 z-40 flex items-center gap-1 rounded-lg border border-border bg-background/95 p-1.5 shadow-lg backdrop-blur"
        role="search"
      >
        <Search class="ml-1 h-4 w-4 shrink-0 text-muted" aria-hidden="true" />
        <input
          ref="pageSearchInput"
          v-model="pageSearchQuery"
          class="w-56 bg-transparent px-1.5 py-1 text-sm text-foreground outline-none placeholder:text-muted"
          type="search"
          placeholder="在当前对话中查找"
          aria-label="在当前对话中查找"
          @keydown="onPageSearchKeydown"
        />
        <span class="min-w-12 text-center text-xs tabular-nums text-muted">
          {{ pageSearchMatches.length ? `${pageSearchIndex + 1}/${pageSearchMatches.length}` : '0/0' }}
        </span>
        <button
          type="button"
          class="rounded p-1 text-muted transition hover:bg-hover hover:text-foreground disabled:opacity-40"
          title="上一个匹配（Shift+Enter）"
          :disabled="pageSearchMatches.length === 0"
          @click="movePageSearch(-1)"
        >
          <ChevronUp class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="rounded p-1 text-muted transition hover:bg-hover hover:text-foreground disabled:opacity-40"
          title="下一个匹配（Enter）"
          :disabled="pageSearchMatches.length === 0"
          @click="movePageSearch(1)"
        >
          <ChevronDown class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="rounded p-1 text-muted transition hover:bg-hover hover:text-foreground"
          title="关闭（Esc）"
          @click="closePageSearch"
        >
          <X class="h-4 w-4" />
        </button>
      </div>
      <div v-if="showMessageListPlaceholder" class="h-full flex flex-col min-h-0">
        <MessageListSkeleton />
      </div>

      <div v-else-if="showWelcomeHome" class="chat-scroll-area h-full overflow-y-auto chat-shell">
        <div
          class="chat-column w-full pb-10"
          :class="isMobileViewport ? 'pt-4' : 'translate-y-[90px]'"
        >
          <!-- Desktop hero: slogan + inline composer. Mobile uses the footer composer. -->
          <template v-if="!isMobileViewport">
            <div class="mx-auto flex w-full max-w-[42rem] min-h-[clamp(7rem,calc(50vh-4.5rem),14rem)] flex-col items-center justify-end">
              <h1 class="mb-[30px] max-w-[22rem] text-center text-[1.375rem] font-semibold leading-snug tracking-tight text-foreground sm:max-w-none sm:text-[1.625rem] md:text-[1.75rem]">
                <span class="brand-text">Pointer</span>：你说，我做，就这么简单！
              </h1>
            </div>

            <div class="w-full shrink-0">
              <Composer placement="inline" />
            </div>
          </template>

          <div
            v-if="!isMobileViewport || needsPlatformLogin"
            class="flex w-full flex-col"
            :class="
              isMobileViewport
                ? 'pt-2'
                : !platformAuth.isStandalone && experienceSectionExpanded
                  ? 'min-h-[clamp(8rem,calc(50vh-4.5rem),16rem)] pt-12'
                  : 'pt-10'
            "
          >
            <template v-if="!isMobileViewport && !platformAuth.isStandalone">
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
            </template>

            <div v-if="needsPlatformLogin" class="mt-10 flex w-full max-w-sm flex-col items-center">
              <div class="mb-4 h-px w-full bg-border" />
              <PlatformLoginActions
                variant="hero"
                :loading="platformAuth.loading"
                :error="platformAuth.error"
                @login="onPlatformLogin"
                @cancel="onPlatformLoginCancel"
                @local-success="onLocalLoginSuccess"
              />
            </div>
          </div>
        </div>
      </div>

      <div v-else class="h-full flex flex-col min-h-0">
        <div class="flex-1 min-h-0 overflow-hidden">
          <MessageList
            :search-match-ids="pageSearchMatchMessageIds"
            :search-match-tool-call-ids="pageSearchMatchToolCallIds"
            :active-search-message-id="activePageSearchMessageId"
            :active-search-tool-call-id="activePageSearchToolCallId"
            :search-query="debouncedPageSearchQuery"
          />
        </div>
      </div>
    </div>
    <div v-if="!showWelcomeHome && !isHydratingMessages" class="chat-shell shrink-0 bg-background">
      <div class="chat-column">
        <ChangeSummary :messages="conversationMessages" />
      </div>
    </div>
    <Composer v-if="showFooterComposer" />
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
