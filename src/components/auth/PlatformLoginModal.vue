<script setup lang="ts">
import { Bot, ExternalLink, Loader2 } from 'lucide-vue-next'

defineProps<{
  loading: boolean
  error: string | null
}>()

const emit = defineEmits<{ (e: 'login'): void; (e: 'cancel'): void }>()

</script>

<template>
  <Transition name="platform-login-scrim" appear>
    <div
      class="platform-login-scrim fixed inset-0 z-50 flex items-center justify-center p-4"
      role="dialog"
      aria-modal="true"
      aria-labelledby="platform-login-title"
    >
      <Transition name="platform-login-panel" appear>
        <div
          class="platform-login-panel relative w-fit max-w-[calc(100vw-2rem)] overflow-hidden rounded-2xl border border-border bg-card shadow-2xl"
        >
          <div
            class="pointer-events-none absolute -top-12 right-2 h-24 w-32 rounded-full bg-accent/10 blur-3xl"
            aria-hidden="true"
          />

          <div class="relative flex flex-col gap-4 px-5 py-5">
            <div class="flex w-full items-center gap-3">
              <div class="flex items-center gap-3">
                <div
                  class="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg border border-border bg-accent/10"
                >
                  <Bot class="h-5 w-5 text-accent" />
                </div>
                <h2
                  id="platform-login-title"
                  class="text-[17px] font-semibold tracking-wide brand-text"
                >
                  Pointer
                </h2>
              </div>
            </div>

            <div class="login-actions flex w-full flex-col gap-2">
              <Transition name="platform-login-alert">
                <p
                  v-if="error"
                  class="rounded-lg border border-danger/30 bg-danger/10 px-3 py-1.5 text-xs leading-snug text-danger"
                  role="alert"
                >
                  {{ error }}
                </p>
              </Transition>

              <div class="flex w-full flex-wrap gap-2">
                <button
                  type="button"
                  class="inline-flex flex-1 min-w-[8rem] items-center justify-center gap-2 rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white cursor-pointer transition-all hover:opacity-95 active:scale-[0.98] disabled:opacity-50 disabled:pointer-events-none disabled:active:scale-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 focus-visible:ring-offset-2 focus-visible:ring-offset-card"
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
                  class="inline-flex shrink-0 items-center justify-center rounded-lg border border-border px-4 py-2 text-sm text-foreground hover:bg-hover cursor-pointer transition-colors"
                  @click="emit('cancel')"
                >
                  取消
                </button>
              </div>
            </div>
          </div>
        </div>
      </Transition>
    </div>
  </Transition>
</template>

<style scoped>
.platform-login-scrim {
  background: hsl(var(--foreground) / 0.32);
  backdrop-filter: blur(4px);
}

.platform-login-scrim-enter-active,
.platform-login-scrim-leave-active {
  transition: opacity 0.22s ease;
}
.platform-login-scrim-enter-from,
.platform-login-scrim-leave-to {
  opacity: 0;
}

.platform-login-panel-enter-active {
  transition:
    opacity 0.28s ease,
    transform 0.28s cubic-bezier(0.16, 1, 0.3, 1);
}
.platform-login-panel-leave-active {
  transition:
    opacity 0.18s ease,
    transform 0.18s ease;
}
.platform-login-panel-enter-from,
.platform-login-panel-leave-to {
  opacity: 0;
  transform: scale(0.96) translateY(10px);
}

.platform-login-alert-enter-active,
.platform-login-alert-leave-active {
  transition: opacity 0.2s ease, transform 0.2s ease;
}
.platform-login-alert-enter-from,
.platform-login-alert-leave-to {
  opacity: 0;
  transform: translateY(-4px);
}

@media (prefers-reduced-motion: reduce) {
  .platform-login-scrim-enter-active,
  .platform-login-scrim-leave-active,
  .platform-login-panel-enter-active,
  .platform-login-panel-leave-active,
  .platform-login-alert-enter-active,
  .platform-login-alert-leave-active {
    transition: none;
  }
  .platform-login-panel-enter-from,
  .platform-login-panel-leave-to {
    transform: none;
  }
}
</style>
