import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api, type Settings, type SettingsPatch, type SystemInfo } from '@/api/tauri'
import { i18n } from '@/i18n'

export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<Settings | null>(null)
  const system = ref<SystemInfo | null>(null)

  function applyLanguage(s: Settings) {
    i18n.global.locale.value = s.language
    document.documentElement.lang = s.language
  }

  async function load() {
    const [s, sys] = await Promise.all([api.settingsGet(), api.systemInfo()])
    settings.value = s
    system.value = sys
    applyLanguage(s)
  }

  async function update(patch: SettingsPatch) {
    const s = await api.settingsSet(patch)
    settings.value = s
    applyLanguage(s)
    return s
  }

  return { settings, system, load, update }
})
