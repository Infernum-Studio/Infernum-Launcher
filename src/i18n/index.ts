import { createI18n } from 'vue-i18n'
import es from './es.json'

export const i18n = createI18n({
  legacy: false,
  locale: 'es',
  fallbackLocale: 'es',
  messages: { es },
})

export function errorKey(code: string): string {
  return code in es.errors ? `errors.${code}` : 'errors.generic'
}

export function errorText(error: { code: string; details?: unknown }): string {
  const details = (error.details ?? {}) as Record<string, unknown>
  if (error.code === 'minecraft_install_failed' && typeof details.loader === 'string') {
    return i18n.global.t('errors.minecraft_loader_install_failed', { loader: details.loader })
  }
  return i18n.global.t(errorKey(error.code))
}
