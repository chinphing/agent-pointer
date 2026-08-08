<script setup lang="ts">
import { defineAsyncComponent, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import AppShell from './components/layout/AppShell.vue'
import ChatView from './components/chat/ChatView.vue'
import ComputerCompactBar from './components/chat/ComputerCompactBar.vue'
import { useComputerCompactMode } from './composables/useComputerCompactMode'
import { useChannelPairingPrompt } from './composables/useChannelPairingPrompt'
import { useExternalSkillsImportPrompt } from './composables/useExternalSkillsImportPrompt'
import { useAppUpdater } from './composables/useAppUpdater'
import UpdateReadyBanner from './components/updater/UpdateReadyBanner.vue'
import { useChatStore } from './stores/chat'
import { usePlatformAuthStore } from './stores/platformAuth'
import { useSettingsStore } from './stores/settings'
import { useSkillsStore } from './stores/skills'
import { isTauriRuntime } from './lib/runtime'

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
const SkillPicker = defineAsyncComponent(() => import('./components/skills/SkillPicker.vue'))
const ExternalSkillsImportModal = defineAsyncComponent(
  () => import('./components/skills/ExternalSkillsImportModal.vue')
)

const chat = useChatStore()
const { terminalInputRequest } = storeToRefs(chat)
const platformAuth = usePlatformAuthStore()
const settings = useSettingsStore()
const skills = useSkillsStore()

const showSettings = ref(false)
const showSkills = ref(false)
const settingsInitialSection = ref('account')
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
  isCompact,
  planSummary,
  planLine,
  statusLine,
  twoLines,
  expand: expandComputerCompact,
  stop: stopComputerCompact
} = useComputerCompactMode()

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

function onOpenSkillsFromSettings() {
  showSettings.value = false
  showSkills.value = true
}

function openSettings(section = 'account') {
  settingsInitialSection.value = section
  showSettings.value = true
}

function openAutomation() {
  openSettings('automation')
}

watch(showSettings, open => {
  if (open && isCompact.value) expandComputerCompact()
})

watch(showSkills, open => {
  if (open && isCompact.value) expandComputerCompact()
})
</script>

<template>
  <div class="h-full w-full min-h-0 flex flex-col">
  <ComputerCompactBar
    v-if="isCompact"
    :class="isDesktopApp
      ? 'h-full w-full min-h-0'
      : 'fixed bottom-4 right-4 z-[500] w-[min(400px,calc(100vw-32px))] shadow-lg'"
    :plan-summary="planSummary"
    :plan-line="planLine"
    :status-line="statusLine"
    :two-lines="twoLines"
    @expand="expandComputerCompact"
    @stop="stopComputerCompact"
  />
    <template v-else>
      <SettingsView
        v-if="showSettings"
        :initial-section="settingsInitialSection"
        @close="showSettings = false"
        @open-skills="onOpenSkillsFromSettings"
      />
      <AppShell
        v-else
        @open-settings="openSettings"
        @open-automation="openAutomation"
        @open-skills="showSkills = true"
      >
        <ChatView />
      </AppShell>
    </template>
  </div>

  <SkillPicker v-if="showSkills" @close="showSkills = false" />

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
