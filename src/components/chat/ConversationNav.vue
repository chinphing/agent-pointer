<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { ChevronDown, ChevronUp } from 'lucide-vue-next'
import { useChatStore } from '../../stores/chat'
import { hasDisconnectedLiveTail, messagesInCurrentPageWindow } from '../../stores/chat/helpers'
import { listConversationOutline } from '../../lib/api'
import {
  CONVERSATION_NAV_TICK_GAP_PX,
  CONVERSATION_NAV_TICK_SLOT_PX,
  conversationNavFocusFromPointer,
  conversationNavJumpLoadsTail,
  conversationNavMaxHeightPx,
  conversationNavPageDelta,
  conversationNavScrollAffordances,
  conversationNavTickVisual,
  mergeConversationNavItems
} from '../../lib/conversationNav'
import { useMessageMilestoneStore } from '../../stores/messageMilestones'
import type { ConversationOutlineItem } from '../../types/chat'

const chat = useChatStore()
const milestones = useMessageMilestoneStore()
const fromApi = ref<ConversationOutlineItem[]>([])
const listEl = ref<HTMLElement | null>(null)
const asideEl = ref<HTMLElement | null>(null)
/** Float index of the pointer along the tick column; null when not hovering. */
const hoverFocus = ref<number | null>(null)
const hoverPreview = ref<{ text: string; top: number; messageId: string } | null>(null)
let fetchSeq = 0

const items = computed(() => {
  const merged = mergeConversationNavItems(fromApi.value, chat.current?.messages ?? [])
  const convId = chat.currentId?.trim() ?? ''
  if (!convId) return merged
  return merged.map(item => ({
    ...item,
    milestone: milestones.isMilestone(convId, item.messageId)
  }))
})
/** User turns currently in the painted page window (rest ticks slightly stronger). */
const loadedWindowUserIds = computed(() => {
  const convId = chat.currentId?.trim()
  if (!convId) return new Set<string>()
  const ids = new Set<string>()
  for (const message of messagesInCurrentPageWindow(
    chat.current?.messages ?? [],
    chat.messagePageState(convId)
  )) {
    if (message.role === 'user' && message.id) ids.add(message.id)
  }
  return ids
})
const hasItems = computed(() => items.value.length > 0)
const activeId = computed(() => chat.visibleNavMessageId)
const navMaxHeightPx = conversationNavMaxHeightPx()
const canScrollUp = ref(false)
const canScrollDown = ref(false)
let navOverflowEl: HTMLElement | null = null
let navOverflowObserver: ResizeObserver | null = null

function tickVisual(index: number) {
  const item = items.value[index]
  const focus = hoverFocus.value
  return conversationNavTickVisual({
    hoverDistance: focus == null ? null : Math.abs(index - focus),
    isActive: item?.messageId === activeId.value,
    inLoadedWindow: Boolean(item && loadedWindowUserIds.value.has(item.messageId)),
    milestone: item?.milestone === true
  })
}

function tickStyle(index: number) {
  const visual = tickVisual(index)
  return {
    width: `${visual.widthPx}px`,
    height: `${visual.heightPx}px`,
    opacity: String(visual.opacity),
    clipPath: visual.diamond ? 'polygon(50% 0%, 100% 50%, 50% 100%, 0% 50%)' : undefined,
    borderRadius: visual.diamond ? '0' : '1px'
  }
}

function tickClass(index: number) {
  return tickVisual(index).diamond ? 'bg-accent' : 'bg-foreground'
}

function setHoverFromEvent(event: MouseEvent | FocusEvent) {
  const nav = listEl.value
  const aside = asideEl.value
  const n = items.value.length
  if (!nav || !aside || n === 0) return
  const firstTick = nav.querySelector('button')
  const clientY =
    'clientY' in event ? event.clientY : (event.target as HTMLElement | null)?.getBoundingClientRect().top
  if (clientY == null || !firstTick) return
  const focus = conversationNavFocusFromPointer({
    pointerY: clientY,
    firstTickTop: firstTick.getBoundingClientRect().top,
    tickCount: n
  })
  hoverFocus.value = focus
  const nearest = items.value[Math.round(focus)]
  if (!nearest) {
    hoverPreview.value = null
    return
  }
  hoverPreview.value = {
    text: nearest.preview,
    top: clientY - aside.getBoundingClientRect().top,
    messageId: nearest.messageId
  }
}

