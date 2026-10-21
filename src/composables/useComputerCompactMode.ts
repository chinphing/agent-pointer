import { computed, onUnmounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useChatStore } from '../stores/chat'
import { useSettingsStore } from '../stores/settings'
import { shouldShrinkComputerWindow } from '../lib/computerExecuting'
import {
  restoreComputerCompactWindow,
  setCompactShellActive,
  shrinkComputerCompactWindow
} from './useComputerCompactWindow'
import { useComputerCompactTitle } from './useComputerCompactTitle'

export function useComputerCompactMode() {
  const chat = useChatStore()
  const settings = useSettingsStore()
  const {
    generating,
    activeGeneratingMessageId,
    currentId,
    computerMonitorPickRequest
  } = storeToRefs(chat)

  const isCompact = ref(false)
  const userExpandedOverride = ref(false)
  const stoppedHint = ref(false)
  let stoppedTimer: ReturnType<typeof setTimeout> | undefined

  const { planSummary, planLine, statusLine, twoLines } = useComputerCompactTitle(stoppedHint)

  const activeMessage = computed(() => {
    const conv = chat.current
    const id = activeGeneratingMessageId.value
    if (!conv || !id) return undefined
    return conv.messages.find(m => m.id === id)
  })

  const computerAutoCompact = computed(
    () => settings.userSettings.computerAutoCompact !== false
  )

  const shouldEnterCompact = computed(() => {
    if (!computerAutoCompact.value) return false
    if (userExpandedOverride.value) return false
    if (computerMonitorPickRequest.value) return false
    return shouldShrinkComputerWindow(
      generating.value,
      chat.effectiveConversationLeadAgentId(chat.current),
      activeMessage.value
    )
  })

  async function applyCompact(enter: boolean) {
    if (enter) {
      setCompactShellActive(true)
      const ok = await shrinkComputerCompactWindow(twoLines.value)
      if (ok) {
        isCompact.value = true
      } else {
        setCompactShellActive(false)
        isCompact.value = false
      }
    } else {
      isCompact.value = false
      await restoreComputerCompactWindow()
    }
  }

  watch(
    shouldEnterCompact,
    enter => {
      if (enter && !isCompact.value) {
        const id = currentId.value
        if (id) void chat.refreshTaskBoard(id)
        void applyCompact(true)
      } else if (!enter && isCompact.value) {
        void applyCompact(false)
      }
    },
    { flush: 'post' }
  )

  watch(twoLines, (lines, prev) => {
    if (!isCompact.value || lines === prev) return
    void shrinkComputerCompactWindow(lines, true)
  })

  watch(generating, (g, wasGenerating) => {
    if (g) return
    stoppedHint.value = false
    if (stoppedTimer) {
      clearTimeout(stoppedTimer)
      stoppedTimer = undefined
    }
    // Manual expand override applies only for the current generation turn.
    userExpandedOverride.value = false
    if (wasGenerating && isCompact.value) {
      void applyCompact(false)
    }
  })

  watch(currentId, () => {
    userExpandedOverride.value = false
    if (isCompact.value) void applyCompact(false)
  })

  watch(
    () => chat.effectiveConversationLeadAgentId(chat.current),
    () => {
      if (isCompact.value && !shouldEnterCompact.value) void applyCompact(false)
    }
  )

  function expand() {
    userExpandedOverride.value = true
    if (isCompact.value) void applyCompact(false)
  }

  async function stop() {
    stoppedHint.value = true
    await chat.stop()
    if (stoppedTimer) clearTimeout(stoppedTimer)
    stoppedTimer = setTimeout(() => {
      stoppedHint.value = false
      stoppedTimer = undefined
    }, 1200)
  }

  onUnmounted(() => {
    if (stoppedTimer) clearTimeout(stoppedTimer)
    if (isCompact.value) void restoreComputerCompactWindow()
  })

  return {
    isCompact,
    planSummary,
    planLine,
    statusLine,
    twoLines,
    expand,
    stop
  }
}
