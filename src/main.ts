import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import './styles/globals.css'
import { applyThemeBootstrap } from './lib/theme'
import { isTauriRuntime } from './lib/runtime'
import { installExternalLinkClickHandler } from './lib/externalLinkClick'
import { initRenderPerf } from './lib/renderPerf'
import { i18n } from './i18n'

applyThemeBootstrap()
installExternalLinkClickHandler()
// Registers the render-perf HUD toggle (keyboard + console). HUD stays off by default.
initRenderPerf()

if (isTauriRuntime()) {
  document.documentElement.classList.add('tauri-app')
}

const app = createApp(App)
app.use(createPinia())
app.use(i18n)

app.config.errorHandler = (err, _vm, info) => {
  console.error('[Vue Error]', info, err)
}

app.mount('#app')
