<script setup lang="ts">
import { computed, ref } from 'vue'
import { Check, Loader2 } from 'lucide-vue-next'
import type { ToolCall } from '../../types/chat'
import { submitAskUser } from '../../lib/api'
import { parseAskUserArgs, parseAskUserSelection } from '../../lib/askUser'

const props = defineProps<{ toolCall: ToolCall }>()

const args = computed(() => parseAskUserArgs(props.toolCall.arguments))
const completedSelection = computed(() => parseAskUserSelection(props.toolCall.result))
const localSelection = ref<string[]>([])
const otherText = ref('')
const otherFocused = ref(false)
const composing = ref(false)
const submitted = ref(false)
const submitting = ref(false)
const error = ref('')

const optionLabels = computed(
  () => new Set((args.value?.options ?? []).map(option => option.label))
)

const isCompleted = computed(
  () => submitted.value || (props.toolCall.status === 'success' && completedSelection.value.length > 0)
)

const displaySelected = computed(() =>
  completedSelection.value.length > 0 ? completedSelection.value : localSelection.value
)

const freeTextSelected = computed(() =>
  displaySelected.value.find(value => !optionLabels.value.has(value)) ?? ''
)

/** Hermes: Other looks selected when focused, has text, or completed via free text. */
const otherActive = computed(() => {
  if (isCompleted.value) return !!freeTextSelected.value
  return otherFocused.value || otherText.value.trim().length > 0
})

function isSelected(label: string): boolean {
  if (submitted.value) return localSelection.value.includes(label)
  if (completedSelection.value.length > 0) return completedSelection.value.includes(label)
  return localSelection.value.includes(label)
}

async function submit(selected: string[]) {
  if (submitting.value || isCompleted.value) return
  const cleaned = selected.map(item => item.trim()).filter(Boolean)
  if (cleaned.length === 0) {
    error.value = '请输入内容'
    return
  }
  submitting.value = true
  error.value = ''
  try {
    await submitAskUser(props.toolCall.id, cleaned)
    submitted.value = true
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    submitting.value = false
  }
}

function choose(label: string) {
  if (!args.value || submitting.value || isCompleted.value) return
  otherText.value = ''
  otherFocused.value = false
  if (!args.value.multiSelect) {
    localSelection.value = [label]
    void submit([label])
    return
  }
  localSelection.value = localSelection.value.includes(label)
    ? localSelection.value.filter(item => item !== label)
    : [...localSelection.value, label]
}

/** Hermes: focusing Other deselects listed choices (single-select). */
function onOtherFocus() {
  if (isCompleted.value) return
  otherFocused.value = true
  if (!args.value?.multiSelect) localSelection.value = []
}

function onOtherBlur() {
  otherFocused.value = false
}

function onOtherInput(event: Event) {
  otherText.value = (event.target as HTMLInputElement).value
  if (!args.value?.multiSelect) localSelection.value = []
}

function confirmOtherSingle() {
  const text = otherText.value.trim()
  if (!text) {
    error.value = '请输入内容'
    return
  }
  localSelection.value = [text]
  void submit([text])
}

function confirmMultiple() {
  const selected = [...localSelection.value]
  const text = otherText.value.trim()
  if (text && !selected.includes(text)) selected.push(text)
  if (selected.length === 0) {
    error.value = '请选择选项或输入其他内容'
    return
  }
  void submit(selected)
}

function onOtherEnter(event: KeyboardEvent) {
  // IME composition: Enter confirms the candidate — never submit from it.
  if (event.isComposing || composing.value) return
  event.preventDefault()
  if (args.value?.multiSelect) confirmMultiple()
  else confirmOtherSingle()
}

const canConfirmMultiple = computed(
  () => localSelection.value.length > 0 || otherText.value.trim().length > 0
)
</script>

