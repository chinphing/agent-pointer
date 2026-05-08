<script setup lang="ts">
import { onMounted, ref } from 'vue'
import AppShell from './components/layout/AppShell.vue'
import ChatView from './components/chat/ChatView.vue'
import SettingsDialog from './components/settings/SettingsDialog.vue'
import SkillPicker from './components/skills/SkillPicker.vue'
import { useChatStore } from './stores/chat'
import { useSettingsStore } from './stores/settings'
import { useSkillsStore } from './stores/skills'

const chat = useChatStore()
const settings = useSettingsStore()
const skills = useSkillsStore()

const showSettings = ref(false)
const showSkills = ref(false)

onMounted(async () => {
  await Promise.all([settings.load(), skills.load(), chat.init()])
  if (!settings.settings.hasKey) showSettings.value = true
})
</script>

<template>
  <AppShell
    @open-settings="showSettings = true"
    @open-skills="showSkills = true"
  >
    <ChatView />
  </AppShell>

  <SettingsDialog v-if="showSettings" @close="showSettings = false" />
  <SkillPicker v-if="showSkills" @close="showSkills = false" />
</template>
