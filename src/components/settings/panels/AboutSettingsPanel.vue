<script setup lang="ts">
import { ref } from 'vue'
import { Info, ArrowUpRight, Loader2, RotateCw } from 'lucide-vue-next'
import { useAppUpdater } from '../../../composables/useAppUpdater'
import { isTauriRuntime } from '../../../lib/runtime'

const { updateReady, updateVersion, checking, downloading, error, checkAndDownload, relaunch } = useAppUpdater()

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
  void checkAndDownload()
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
          v-if="updateReady"
          class="h-7 px-3 rounded-lg bg-accent text-white text-xs font-medium cursor-pointer hover:opacity-95 transition-opacity inline-flex items-center gap-1.5 shrink-0"
          @click="relaunch"
        >
          <RotateCw class="w-3.5 h-3.5" />
          <span>立即重启 {{ updateVersion }}</span>
        </button>
        <button
          v-else
          class="h-7 px-3 rounded-lg border border-border/60 text-xs text-muted cursor-pointer hover:bg-hover hover:text-foreground transition-colors inline-flex items-center gap-1.5 shrink-0 disabled:opacity-40"
          :disabled="checking || downloading"
          @click="handleCheckUpdate"
        >
          <Loader2 v-if="checking || downloading" class="w-3.5 h-3.5 animate-spin" />
          <span v-if="checking">检查中…</span>
          <span v-else-if="downloading">下载中 {{ updateVersion }}…</span>
          <span v-else>检查更新</span>
        </button>
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
