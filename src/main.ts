import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import { router } from './router'
import { i18n } from './i18n'
import { isAppError, useErrorsStore } from './stores/errors'
import './style.css'

const app = createApp(App)
const pinia = createPinia()
app.use(pinia).use(router).use(i18n)

const errors = useErrorsStore(pinia)
function unhandled(err: unknown) {
  console.error(err)
  if (isAppError(err)) errors.report(err)
}
app.config.errorHandler = unhandled
window.addEventListener('unhandledrejection', (e) => unhandled(e.reason))

app.mount('#app')