function onNavMove(event: MouseEvent) {
  setHoverFromEvent(event)
}

function onNavLeave() {
  hoverFocus.value = null
  hoverPreview.value = null
}

/** Wheel on the rail only pans ticks; it never scrolls the transcript. */
function onNavWheel(event: WheelEvent) {
  const nav = listEl.value
  if (!nav) return
  event.preventDefault()
  nav.scrollTop += event.deltaY
  updateScrollAffordances()
}

function updateScrollAffordances() {
  const el = listEl.value
  const ticks = el?.querySelectorAll('button')
  const first = ticks?.[0]
  const last = ticks?.[ticks.length - 1]
  if (!el || !first || !last) {
    canScrollUp.value = false
    canScrollDown.value = false
    return
  }
  const viewport = el.getBoundingClientRect()
  const next = conversationNavScrollAffordances({
    firstTickTop: first.getBoundingClientRect().top,
    lastTickBottom: last.getBoundingClientRect().bottom,
    viewportTop: viewport.top,
    viewportBottom: viewport.bottom
  })
  canScrollUp.value = next.up
  canScrollDown.value = next.down
}

function unbindNavOverflow() {
  navOverflowEl?.removeEventListener('scroll', updateScrollAffordances)
  navOverflowObserver?.disconnect()
  navOverflowEl = null
  navOverflowObserver = null
}

function bindNavOverflow() {
  unbindNavOverflow()
  const el = listEl.value
  if (!el) {
    canScrollUp.value = false
    canScrollDown.value = false
    return
  }
  navOverflowEl = el
  el.addEventListener('scroll', updateScrollAffordances, { passive: true })
  if (typeof ResizeObserver !== 'undefined') {
    navOverflowObserver = new ResizeObserver(() => updateScrollAffordances())
    navOverflowObserver.observe(el)
  }
  updateScrollAffordances()
}

function onArrowClick(direction: -1 | 1) {
  const el = listEl.value
  if (!el) {
    console.warn('[nav] arrow click: tick list missing')
    return
  }
  const page = conversationNavPageDelta(el.clientHeight)
  const maxTop = Math.max(0, el.scrollHeight - el.clientHeight)
  const next = Math.max(0, Math.min(maxTop, el.scrollTop + direction * page))
  el.scrollTo({ top: next, behavior: 'smooth' })
}

function onTickFocus(item: ConversationOutlineItem, event: FocusEvent) {
  const index = items.value.findIndex(row => row.messageId === item.messageId)
  if (index < 0) return
  hoverFocus.value = index
  const aside = asideEl.value
  const target = event.currentTarget
  if (!aside || !(target instanceof HTMLElement)) return
  hoverPreview.value = {
    text: item.preview,
    top: target.getBoundingClientRect().top - aside.getBoundingClientRect().top + target.offsetHeight / 2,
    messageId: item.messageId
  }
}

watch(
  () => chat.currentId,
  async id => {
    const seq = ++fetchSeq
    fromApi.value = []
    hoverFocus.value = null
    hoverPreview.value = null
    const convId = id?.trim() ?? ''
    if (!convId) return
    try {
      const rows = await listConversationOutline(convId)
      if (seq !== fetchSeq || chat.currentId !== convId) return
      fromApi.value = rows
      milestones.hydrate(convId, rows)
      console.info('[nav] outline loaded', convId, rows.length)
    } catch (err) {
      console.warn('[nav] outline load failed', convId, err)
      if (seq !== fetchSeq) return
      fromApi.value = []
    }
  },
  { immediate: true }
)

