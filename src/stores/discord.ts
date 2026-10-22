import { defineStore } from 'pinia'
import { ref } from 'vue'
import { api, type DiscordLoginOutcome, type DiscordSession } from '@/api/tauri'

export const useDiscordStore = defineStore('discord', () => {
  const session = ref<DiscordSession | null>(null)
  const waiting = ref(false)
  const loaded = ref(false)
  let attempt = 0

  async function login(): Promise<DiscordLoginOutcome> {
    const mine = ++attempt
    waiting.value = true
    try {
      const outcome = await api.discordLogin()
      if (outcome.status === 'loggedIn') session.value = outcome.session
      return outcome
    } finally {
      if (mine === attempt) waiting.value = false
    }
  }

  return {
    session,
    waiting,
    loaded,
    login,
    load: async () => {
      try {
        session.value = await api.discordSession()
      } finally {
        loaded.value = true
      }
    },
    cancel: () => api.discordCancelLogin(),
    logout: async () => {
      await api.discordLogout()
      session.value = null
    },
  }
})
