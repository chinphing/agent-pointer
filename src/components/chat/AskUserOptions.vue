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
const submitted = ref(false)
const submitting = ref(false)
const error = ref('')

const isCompleted = computed(
  () => submitted.value || (props.toolCall.status === 'success' && completedSelection.value.length > 0)
)

const displaySelected = computed(() =>
  completedSelection.value.length > 0 ? completedSelection.value : localSelection.value
)

function isSelected(label: string): boolean {
  if (submitted.value) return localSelection.value.includes(label)
  if (completedSelection.value.length > 0) return completedSelection.value.includes(label)
  return localSelection.value.includes(label)
}

async function submit(selected: string[]) {
  if (submitting.value || isCompleted.value) return
  submitting.value = true
  error.value = ''
  try {
    await submitAskUser(props.toolCall.id, selected)
    submitted.value = true
  } catch (cause) {
    error.value = cause instanceof Error ? cause.message : String(cause)
  } finally {
    submitting.value = false
  }
}

function choose(label: string) {
  if (!args.value || submitting.value || isCompleted.value) return
  if (!args.value.multiSelect) {
    localSelection.value = [label]
    void submit([label])
    return
  }
  localSelection.value = localSelection.value.includes(label)
    ? localSelection.value.filter(item => item !== label)
    : [...localSelection.value, label]
}

function confirmMultiple() {
  if (localSelection.value.length > 0) void submit([...localSelection.value])
}
</script>

<template>
  <div v-if="args" class="ml-4 mb-2 max-w-xl space-y-2" @click.stop>
    <p class="text-[13px] leading-5 text-foreground/90">{{ args.question }}</p>

    <div class="grid gap-1.5">
      <button
        v-for="option in args.options"
        :key="option.label"
        type="button"
        class="group w-full min-h-9 px-3 py-2 border rounded-md text-left transition-colors disabled:cursor-default"
        :class="isSelected(option.label)
          ? 'border-border bg-hover text-foreground'
          : 'border-border/70 bg-card hover:bg-hover/80 hover:border-border text-foreground/85'"
        :disabled="submitting || isCompleted"
        @click="choose(option.label)"
      >
        <span class="flex items-start gap-2">
          <span
            class="mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center border text-[10px]"
            :class="[
              args.multiSelect ? 'rounded-[3px]' : 'rounded-full',
              isSelected(option.label)
                ? 'border-foreground/55 bg-foreground/55 text-background'
                : 'border-border bg-background'
            ]"
          >
            <Check v-if="isSelected(option.label)" class="h-3 w-3" stroke-width="3" />
          </span>
          <span class="min-w-0 truncate text-xs leading-4">
            <span class="font-medium">{{ option.label }}</span>
            <span v-if="option.description" class="ml-1.5 text-[11px] text-muted">
              · {{ option.description }}
            </span>
          </span>
        </span>
      </button>
    </div>

    <div v-if="args.multiSelect && !isCompleted" class="flex items-center gap-2">
      <button
        type="button"
        class="h-8 px-3 rounded-md bg-foreground/65 text-background text-xs font-medium hover:bg-foreground/75 disabled:opacity-45 disabled:cursor-not-allowed"
        :disabled="submitting || localSelection.length === 0"
        @click="confirmMultiple"
      >
        <span class="inline-flex items-center gap-1.5">
          <Loader2 v-if="submitting" class="h-3 w-3 animate-spin" />
          确认选择
        </span>
      </button>
      <span class="text-[11px] text-muted">已选 {{ localSelection.length }} 项</span>
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
