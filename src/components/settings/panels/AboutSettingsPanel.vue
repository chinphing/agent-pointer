<script setup lang="ts">
import { ref } from 'vue'
import { Info, ArrowUpRight, Loader2 } from 'lucide-vue-next'
import { useAppUpdater } from '../../../composables/useAppUpdater'
import { isTauriRuntime } from '../../../lib/runtime'

const { updateVersion, checking, downloading, error, checkAndDownload } = useAppUpdater()

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
  currentVersion.value = '—'
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

    <div class="rounded-xl border border-border p-4 space-y-3">
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
      </div>

      <div
        v-if="error"
        class="rounded-lg bg-red-50 px-3 py-2 text-xs text-red-700"
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

      <button
        class="h-9 w-full rounded-lg bg-accent text-white text-sm font-medium cursor-pointer hover:opacity-95 disabled:opacity-50 transition-opacity inline-flex items-center justify-center gap-2"
        :disabled="checking || downloading"
        @click="handleCheckUpdate"
      >
        <Loader2 v-if="checking || downloading" class="w-4 h-4 animate-spin" />
        <span v-if="checking">正在检查更新…</span>
        <span v-else-if="downloading">正在下载新版本 {{ updateVersion }}…</span>
        <span v-else>检查更新</span>
      </button>
    </div>
  </section>
</template>
