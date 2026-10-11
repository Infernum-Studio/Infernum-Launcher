import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { api, type Account, type AccountsList } from '@/api/tauri'

export const useAccountsStore = defineStore('accounts', () => {
  const accounts = ref<Account[]>([])
  const activeId = ref<string | null>(null)
  const active = computed(() => accounts.value.find((a) => a.id === activeId.value) ?? null)

  function apply(list: AccountsList) {
    accounts.value = list.accounts
    activeId.value = list.active
  }

  return {
    accounts,
    activeId,
    active,
    load: async () => apply(await api.accountsList()),
    addOffline: async (name: string) => apply(await api.accountsAddOffline(name)),
    remove: async (id: string) => apply(await api.accountsRemove(id)),
    setActive: async (id: string) => apply(await api.accountsSetActive(id)),
    loginMicrosoft: async () => {
      const list = await api.accountsLoginMicrosoft()
      if (list) apply(list)
      return list !== null
    },
  }
})

export function isValidNickname(name: string): boolean {
  return /^[A-Za-z0-9_]{3,16}$/.test(name)
}
