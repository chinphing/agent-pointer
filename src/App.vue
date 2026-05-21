<script setup lang="ts">
import { defineAsyncComponent, onMounted, ref } from 'vue'
import AppShell from './components/layout/AppShell.vue'
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
</script>

<template>
  <AppShell
    @open-settings="showSettings = true"
    @open-skills="showSkills = true"
  >
    <ChatView />
  </AppShell>

  <div
    v-if="showPlatformLogin"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
  >
    <div class="max-w-md rounded-2xl border border-black/10 bg-white p-6 shadow-lg">
      <h2 class="text-lg font-semibold">登录 Openpointer</h2>
      <p class="mt-2 text-sm text-black/60">
        使用官网账户登录后，方可使用截图标注（SOM）与平台模型用量统计。
      </p>
      <p v-if="platformAuth.error" class="mt-3 rounded-lg bg-red-50 p-2 text-sm text-red-800">
        {{ platformAuth.error }}
      </p>
      <button
        type="button"
        class="mt-4 w-full rounded-full bg-ink px-4 py-2.5 text-sm font-medium text-white disabled:opacity-50"
        :disabled="platformAuth.loading"
        @click="onPlatformLogin"
      >
        {{ platformAuth.loading ? '正在打开浏览器…' : '在浏览器中登录' }}
      </button>
    </div>
  </div>

  <SettingsDialog v-if="showSettings" @close="showSettings = false" />
  <SkillPicker v-if="showSkills" @close="showSkills = false" />
</template>
