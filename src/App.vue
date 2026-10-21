<script setup lang="ts">
import { computed, defineAsyncComponent, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import AppShell from './components/layout/AppShell.vue'
import ChatView from './components/chat/ChatView.vue'
import { useChannelPairingPrompt } from './composables/useChannelPairingPrompt'
import { useExternalSkillsImportPrompt } from './composables/useExternalSkillsImportPrompt'
import { useAppUpdater } from './composables/useAppUpdater'
import UpdateReadyBanner from './components/updater/UpdateReadyBanner.vue'
import { useChatStore } from './stores/chat'
import { usePlatformAuthStore } from './stores/platformAuth'
import { useSettingsStore } from './stores/settings'
import { useSkillsStore } from './stores/skills'
import { isTauriRuntime } from './lib/runtime'
import { migrateLegacyTurnElapsedOnce } from './lib/turnElapsedMigration'

const isDesktopApp = isTauriRuntime()

/** Lazy: keep first paint free of settings / modal / skills graphs. */
const SettingsView = defineAsyncComponent(
  () => import('./components/settings/SettingsDialog.vue')
)
const ChannelPairingModal = defineAsyncComponent(
  () => import('./components/channels/ChannelPairingModal.vue')
)
const TerminalInputModal = defineAsyncComponent(
  () => import('./components/chat/TerminalInputModal.vue')
)
const ExternalSkillsImportModal = defineAsyncComponent(
  () => import('./components/skills/ExternalSkillsImportModal.vue')
)

const chat = useChatStore()
const { terminalInputRequest } = storeToRefs(chat)
const platformAuth = usePlatformAuthStore()
const settings = useSettingsStore()
const skills = useSkillsStore()

const showSettings = ref(false)
const isAppAdmin = computed(
  () => platformAuth.isPlatformAdmin || settings.isPlatformAdmin
)
const settingsInitialSection = ref('assistant')
const {
  open: pairingModalOpen,
  pendingItem: pairingModalPending,
  dismiss: dismissPairingModal,
  onApproved: onPairingApproved
} = useChannelPairingPrompt()

const {
  open: externalSkillsOpen,
  sources: externalSkillSources,
  totalSkills: externalSkillsTotal,
  importing: externalSkillsImporting,
  checkOnBoot: checkExternalSkillsOnBoot,
  dismiss: dismissExternalSkills,
  importSelected: importExternalSkillsSelected
} = useExternalSkillsImportPrompt()


const {
  updateReady,
  updateVersion,
  updateNotes,
  applyUpdateNow,
  dismissReady,
  skipVersion,
} = useAppUpdater()

onMounted(() => {
  void Promise.all([platformAuth.load(), settings.load()])
    .then(async () => {
      skills.initEnabledFromUserSettings()
      await skills.load()
      await checkExternalSkillsOnBoot()
      await chat.init()
      // One-shot, failure-safe: fold the legacy localStorage turn-elapsed blob
      // into `messages.turn_elapsed_ms`, then drop the key. Runs after the
      // first conversation load so the batch never competes with it for the
      // store's write connection; fire-and-forget so a slow batch (19k legacy
      // turns) never delays chat boot.
      void migrateLegacyTurnElapsedOnce()
      if (chat.currentId) {
        void chat.refreshTaskBoard(chat.currentId)
        void chat.refreshSubAgentTaskBoards(chat.currentId)
      }
    })
    .catch(e => console.error('[app boot]', e))

  if (!isDesktopApp) {
    watch(
      () => platformAuth.session.logged_in,
      (loggedIn, wasLoggedIn) => {
        if (wasLoggedIn && !loggedIn) {
          chat.resetForPlatformLogout()
        }
      }
    )
  }
})

function openSettings(section = 'assistant') {
  if (!isAppAdmin.value) {
    console.warn('[app] settings stayed closed: sign in before the settings view can load')
    return
  }
  settingsInitialSection.value = section
  showSettings.value = true
}

function openAutomation() {
  openSettings('automation')
}

</script>

<template>
  <div class="h-full w-full min-h-0 flex flex-col">
      <SettingsView
        v-if="showSettings"
        class="min-h-0 flex-1 overflow-hidden"
        :initial-section="settingsInitialSection"
        @close="showSettings = false"
      />
      <AppShell
        v-else
        class="min-h-0 flex-1 overflow-hidden"
        @open-settings="openSettings"
        @open-automation="openAutomation"
      >
        <ChatView />
      </AppShell>
  </div>

  <ChannelPairingModal
    v-model:open="pairingModalOpen"
    :pending="pairingModalPending"
    @approved="onPairingApproved"
    @dismiss="dismissPairingModal"
  />

  <ExternalSkillsImportModal
    v-model:open="externalSkillsOpen"
    :sources="externalSkillSources"
    :total-skills="externalSkillsTotal"
    :importing="externalSkillsImporting"
    @dismiss="dismissExternalSkills"
    @import="ids => importExternalSkillsSelected(ids, () => skills.load({ rescan: true }))"
  />

  <TerminalInputModal
    v-if="terminalInputRequest"
    :request="terminalInputRequest"
    @close="chat.dismissTerminalInputModal()"
  />

  <UpdateReadyBanner
    v-if="updateReady"
    :version="updateVersion"
    :notes="updateNotes"
    @apply="applyUpdateNow"
    @dismiss="dismissReady"
    @skip-version="skipVersion"
  />
</template>
