import { defineStore } from 'pinia'
import { ref, watch } from 'vue'
import { api } from '@/api/tauri'
import { useAccountsStore } from './accounts'

export interface SkinData {
  dataUrl: string
  slim: boolean
}

const SKIN_TTL_MS = 5 * 60 * 1000

export const useSkinStore = defineStore('skin', () => {
  const accounts = useAccountsStore()
  const skin = ref<SkinData | null>(null)
  let requested: string | null = null

  const cache = new Map<string, { at: number; data: Promise<SkinData> }>()

  function fetchSkin(id: string): Promise<SkinData> {
    const hit = cache.get(id)
    if (hit && Date.now() - hit.at < SKIN_TTL_MS) return hit.data
    const data = api.accountSkin(id)
    const entry = { at: Date.now(), data }
    cache.set(id, entry)
    data.catch(() => {
      if (cache.get(id) === entry) cache.delete(id)
    })
    return data
  }

  async function load(id: string | null) {
    requested = id
    if (!id) {
      skin.value = null
      return
    }
    try {
      const data = await fetchSkin(id)
      if (requested === id) skin.value = data
    } catch {
      if (requested === id) skin.value = null
    }
  }

  watch(() => accounts.activeId, load, { immediate: true })

  return {
    skin,
    fetchSkin,
    reload: () => {
      if (accounts.activeId) cache.delete(accounts.activeId)
      return load(accounts.activeId)
    },
  }
})