<template>
  <div v-if="args" class="ml-4 mb-2 max-w-xl min-w-0 space-y-2" @click.stop>
    <p class="text-[13px] leading-5 break-words text-foreground/90">{{ args.question }}</p>

    <div class="grid gap-0 min-w-0">
      <button
        v-for="option in args.options"
        :key="option.label"
        type="button"
        class="group w-full min-w-0 min-h-8 px-3 py-1.5 rounded-md text-left transition-colors disabled:cursor-default"
        :class="isSelected(option.label)
          ? 'text-foreground'
          : 'hover:bg-hover/30 text-foreground/85'"
        :disabled="submitting || isCompleted"
        @click="choose(option.label)"
      >
        <span class="flex min-w-0 items-start gap-2">
          <span
            class="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center text-[10px]"
            :class="[
              args.multiSelect ? 'rounded-[3px]' : 'rounded-full',
              isSelected(option.label)
                ? 'bg-foreground/55 text-background'
                : 'ring-1 ring-foreground/25 ring-inset'
            ]"
          >
            <Check v-if="isSelected(option.label)" class="h-3 w-3" stroke-width="3" />
          </span>
          <span class="min-w-0 flex-1 whitespace-normal break-words text-xs leading-4">
            <span class="font-medium">{{ option.label }}</span>
            <span v-if="option.description" class="ml-1.5 text-[11px] text-muted">
              · {{ option.description }}
            </span>
          </span>
        </span>
      </button>

      <!-- Hermes: Other is always an inline field; Enter confirms. -->
      <div
        class="flex min-w-0 items-center gap-2 min-h-8 px-3 py-1.5 rounded-md"
        :class="otherActive ? 'text-foreground' : 'text-foreground/85'"
      >
        <span
          class="flex h-4 w-4 shrink-0 items-center justify-center text-[10px]"
          :class="[
            args.multiSelect ? 'rounded-[3px]' : 'rounded-full',
            otherActive
              ? 'bg-foreground/55 text-background'
              : 'ring-1 ring-foreground/25 ring-inset'
          ]"
        >
          <Check v-if="otherActive" class="h-3 w-3" stroke-width="3" />
        </span>
        <span class="shrink-0 text-xs leading-4 font-medium">其他</span>
        <input
          type="text"
          class="h-7 w-full max-w-[14rem] min-w-0 rounded-md border border-border bg-transparent px-2 text-xs text-foreground placeholder:text-muted focus:outline-none focus:ring-1 focus:ring-foreground/25 disabled:opacity-80"
          :placeholder="isCompleted ? '' : '请输入其他选项'"
          :value="isCompleted ? freeTextSelected : otherText"
          :disabled="submitting || isCompleted"
          :readonly="isCompleted"
          @click.stop
          @focus="onOtherFocus"
          @blur="onOtherBlur"
          @input="onOtherInput"
          @keydown.enter="onOtherEnter"
          @compositionstart="composing = true"
          @compositionend="composing = false"
        >
      </div>
    </div>

    <div v-if="args.multiSelect && !isCompleted" class="flex items-center gap-2">
      <button
        type="button"
        class="h-8 px-3 rounded-md bg-foreground/65 text-background text-xs font-medium hover:bg-foreground/75 disabled:opacity-45 disabled:cursor-not-allowed"
        :disabled="submitting || !canConfirmMultiple"
        @click="confirmMultiple"
      >
        <span class="inline-flex items-center gap-1.5">
          <Loader2 v-if="submitting" class="h-3 w-3 animate-spin" />
          确认选择
        </span>
      </button>
      <span class="text-[11px] text-muted">
        已选 {{ localSelection.length + (otherText.trim() ? 1 : 0) }} 项
      </span>
    </div>

    <p v-else-if="submitting" class="inline-flex items-center gap-1.5 text-[11px] text-muted">
      <Loader2 class="h-3 w-3 animate-spin" />正在提交选择
    </p>
    <p v-else-if="isCompleted" class="text-[11px] text-muted truncate" :title="'已选择：' + displaySelected.join('、')">
      已选择：{{ displaySelected.join('、') }}
    </p>
    <p v-if="error" class="text-[11px] leading-4 text-danger">{{ error }}</p>
  </div>
</template>
