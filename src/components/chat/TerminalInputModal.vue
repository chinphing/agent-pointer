<script setup lang="ts">
import { computed, nextTick, onUnmounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { KeyRound, Terminal, X } from 'lucide-vue-next'
import { dismissTerminalInput, submitTerminalInput } from '../../lib/api'
import type { TerminalInputRequest } from '../../types/chat'


const { t } = useI18n()
const props = defineProps<{
  request: TerminalInputRequest
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const text = ref('')
const submitting = ref(false)
const error = ref('')

const isSecret = computed(() => props.request.inputClass === 'secret')
const title = computed(() => (isSecret.value ? t('terminalInputModal.titleSecret') : t('terminalInputModal.titleCommand')))
const commandText = computed(() => props.request.command?.trim() ?? '')
const outputContext = computed(() => props.request.outputContext?.trim() ?? '')
const showContext = computed(() => commandText.value.length > 0 || outputContext.value.length > 0)
const outputEl = ref<HTMLElement | null>(null)

function scrollOutputToBottom() {
  const el = outputEl.value
  if (!el) return
  el.scrollTop = el.scrollHeight
}

watch(outputContext, () => {
  void nextTick(scrollOutputToBottom)
}, { immediate: true })

async function submit() {
  const value = text.value
  if (!value.trim()) {
    error.value = t('terminalInputModal.enterContent')
    return
  }
  submitting.value = true
  error.value = ''
  try {
    await submitTerminalInput(props.request.requestId, value)
    text.value = ''
    emit('close')
  } catch (e: unknown) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    submitting.value = false
  }
}

async function dismiss() {
  submitting.value = true
  try {
    await dismissTerminalInput(props.request.requestId)
  } catch {
    // Best effort; backend may already have timed out.
  } finally {
    submitting.value = false
    text.value = ''
    emit('close')
  }
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    void dismiss()
  } else if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    void submit()
  }
}

document.addEventListener('keydown', onKeydown)
onUnmounted(() => document.removeEventListener('keydown', onKeydown))
</script>

<template>
  <Teleport to="body">
    <div
      class="fixed inset-0 z-[250] flex items-center justify-center bg-foreground/32 p-4"
      role="dialog"
      aria-modal="true"
      :aria-label="title"
      @click.self="dismiss()"
    >
      <div
        class="relative flex w-full max-w-xl max-h-[min(90vh,640px)] flex-col overflow-hidden rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl"
      >
        <header class="flex shrink-0 items-start gap-3 border-b border-border px-5 py-4 pr-14">
          <div
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
            :class="isSecret ? 'bg-warning/15' : 'bg-accent/15'"
          >
            <KeyRound v-if="isSecret" class="h-4 w-4 text-warning" />
            <Terminal v-else class="h-4 w-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1">
            <h2 class="text-base font-semibold text-foreground">{{ title }}</h2>
          </div>
          <button
            type="button"
            class="absolute top-4 right-4 rounded-lg p-2 text-muted transition-colors hover:bg-hover cursor-pointer"
            :aria-label="t('terminalInputModal.cancelAria')"
            :disabled="submitting"
            @click="dismiss()"
          >
            <X class="h-4 w-4" />
          </button>
        </header>

        <div v-if="showContext" class="shrink-0 space-y-3 border-b border-border px-5 py-3">
          <div v-if="commandText">
            <p class="mb-1 text-[12px] text-muted">{{ t('terminalInputModal.commandLabel') }}</p>
            <pre
              class="max-h-20 overflow-auto whitespace-pre-wrap break-all rounded-lg bg-background/80 px-3 py-2 font-mono text-[12px] leading-relaxed text-foreground"
            >{{ commandText }}</pre>
          </div>
          <div v-if="outputContext">
            <p class="mb-1 text-[12px] text-muted">{{ t('terminalInputModal.outputLabel') }}</p>
            <pre
              ref="outputEl"
              class="max-h-36 overflow-auto whitespace-pre-wrap break-words rounded-lg bg-background/80 px-3 py-2 font-mono text-[12px] leading-relaxed text-foreground"
            >{{ outputContext }}</pre>
          </div>
        </div>

        <div class="space-y-3 px-5 py-4">
          <input
            v-model="text"
            :type="isSecret ? 'password' : 'text'"
            autocomplete="off"
            class="w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground outline-none focus:border-accent"
            :placeholder="isSecret ? t('terminalInputModal.passwordPlaceholder') : t('terminalInputModal.inputPlaceholder')"
            :disabled="submitting"
          />
          <p v-if="error" class="text-[12px] text-destructive">{{ error }}</p>
          <div class="flex justify-end gap-2">
            <button
              type="button"
              class="rounded-lg px-3 py-1.5 text-sm text-muted hover:bg-hover cursor-pointer"
              :disabled="submitting"
              @click="dismiss()"
            >
              {{ t('terminalInputModal.cancel') }}
            </button>
            <button
              type="button"
              class="rounded-lg bg-accent px-3 py-1.5 text-sm text-accent-foreground hover:opacity-90 cursor-pointer disabled:opacity-50"
              :disabled="submitting"
              @click="submit()"
            >
              {{ t('terminalInputModal.submit') }}
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
