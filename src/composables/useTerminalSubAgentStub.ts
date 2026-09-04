import { onUnmounted, ref, watch, type MaybeRefOrGetter, toValue } from 'vue'
import type { AgentTrace } from '../types/chat'
import {
  shouldKeepFullSubAgentFrame,
  shouldStubTerminalImmediately,
  TERMINAL_COLLAPSED_STUB_IDLE_MS
} from '../lib/subAgentFrameMount'
import { isSubAgentTraceTerminal } from '../lib/subAgentSession'

/** After a terminal trace stays collapsed, swap SubAgentFrame for a lightweight stub. */
export function useTerminalSubAgentStub(
  trace: MaybeRefOrGetter<AgentTrace>,
  options?: { pinned?: MaybeRefOrGetter<boolean> }
) {
  const stubIdleElapsed = ref(false)
  let timer: ReturnType<typeof setTimeout> | null = null
  let initialized = false

  function clearTimer() {
    if (timer != null) {
      clearTimeout(timer)
      timer = null
    }
  }

  function startGraceTimer() {
    stubIdleElapsed.value = false
    timer = setTimeout(() => {
      stubIdleElapsed.value = true
      timer = null
    }, TERMINAL_COLLAPSED_STUB_IDLE_MS)
  }

  function reschedule() {
    clearTimer()
    const t = toValue(trace)
    const status = (t.status ?? '').trim()

    if (shouldKeepFullSubAgentFrame(t)) {
      stubIdleElapsed.value = false
      initialized = true
      return
    }
    if (toValue(options?.pinned) === true) {
      stubIdleElapsed.value = false
      initialized = true
      return
    }

    if (shouldStubTerminalImmediately({ initialized, status })) {
      stubIdleElapsed.value = true
    } else if (isSubAgentTraceTerminal(status)) {
      startGraceTimer()
    } else {
      stubIdleElapsed.value = false
    }

    initialized = true
  }

  watch(
    () => [
      toValue(trace).id,
      toValue(trace).status,
      toValue(trace).userExpanded,
      toValue(trace).collapsed,
      toValue(options?.pinned)
    ] as const,
    reschedule,
    { immediate: true }
  )

  onUnmounted(clearTimer)

  return { stubIdleElapsed }
}
