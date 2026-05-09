<script setup lang="ts">
import { defineAsyncComponent, onMounted, ref } from 'vue'
import AppShell from './components/layout/AppShell.vue'
import ChatView from './components/chat/ChatView.vue'
import { useChatStore } from './stores/chat'
import { useSettingsStore } from './stores/settings'
import { useSkillsStore } from './stores/skills'

/** Lazy: large SFC + many icons; keeps dev / first-paint transform graph small. */
const loadSettingsDialog = () => import('./components/settings/SettingsDialog.vue')
const SettingsDialog = defineAsyncComponent(loadSettingsDialog)

const chat = useChatStore()
const settings = useSettingsStore()
const skills = useSkillsStore()

const showSettings = ref(false)

onMounted(async () => {
  void loadSettingsDialog()
  await Promise.all([settings.load(), skills.load(), chat.init()])
  if (!settings.settings.hasKey) showSettings.value = true
})
</script>

<template>
  <AppShell
    @open-settings="showSettings = true"
  >
    <ChatView />
  </AppShell>

  <SettingsDialog v-if="showSettings" @close="showSettings = false" />
</template>
