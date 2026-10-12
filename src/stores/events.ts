import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { api, toAppError, type AppError, type InfernumEvent } from '@/api/tauri'

export const useEventsStore = defineStore('events', () => {
  const events = ref<InfernumEvent[]>([])
  const offline = ref(false)
  const loading = ref(false)
  const error = ref<AppError | null>(null)

  async function load() {
    loading.value = true
    error.value = null
    try {
      const cached = await api.eventsList(false)
      if (cached.events.length) events.value = cached.events
      const fresh = await api.eventsList(true)
      events.value = fresh.events
      offline.value = fresh.offline
    } catch (err) {
      error.value = toAppError(err)
    } finally {
      loading.value = false
    }
  }

  const byId = (id: string) => events.value.find((e) => e.id === id)

  const storedId = (() => {
    try {
      return localStorage.getItem('selectedEvent')
    } catch {
      return null
    }
  })()
  const selectedId = ref<string | null>(storedId)
  const selected = computed(
    () => byId(selectedId.value ?? '') ?? events.value.find((e) => e.status !== 'ended') ?? events.value[0] ?? null,
  )
  function select(id: string) {
    selectedId.value = id
    try {
      localStorage.setItem('selectedEvent', id)
    } catch {
      // si localStorage no esta disponible, la selección no se guarda
    }
  }

  return { events, offline, loading, error, load, byId, selected, select }
})
