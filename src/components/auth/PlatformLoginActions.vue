<script setup lang="ts">
import { ExternalLink, Loader2 } from 'lucide-vue-next'

withDefaults(
  defineProps<{
    loading: boolean
    error: string | null
    variant?: 'hero' | 'compact'
  }>(),
  { variant: 'hero' }
)

const emit = defineEmits<{ (e: 'login'): void; (e: 'cancel'): void }>()

const heroHintText = '将在系统浏览器中打开授权页面'
</script>

<template>
  <div
    :class="
      variant === 'compact'
        ? 'inline-flex shrink-0 items-center gap-2'
        : 'flex flex-col items-center gap-2'
    "
  >
    <p
      v-if="error"
      class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs leading-snug text-danger"
      :class="variant === 'compact' ? 'sm:max-w-[14rem]' : 'max-w-sm'"
      role="alert"
    >
      {{ error }}
    </p>

    <div
      v-if="variant === 'hero'"
      class="inline-flex flex-col-reverse items-center gap-2"
    >
      <p class="whitespace-nowrap text-center text-[11px] leading-relaxed text-muted">
        {{ loading ? '请在浏览器中完成授权' : heroHintText }}
      </p>
      <div class="flex w-[calc(100%+0.75rem)] items-center justify-center gap-2">
        <button
          type="button"
          class="inline-flex h-10 flex-1 items-center justify-center gap-2 rounded-xl bg-accent px-4 text-sm font-medium text-white cursor-pointer transition-all shadow-sm hover:opacity-95 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50 disabled:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
          :disabled="loading"
          @click="emit('login')"
        >
          <Loader2 v-if="loading" class="h-4 w-4 shrink-0 animate-spin" />
          <ExternalLink v-else class="h-4 w-4 shrink-0 opacity-90" />
          <span>{{ loading ? '等待授权' : '浏览器登录' }}</span>
        </button>
        <button
          v-if="loading"
          type="button"
          class="inline-flex h-10 shrink-0 items-center justify-center rounded-lg border border-border px-4 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors"
          @click="emit('cancel')"
        >
          取消
        </button>
      </div>
    </div>

    <div
      v-else
      class="flex shrink-0 flex-wrap items-center justify-center gap-2"
    >
      <button
        type="button"
        class="inline-flex h-8 items-center justify-center gap-2 rounded-lg bg-accent px-3 text-sm font-medium text-white cursor-pointer transition-all hover:opacity-95 active:scale-[0.98] disabled:pointer-events-none disabled:opacity-50 disabled:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
        :disabled="loading"
        @click="emit('login')"
      >
        <Loader2 v-if="loading" class="h-4 w-4 shrink-0 animate-spin" />
        <ExternalLink v-else class="h-4 w-4 shrink-0 opacity-90" />
        <span>{{ loading ? '等待授权' : '浏览器登录' }}</span>
      </button>
      <button
        v-if="loading"
        type="button"
        class="inline-flex h-8 shrink-0 items-center justify-center rounded-lg border border-border px-3 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors"
        @click="emit('cancel')"
      >
        取消
      </button>
    </div>
  </div>
</template>
