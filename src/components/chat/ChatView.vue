<script setup lang="ts">
import { computed, defineAsyncComponent, defineComponent, h, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { storeToRefs } from 'pinia'
import Composer from './Composer.vue'
import ExperienceHomePanel from './ExperienceHomePanel.vue'
import ExperienceHotPreview from './ExperienceHotPreview.vue'
import PlatformLoginActions from '../auth/PlatformLoginActions.vue'
import { useChatStore } from '../../stores/chat'
import { usePlatformAuthStore } from '../../stores/platformAuth'
import TerminalLiveOutputModal from './TerminalLiveOutputModal.vue'
import { useCurrentConversationSearch } from '../../lib/useCurrentConversationSearch'
import { shouldShowMessageListPlaceholder, shouldShowWelcomeHome } from '../../lib/chatMainPane'
import {
  MOBILE_VIEWPORT_MEDIA_QUERY,
  shouldShowFooterComposer
} from '../../lib/mobileChat'
import { resolveBrandName, resolveWelcomeTip } from '../../lib/webBranding'

const brandName = resolveBrandName()
import WelcomeTipBanner from './WelcomeTipBanner.vue'

import { ChevronDown, ChevronUp, Search, X } from 'lucide-vue-next'
import { showScrollbarWhileScrolling } from '../../lib/autoHideScrollbar'
import ConversationNav from './ConversationNav.vue'
import RenderPerfHud from '../dev/RenderPerfHud.vue'
import { useRenderPerfEnabled } from '../../lib/renderPerf'
import type { MemoryGaugeReading } from '../../lib/memoryProbe'
import { measureChatRetention } from '../../lib/chatRetention'
import { EMPTY_RESIDENCY_CHAT, type ResidencyChatReading } from '../../lib/residencyProbe'

const { t } = useI18n()


/** 避免异步分包未返回前主区域长时间空白（Windows 杀毒/冷盘常见）。 */
const MessageListSkeleton = defineComponent({
  name: 'MessageListSkeleton',
  setup() {
    return () =>
      h(
        'div',
        {
          class:
            'h-full flex flex-col items-center justify-center gap-3 text-muted text-sm px-6 text-center'
        },
        [
          h('div', {
            class:
              'h-9 w-9 rounded-full border-2 border-primary/25 border-t-primary-cyan animate-spin shrink-0'
          }),
          h('span', {}, t('chat.s_f9b3f4'))
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
    deferMainPane.value,
    chat.current?.messages.length ?? 0
  )
)
const showWelcomeHome = computed(() =>
  shouldShowWelcomeHome(
    chat.current !== null,
    chat.current?.messages.length ?? 0,
    showMessageListPlaceholder.value,
    chat.current?.messageCount ?? 0
  )
)
/** Server/Vite branding tip — only on brand-new empty conversations (welcome home). */
const welcomeTip = computed(() => (showWelcomeHome.value ? resolveWelcomeTip() : null))
const isMobileViewport = ref(
  typeof window !== 'undefined' && window.matchMedia(MOBILE_VIEWPORT_MEDIA_QUERY).matches
)
const showFooterComposer = computed(() =>
  shouldShowFooterComposer(
    showWelcomeHome.value,
    isMobileViewport.value,
    isHydratingMessages.value && (chat.current?.messages.length ?? 0) === 0
  )
)
// Standalone installs (no control plane) chat with local model keys — no login.
const needsPlatformLogin = computed(
  () => !platformAuth.isStandalone && !platformAuth.session.logged_in
)
const experienceSectionExpanded = ref(false)
/** Dev-only render perf overlay — off unless toggled (see `lib/renderPerf.ts`). */
const perfHudOn = useRenderPerfEnabled()

/**
 * App-side counters for the HUD's memory rows (`lib/memoryProbe.ts`). Called at
 * most once per second and only while the HUD is on. Reads array lengths and
 * agent-trace counts — never walks the scoped sub-agent store.
 */
function readMemoryGauges(): MemoryGaugeReading[] {
  const messages = chat.current?.messages ?? []
  let spawns = 0
  for (const message of messages) spawns += message.agentTrace?.length ?? 0
  return [
    { label: 'lead msgs', value: messages.length },
    { label: 'spawns', value: spawns }
  ]
}

/**
 * Retained chat text for the HUD's residency rows (`lib/residencyProbe.ts`), read
 * at most once every few seconds and only while the HUD is on. Walks the lead
 * transcript and the live scoped rows — the dominant holders in a long
 * conversation, and the thing a leak has to be told apart from — so it is not a
 * per-tick read.
 */
function readResidency(): ResidencyChatReading {
  const conversation = chat.current
  if (!conversation) return EMPTY_RESIDENCY_CHAT
  return {
    ...measureChatRetention(
      conversation.messages ?? [],
      chat.listScopedRows(conversation.id)
    ),
    scopedSpawns: chat.countScopedSpawns(conversation.id)
  }
}

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
/**
 * Conversation-wide find box. Idle (blank query) the composable hands back shared
 * frozen empties and never reads the scoped store, so streamed sub-agent chunks cannot
 * churn the search props bound to `MessageList`.
 */
const {
  matches: pageSearchMatches,
  matchMessageIds: pageSearchMatchMessageIds,
  matchToolCallIds: pageSearchMatchToolCallIds,
  matchContentIds: pageSearchMatchContentIds
} = useCurrentConversationSearch({
  query: debouncedPageSearchQuery,
  conversationId: () => chat.currentId ?? chat.current?.id ?? '',
  messages: () => chat.current?.messages ?? []
})
const activePageSearchMatch = computed(() =>
  pageSearchMatches.value[pageSearchIndex.value] ?? null
)
const activePageSearchMessageId = computed(() =>
  activePageSearchMatch.value?.messageId ?? null
)
const activePageSearchToolCallId = computed(() =>
  activePageSearchMatch.value?.toolCallId ?? null
)
const activePageSearchContentId = computed(() =>
  activePageSearchMatch.value?.contentMessageId ?? null
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
          :placeholder="t('chat.s_03c0ed')"
          :aria-label="t('chat.s_03c0ed')"
          @keydown="onPageSearchKeydown"
        />
        <span class="min-w-12 text-center text-xs tabular-nums text-muted">
          {{ pageSearchMatches.length ? `${pageSearchIndex + 1}/${pageSearchMatches.length}` : '0/0' }}
        </span>
        <button
          type="button"
          class="rounded p-1 text-muted transition hover:bg-hover hover:text-foreground disabled:opacity-40"
          :title="t('workspace.prevMatchShift')"
          :disabled="pageSearchMatches.length === 0"
          @click="movePageSearch(-1)"
        >
          <ChevronUp class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="rounded p-1 text-muted transition hover:bg-hover hover:text-foreground disabled:opacity-40"
          :title="t('workspace.nextMatchEnter')"
          :disabled="pageSearchMatches.length === 0"
          @click="movePageSearch(1)"
        >
          <ChevronDown class="h-4 w-4" />
        </button>
        <button
          type="button"
          class="rounded p-1 text-muted transition hover:bg-hover hover:text-foreground"
          :title="t('workspace.closeEsc')"
          @click="closePageSearch"
        >
          <X class="h-4 w-4" />
        </button>
      </div>
      <div
        v-if="showWelcomeHome"
        class="chat-scroll-area auto-hide-scrollbar h-full overflow-y-auto chat-shell"
        @scroll.passive="showScrollbarWhileScrolling"
      >
        <div
          class="chat-column w-full pb-10"
          :class="isMobileViewport ? 'pt-4' : 'translate-y-[90px]'"
        >
          <!-- Desktop hero: slogan + inline composer. Mobile uses the footer composer. -->
          <template v-if="!isMobileViewport">
            <div class="mx-auto flex w-full max-w-[42rem] min-h-[clamp(7rem,calc(50vh-4.5rem),14rem)] flex-col items-center justify-end">
              <WelcomeTipBanner
                v-if="welcomeTip"
                class="mb-[30px] w-full max-w-[42rem]"
                :tip="welcomeTip"
              />
              <h1
                v-else
                class="mb-[30px] max-w-[22rem] text-center text-[1.375rem] font-semibold leading-snug tracking-tight text-foreground sm:max-w-none sm:text-[1.625rem] md:text-[1.75rem]"
              >
                <span class="brand-text">{{ brandName }}</span>{{ t('chat.welcome.slogan') }}
              </h1>
            </div>

            <div class="w-full shrink-0">
              <Composer placement="inline" />
            </div>
          </template>

          <WelcomeTipBanner
            v-else-if="welcomeTip"
            class="mb-3 w-full"
            :tip="welcomeTip"
          />

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
                  <span class="text-xs font-medium text-muted">{{ t('chat.s_79f3b1') }}</span>
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

      <div v-else class="flex h-full min-h-0 flex-col">
        <AskUserBanner />
        <div class="relative min-h-0 flex-1">
          <MessageListSkeleton v-if="showMessageListPlaceholder" />
          <MessageList
            v-else
            :key="chat.currentId ?? 'none'"
            :search-match-ids="pageSearchMatchMessageIds"
            :search-match-tool-call-ids="pageSearchMatchToolCallIds"
            :search-match-content-ids="pageSearchMatchContentIds"
            :active-search-message-id="activePageSearchMessageId"
            :active-search-tool-call-id="activePageSearchToolCallId"
            :active-search-content-id="activePageSearchContentId"
            :search-query="debouncedPageSearchQuery"
          />
          <ConversationNav />
        </div>
      </div>
    </div>
    <Composer v-if="showFooterComposer" />
    <RenderPerfHud v-if="perfHudOn" :read-gauges="readMemoryGauges" :read-residency="readResidency" />
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
