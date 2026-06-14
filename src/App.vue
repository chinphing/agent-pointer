<script setup lang="ts">
import { defineAsyncComponent, onMounted, ref, watch } from 'vue'
import AppShell from './components/layout/AppShell.vue'
import ChatView from './components/chat/ChatView.vue'
import ComputerCompactBar from './components/chat/ComputerCompactBar.vue'
import ChannelPairingModal from './components/channels/ChannelPairingModal.vue'
import SettingsDialog from './components/settings/SettingsDialog.vue'
import { useComputerCompactMode } from './composables/useComputerCompactMode'
import { useChannelPairingPrompt } from './composables/useChannelPairingPrompt'
import { useExternalSkillsImportPrompt } from './composables/useExternalSkillsImportPrompt'
import { useChatStore } from './stores/chat'
import { usePlatformAuthStore } from './stores/platformAuth'
import { useSettingsStore } from './stores/settings'
import { useSkillsStore } from './stores/skills'
import { isTauriRuntime } from './lib/runtime'

const isDesktopApp = isTauriRuntime()

/** Lazy: large SFC + many icons; keeps dev / first-paint transform graph small. */
const SkillPicker = defineAsyncComponent(() => import('./components/skills/SkillPicker.vue'))
const ExternalSkillsImportModal = defineAsyncComponent(
  () => import('./components/skills/ExternalSkillsImportModal.vue')
)

const chat = useChatStore()
const platformAuth = usePlatformAuthStore()
const settings = useSettingsStore()
const skills = useSkillsStore()

const showSettings = ref(false)
const showSkills = ref(false)
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
})

function onOpenSkillsFromSettings() {
  showSettings.value = false
  showSkills.value = true
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
      <AppShell
        @open-settings="showSettings = true"
      >
        <ChatView />
      </AppShell>
    </template>
  </div>

  <SettingsDialog
    v-if="showSettings"
    @close="showSettings = false"
    @open-skills="onOpenSkillsFromSettings"
  />
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
</template>
