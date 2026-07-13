<script setup lang="ts">
import { ref } from 'vue'
import { Info, ArrowUpRight, Loader2 } from 'lucide-vue-next'
import { useAppUpdater } from '../../../composables/useAppUpdater'
import { isTauriRuntime } from '../../../lib/runtime'

const {
  updateAvailable,
  updateReady,
  updateVersion,
  updateNotes,
  checking,
  updating,
  error,
  statusMessage,
  checkForUpdateManual,
  applyUpdateNow,
  dismissAvailable,
  dismissReady,
  skipVersion,
} = useAppUpdater()

const currentVersion = ref('...')

if (isTauriRuntime()) {
  import('@tauri-apps/api/app')
    .then(({ getVersion }) => getVersion())
    .then((v) => {
      currentVersion.value = v
    })
    .catch(() => {
      currentVersion.value = '0.1.1'
    })
} else {
  fetch('/api/version')
    .then((res) => res.json())
    .then((data) => {
      currentVersion.value = data.version ?? '0.1.1'
    })
    .catch(() => {
      currentVersion.value = '0.1.1'
    })
}

function handleCheckUpdate() {
  void checkForUpdateManual()
}
</script>

<template>
  <section class="space-y-5">
    <div>
      <h3 class="text-sm font-semibold text-foreground">关于 Pointer</h3>
      <p class="mt-1 text-xs text-muted">查看版本信息并检查更新</p>
    </div>

    <div class="rounded-xl border border-border p-4">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2">
          <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center">
            <Info class="w-4 h-4 text-accent" />
          </div>
          <div>
            <p class="text-sm font-medium text-foreground">当前版本</p>
            <p class="text-xs text-muted">v{{ currentVersion }}</p>
          </div>
        </div>

        <button
          v-if="!updateAvailable && !updateReady && !updating"
          class="h-7 px-3 rounded-lg border border-border/60 text-xs text-muted cursor-pointer hover:bg-hover hover:text-foreground transition-colors inline-flex items-center gap-1.5 shrink-0 disabled:opacity-40"
          :disabled="checking"
          @click="handleCheckUpdate"
        >
          <Loader2 v-if="checking" class="w-3.5 h-3.5 animate-spin" />
          <span v-if="checking">检查中…</span>
          <span v-else-if="statusMessage">重新检查</span>
          <span v-else>检查更新</span>
        </button>
      </div>

      <!-- Manual check: new version discovered -->
      <div
        v-if="updateAvailable && !updating"
        class="mt-3 rounded-lg border border-accent/20 bg-accent-muted/15 p-3 space-y-3"
      >
        <div>
          <p class="text-sm font-medium text-foreground">
            发现新版本 v{{ updateVersion }}
          </p>
          <p class="mt-1 text-xs text-muted leading-relaxed">
            将下载并安装，完成后自动重新打开应用
          </p>
          <p v-if="updateNotes" class="mt-2 text-xs text-muted leading-relaxed line-clamp-3">
            {{ updateNotes }}
          </p>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <button
            class="h-8 px-4 rounded-lg bg-accent text-white text-xs font-medium cursor-pointer hover:opacity-95 transition-opacity"
            @click="applyUpdateNow"
          >
            立即更新
          </button>
          <button
            class="h-8 px-3 rounded-lg bg-hover text-foreground text-xs cursor-pointer hover:bg-hover/80 transition-colors"
            @click="dismissAvailable"
          >
            稍后
          </button>
          <button
            class="h-8 px-3 rounded-lg text-muted text-xs cursor-pointer hover:text-foreground transition-colors"
            @click="skipVersion"
          >
            跳过此版本
          </button>
        </div>
      </div>

      <!-- Background ready or manual updating -->
      <div
        v-else-if="updateReady && !updating"
        class="mt-3 rounded-lg border border-accent/20 bg-accent-muted/15 p-3 space-y-3"
      >
        <div>
          <p class="text-sm font-medium text-foreground">
            新版本 v{{ updateVersion }} 已就绪
          </p>
          <p class="mt-1 text-xs text-muted leading-relaxed">
            安装完成后将自动重新打开应用
          </p>
          <p v-if="updateNotes" class="mt-2 text-xs text-muted leading-relaxed line-clamp-3">
            {{ updateNotes }}
          </p>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <button
            class="h-8 px-4 rounded-lg bg-accent text-white text-xs font-medium cursor-pointer hover:opacity-95 transition-opacity"
            @click="applyUpdateNow"
          >
            立即更新
          </button>
          <button
            class="h-8 px-3 rounded-lg bg-hover text-foreground text-xs cursor-pointer hover:bg-hover/80 transition-colors"
            @click="dismissReady"
          >
            稍后
          </button>
          <button
            class="h-8 px-3 rounded-lg text-muted text-xs cursor-pointer hover:text-foreground transition-colors"
            @click="skipVersion"
          >
            跳过此版本
          </button>
        </div>
      </div>

      <!-- Updating in progress -->
      <div
        v-else-if="updating"
        class="mt-3 rounded-lg border border-accent/20 bg-accent-muted/15 px-3 py-2.5 flex items-start gap-2"
      >
        <Loader2 class="w-3.5 h-3.5 animate-spin text-accent shrink-0 mt-0.5" />
        <div>
          <p class="text-xs font-medium text-foreground">正在更新 v{{ updateVersion }}…</p>
          <p class="mt-0.5 text-xs text-muted">请勿关闭应用，完成后将自动重新打开</p>
        </div>
      </div>

      <div
        v-else-if="statusMessage && !error"
        class="mt-3 flex items-center justify-between rounded-lg bg-accent-muted/15 px-3 py-2 text-xs text-muted"
      >
        <span>{{ statusMessage }}</span>
      </div>

      <div
        v-if="error"
        class="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-red-700"
      >
        {{ error }}
        <a
          href="https://pointer-app.readflowai.com/download"
          target="_blank"
          class="ml-1 inline-flex items-center gap-0.5 underline underline-offset-2"
        >
          前往官网<ArrowUpRight class="w-3 h-3" />
        </a>
      </div>
    </div>
  </section>
</template>
