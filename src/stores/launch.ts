import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  api,
  listenTo,
  toAppError,
  type AppError,
  type ProgressStep,
  type QuarantinedPayload,
} from '@/api/tauri'
import { useDiscordStore } from './discord'

export type PlayPhase = 'idle' | 'checking' | 'installing' | 'launching' | 'running'

export function phaseForStep(step: ProgressStep): PlayPhase {
  switch (step) {
    case 'checking':
      return 'checking'
    case 'launching':
      return 'launching'
    default:
      return 'installing'
  }
}

export const useLaunchStore = defineStore('launch', () => {
  const eventId = ref<string | null>(null)
  const phase = ref<PlayPhase>('idle')
  const step = ref<ProgressStep | null>(null)
  const percent = ref<number | null>(null)
  const detail = ref<string | null>(null)
  const error = ref<AppError | null>(null)
  const quarantined = ref<QuarantinedPayload | null>(null)
  const crashedLog = ref<{ eventId: string; logPath: string } | null>(null)
  const needsDiscord = ref<{ eventId: string; expired: boolean } | null>(null)
  let listening = false

  async function listenAll() {
    if (listening) return
    listening = true
    await listenTo.progress((p) => {
      if (phase.value === 'running') return
      eventId.value = p.eventId
      step.value = p.step
      percent.value = p.percent
      detail.value = p.message
      phase.value = phaseForStep(p.step)
    })
    await listenTo.started((p) => {
      eventId.value = p.eventId
      phase.value = 'running'
      step.value = null
      percent.value = null
    })
    await listenTo.exited(() => {
      phase.value = 'idle'
      eventId.value = null
    })
    await listenTo.crashed((p) => (crashedLog.value = p))
    await listenTo.quarantined((p) => (quarantined.value = p))

    const status = await api.gameStatus()
    if (status.runningEvent) {
      eventId.value = status.runningEvent
      phase.value = 'running'
    }
  }

  async function play(id: string) {
    error.value = null
    eventId.value = id
    phase.value = 'checking'
    try {
      await api.play(id)
    } catch (err) {
      const e = toAppError(err)
      if (e.code === 'login_required') {
        const discord = useDiscordStore()
        needsDiscord.value = { eventId: id, expired: !!discord.session }
        discord.session = null
      }
      error.value = e.code === 'cancelled' || e.code === 'login_required' ? null : e
      phase.value = 'idle'
      step.value = null
      percent.value = null
    }
  }

  async function stop() {
    if (phase.value === 'running') await api.killGame()
    else await api.cancelInstall()
  }

  return {
    eventId,
    phase,
    step,
    percent,
    detail,
    error,
    quarantined,
    crashedLog,
    needsDiscord,
    listenAll,
    play,
    stop,
  }
})
