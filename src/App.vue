<script setup lang="ts">
import { defineAsyncComponent, onMounted, ref } from 'vue'
import AppShell from './components/layout/AppShell.vue'
import PlatformLoginModal from './components/auth/PlatformLoginModal.vue'
import ChatView from './components/chat/ChatView.vue'
import { useChatStore } from './stores/chat'
import { usePlatformAuthStore } from './stores/platformAuth'
import { useSettingsStore } from './stores/settings'
import { useSkillsStore } from './stores/skills'

/** Lazy: large SFC + many icons; keeps dev / first-paint transform graph small. */
const loadSettingsDialog = () => import('./components/settings/SettingsDialog.vue')
const SettingsDialog = defineAsyncComponent(loadSettingsDialog)
const SkillPicker = defineAsyncComponent(() => import('./components/skills/SkillPicker.vue'))

const chat = useChatStore()
const platformAuth = usePlatformAuthStore()
const settings = useSettingsStore()
const skills = useSkillsStore()

const showSettings = ref(false)
const showSkills = ref(false)
const showPlatformLogin = ref(false)

onMounted(() => {
  void loadSettingsDialog()
  void Promise.all([platformAuth.load(), settings.load(), skills.load(), chat.init()]).then(() => {
    if (chat.currentId) void chat.refreshTaskBoard(chat.currentId)
  })
    .catch(e => console.error('[app boot]', e))
    .finally(() => {
      if (!platformAuth.session.logged_in) {
        showPlatformLogin.value = true
      } else if (!settings.settings.hasKey) {
        showSettings.value = true
      }
    })
})

async function onPlatformLogin() {
  try {
    await platformAuth.login()
    showPlatformLogin.value = false
    if (!settings.settings.hasKey) showSettings.value = true
  } catch {
    /* error in store */
  }
}

function onPlatformLoginCancel() {
  void platformAuth.cancelLogin()
}

function onPlatformLoginRequest() {
  showSettings.value = false
  showPlatformLogin.value = true
}

async function onPlatformLogout() {
  showSettings.value = false
  showPlatformLogin.value = true
}
</script>

<template>
  <AppShell
    @open-settings="showSettings = true"
    @open-skills="showSkills = true"
  >
    <ChatView />
  </AppShell>

  <PlatformLoginModal
    v-if="showPlatformLogin"
    :loading="platformAuth.loading"
    :error="platformAuth.error"
    @login="onPlatformLogin"
    @cancel="onPlatformLoginCancel"
  />

  <SettingsDialog
    v-if="showSettings"
    @close="showSettings = false"
    @platform-logout="onPlatformLogout"
    @platform-login="onPlatformLoginRequest"
  />
  <SkillPicker v-if="showSkills" @close="showSkills = false" />
</template>
