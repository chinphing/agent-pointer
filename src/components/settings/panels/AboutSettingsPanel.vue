<script setup lang="ts">
import { ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { Info, ArrowUpRight, Loader2 } from 'lucide-vue-next'
import { useAppUpdater } from '../../../composables/useAppUpdater'
import { isTauriRuntime } from '../../../lib/runtime'
import { APP_VERSION } from '../../../lib/appVersion'
import { openExternalUrl } from '../../../lib/openExternalUrl'
import { isCommunityEdition } from '../../../lib/platformUrls'

const { t } = useI18n()

const DOWNLOAD_URL = (() => {
  const fromEnv = String(
    (import.meta as ImportMeta & { env?: Record<string, string> }).env?.VITE_POINTER_DOWNLOAD_URL ??
      ''
  ).trim()
  if (fromEnv) return fromEnv
  if (isCommunityEdition()) return ''
  return 'https://pointer-app.readflowai.com/download'
})()

const showUpdater = !isCommunityEdition() && isTauriRuntime()

function openDownloadPage(e: MouseEvent) {
  e.preventDefault()
  if (!DOWNLOAD_URL) return
  void openExternalUrl(DOWNLOAD_URL)
}

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

const currentVersion = ref(APP_VERSION)

if (isTauriRuntime()) {
  import('@tauri-apps/api/app')
    .then(({ getVersion }) => getVersion())
    .then((v) => {
      currentVersion.value = v
    })
    .catch(() => {
      currentVersion.value = APP_VERSION
    })
} else {
  fetch('/api/version')
    .then((res) => res.json())
    .then((data) => {
      currentVersion.value = data.version ?? APP_VERSION
    })
    .catch(() => {
      currentVersion.value = APP_VERSION
    })
}

function handleCheckUpdate() {
  void checkForUpdateManual()
}
</script>

<template>
  <section class="space-y-5">
    <div class="rounded-xl border border-border p-4">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2">
          <div class="w-8 h-8 rounded-lg bg-accent/15 flex items-center justify-center">
            <Info class="w-4 h-4 text-accent" />
          </div>
          <div>
            <p class="text-sm font-medium text-foreground">{{ t('settings.about.currentVersion') }}</p>
            <p class="text-xs text-muted">v{{ currentVersion }}</p>
          </div>
        </div>

        <button
          v-if="showUpdater && !updateAvailable && !updateReady && !updating"
          class="h-7 px-3 rounded-lg border border-border/60 text-xs text-muted cursor-pointer hover:bg-hover hover:text-foreground transition-colors inline-flex items-center gap-1.5 shrink-0 disabled:opacity-40"
          :disabled="checking"
          @click="handleCheckUpdate"
        >
          <Loader2 v-if="checking" class="w-3.5 h-3.5 animate-spin" />
          <span v-if="checking">{{ t('settings.about.checking') }}</span>
          <span v-else-if="statusMessage">{{ t('settings.about.recheck') }}</span>
          <span v-else>{{ t('settings.about.checkUpdate') }}</span>
        </button>
      </div>

      <!-- Manual check: new version discovered -->
      <div
        v-if="updateAvailable && !updating"
        class="mt-3 rounded-lg border border-accent/20 bg-accent-muted/15 p-3 space-y-3"
      >
        <div>
          <p class="text-sm font-medium text-foreground">
            {{ t('settings.about.newVersion', { version: updateVersion }) }}
          </p>
          <p class="mt-1 text-xs text-muted leading-relaxed">
            {{ t('settings.about.willInstall') }}
          </p>
          <p v-if="updateNotes" class="mt-2 text-xs text-muted leading-relaxed line-clamp-3">
            {{ updateNotes }}
          </p>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <button
            class="h-8 px-4 rounded-lg bg-accent text-accent-foreground text-xs font-medium cursor-pointer hover:opacity-95 transition-opacity"
            @click="applyUpdateNow"
          >
            {{ t('settings.about.updateNow') }}
          </button>
          <button
            class="h-8 px-3 rounded-lg bg-hover text-foreground text-xs cursor-pointer hover:bg-hover/80 transition-colors"
            @click="dismissAvailable"
          >
            {{ t('settings.about.later') }}
          </button>
          <button
            class="h-8 px-3 rounded-lg text-muted text-xs cursor-pointer hover:text-foreground transition-colors"
            @click="skipVersion"
          >
            {{ t('settings.about.skipVersion') }}
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
            {{ t('settings.about.readyVersion', { version: updateVersion }) }}
          </p>
          <p class="mt-1 text-xs text-muted leading-relaxed">
            {{ t('settings.about.willRelaunch') }}
          </p>
          <p v-if="updateNotes" class="mt-2 text-xs text-muted leading-relaxed line-clamp-3">
            {{ updateNotes }}
          </p>
        </div>
        <div class="flex flex-wrap items-center gap-2">
          <button
            class="h-8 px-4 rounded-lg bg-accent text-accent-foreground text-xs font-medium cursor-pointer hover:opacity-95 transition-opacity"
            @click="applyUpdateNow"
          >
            {{ t('settings.about.updateNow') }}
          </button>
          <button
            class="h-8 px-3 rounded-lg bg-hover text-foreground text-xs cursor-pointer hover:bg-hover/80 transition-colors"
            @click="dismissReady"
          >
            {{ t('settings.about.later') }}
          </button>
          <button
            class="h-8 px-3 rounded-lg text-muted text-xs cursor-pointer hover:text-foreground transition-colors"
            @click="skipVersion"
          >
            {{ t('settings.about.skipVersion') }}
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
          <p class="text-xs font-medium text-foreground">{{ t('settings.about.updating', { version: updateVersion }) }}</p>
          <p class="mt-0.5 text-xs text-muted">{{ t('settings.about.dontClose') }}</p>
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
          v-if="DOWNLOAD_URL"
          :href="DOWNLOAD_URL"
          class="ml-1 inline-flex items-center gap-0.5 underline underline-offset-2"
          @click="openDownloadPage"
        >
          {{ t('settings.about.goWebsite') }}<ArrowUpRight class="w-3 h-3" />
        </a>
      </div>
    </div>
  </section>
</template>
