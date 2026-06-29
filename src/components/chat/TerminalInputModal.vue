<script setup lang="ts">
import { computed, onUnmounted, ref } from 'vue'
import { KeyRound, Terminal, X } from 'lucide-vue-next'
import { dismissTerminalInput, submitTerminalInput } from '../../lib/api'
import type { TerminalInputRequest } from '../../types/chat'

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
const title = computed(() => (isSecret.value ? '需要密码' : '命令需要输入'))
const hint = computed(
  () =>
    props.request.inputHint?.trim() ||
    (isSecret.value ? '请输入 SSH / 远程登录密码' : '请输入命令需要的回复')
)

async function submit() {
  const value = text.value
  if (!value.trim()) {
    error.value = '请输入内容'
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
      class="fixed inset-0 z-[250] flex items-center justify-center bg-black/60 p-4"
      role="dialog"
      aria-modal="true"
      :aria-label="title"
      @click.self="dismiss()"
    >
      <div
        class="relative flex w-full max-w-lg flex-col overflow-hidden rounded-2xl border border-border bg-[hsl(var(--card-elevated))] shadow-2xl"
      >
        <header class="flex items-start gap-3 border-b border-border px-5 py-4 pr-14">
          <div
            class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg"
            :class="isSecret ? 'bg-warning/15' : 'bg-accent/15'"
          >
            <KeyRound v-if="isSecret" class="h-4 w-4 text-warning" />
            <Terminal v-else class="h-4 w-4 text-accent" />
          </div>
          <div class="min-w-0 flex-1">
            <h2 class="text-base font-semibold text-foreground">{{ title }}</h2>
            <p class="mt-1 text-[12px] leading-relaxed text-muted">{{ hint }}</p>
            <p v-if="isSecret" class="mt-1 text-[11px] text-warning/90">
              Agent 无法代填密码；取消后请重新运行该命令。
            </p>
          </div>
          <button
            type="button"
            class="absolute top-4 right-4 rounded-lg p-2 text-muted transition-colors hover:bg-hover cursor-pointer"
            aria-label="取消"
            :disabled="submitting"
            @click="dismiss()"
          >
            <X class="h-4 w-4" />
          </button>
        </header>

        <div class="space-y-3 px-5 py-4">
          <input
            v-model="text"
            :type="isSecret ? 'password' : 'text'"
            autocomplete="off"
            class="w-full rounded-lg border border-border bg-background px-3 py-2 text-sm text-foreground outline-none focus:border-accent"
            :placeholder="isSecret ? '密码' : '输入…'"
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
              取消
            </button>
            <button
              type="button"
              class="rounded-lg bg-accent px-3 py-1.5 text-sm text-accent-foreground hover:opacity-90 cursor-pointer disabled:opacity-50"
              :disabled="submitting"
              @click="submit()"
            >
              提交
            </button>
          </div>
        </div>
      </div>
    </div>
  </Teleport>
</template>
