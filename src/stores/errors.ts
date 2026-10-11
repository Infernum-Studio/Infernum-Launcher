import { defineStore } from 'pinia'
import { ref } from 'vue'
import { toAppError, type AppError } from '@/api/tauri'

export function isAppError(err: unknown): err is AppError {
  return (
    !!err &&
    typeof err === 'object' &&
    typeof (err as AppError).code === 'string' &&
    typeof (err as AppError).message === 'string'
  )
}

export const useErrorsStore = defineStore('errors', () => {
  const current = ref<AppError | null>(null)

  function report(err: unknown) {
    const error = toAppError(err)
    if (error.code === 'cancelled') return
    current.value = error
  }

  return { current, report, clear: () => (current.value = null) }
})