watch(activeId, id => {
  if (!id) return
  void nextTick(() => {
    const el = listEl.value?.querySelector(
      `[data-nav-message-id="${CSS.escape(id)}"]`
    ) as HTMLElement | null
    el?.scrollIntoView({ block: 'nearest' })
    updateScrollAffordances()
  })
})

watch(
  () => [hasItems.value, items.value.length, listEl.value] as const,
  () => {
    void nextTick(() => bindNavOverflow())
  },
  { immediate: true }
)

function onJump(messageId: string) {
  const convId = chat.currentId?.trim()
  if (!convId || !messageId) return
  chat.setVisibleNavMessageId(messageId)
  const last = items.value[items.value.length - 1]
  // Outline may still be empty, so the bottom tick is only the last user in
  // the around window. Around-merge would early-return and leave the hole.
  if (
    conversationNavJumpLoadsTail({
      messageId,
      lastItemMessageId: last?.messageId,
      hasMoreNewer: chat.messagePageState(convId)?.hasMoreNewer === true,
      hasDisconnectedLiveTail: hasDisconnectedLiveTail(
        chat.current?.messages ?? [],
        chat.messagePageState(convId)?.newestPosition
      )
    })
  ) {
    chat.selectConversation(convId)
    return
  }
  chat.selectConversation(convId, { focusMessageId: messageId })
}

onBeforeUnmount(() => {
  fetchSeq += 1
  unbindNavOverflow()
})
</script>

<template>
  <aside
    v-if="hasItems"
    ref="asideEl"
    class="pointer-events-none absolute inset-y-0 right-0 z-20 flex w-7 items-center justify-end pr-1"
    aria-label="导航"
  >
    <p
      v-if="hoverPreview"
      class="pointer-events-auto absolute right-7 max-w-[14rem] -translate-y-1/2 cursor-pointer truncate rounded-md border border-border px-2 py-1 text-[11px] leading-none text-foreground shadow-sm shell-chat"
      :style="{ top: `${hoverPreview.top}px` }"
      role="tooltip"
      @click.stop="onJump(hoverPreview.messageId)"
    >{{ hoverPreview.text }}</p>
    <div
      class="pointer-events-auto flex h-max w-6 flex-none flex-col items-center"
      @wheel="onNavWheel"
    >
      <button
        v-if="canScrollUp"
        type="button"
        class="flex h-4 w-6 shrink-0 items-center justify-center text-muted transition hover:text-foreground"
        aria-label="向上"
        @click.stop="onArrowClick(-1)"
      >
        <ChevronUp class="h-3 w-3" stroke-width="2.5" />
      </button>
      <nav
        ref="listEl"
        class="flex h-max w-6 flex-none flex-col items-center justify-start overflow-y-auto overscroll-none py-1 scrollbar-hide"
        :style="{
          maxHeight: `${navMaxHeightPx}px`,
          gap: `${CONVERSATION_NAV_TICK_GAP_PX}px`
        }"
        @mousemove="onNavMove"
        @mouseleave="onNavLeave"
      >
        <button
          v-for="(item, index) in items"
          :key="item.messageId"
          type="button"
          class="flex w-full shrink-0 items-center justify-center"
          :style="{ height: `${CONVERSATION_NAV_TICK_SLOT_PX}px` }"
          :data-nav-message-id="item.messageId"
          :aria-current="item.messageId === activeId ? 'true' : undefined"
          :aria-label="item.milestone ? `里程碑 ${item.preview}` : item.preview"
          @focus="onTickFocus(item, $event)"
          @blur="onNavLeave"
          @click="onJump(item.messageId)"
        >
          <span
            class="block transition-[width,height,opacity] duration-75 ease-out"
            :class="tickClass(index)"
            :style="tickStyle(index)"
          />
        </button>
      </nav>
      <button
        v-if="canScrollDown"
        type="button"
        class="flex h-4 w-6 shrink-0 items-center justify-center text-muted transition hover:text-foreground"
        aria-label="向下"
        @click.stop="onArrowClick(1)"
      >
        <ChevronDown class="h-3 w-3" stroke-width="2.5" />
      </button>
    </div>
  </aside>
</template>
