<script setup lang="ts">
import { defineAsyncComponent, onMounted, ref } from 'vue'
import AppShell from './components/layout/AppShell.vue'
import ChatView from './components/chat/ChatView.vue'
import ChannelPairingModal from './components/channels/ChannelPairingModal.vue'
import { useChannelPairingPrompt } from './composables/useChannelPairingPrompt'
import { useExternalSkillsImportPrompt } from './composables/useExternalSkillsImportPrompt'
import { useChatStore } from './stores/chat'
import { usePlatformAuthStore } from './stores/platformAuth'
import { useSettingsStore } from './stores/settings'
import { useSkillsStore } from './stores/skills'

/** Lazy: large SFC + many icons; keeps dev / first-paint transform graph small. */
const loadSettingsDialog = () => import('./components/settings/SettingsDialog.vue')
const SettingsDialog = defineAsyncComponent(loadSettingsDialog)
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

onMounted(() => {
  void Promise.all([platformAuth.load(), settings.load()])
    .then(async () => {
      skills.initEnabledFromUserSettings()
      await skills.load()
      await checkExternalSkillsOnBoot()
      await chat.init()
      chat.applyPersistedComposerDefaults()
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
</script>

<template>
  <AppShell
    @open-settings="showSettings = true"
  >
    <ChatView />
  </AppShell>

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
