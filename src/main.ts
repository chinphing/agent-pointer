import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import './styles/globals.css'
import { applyThemeBootstrap } from './lib/theme'

applyThemeBootstrap()

const app = createApp(App)
app.use(createPinia())

app.config.errorHandler = (err, _vm, info) => {
  console.error('[Vue Error]', info, err)
}

app.mount('#app')
