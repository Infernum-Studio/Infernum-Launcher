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

export const SELF_EXPLANATORY: ReadonlySet<string> = new Set([
  'login_required',
  'not_whitelisted',
  'access_denied',
  'event_ended',
  'event_not_found',
  'no_account',
  'already_running',
  'launcher_outdated',
  'invalid_nickname',
  'jvm_args_memory',
  'jvm_args_unparsable',
  'jvm_args_blocked',
  'msa_disabled',
  'msa_login_required',
  'account_not_found',
  'discord_port_busy',
])

export function showsDetails(code: string): boolean {
  return !SELF_EXPLANATORY.has(code)
}

export function errorText(error: { code: string; details?: unknown }): string {
  const details = (error.details ?? {}) as Record<string, unknown>
  if (error.code === 'minecraft_install_failed' && typeof details.loader === 'string') {
    return i18n.global.t('errors.minecraft_loader_install_failed', { loader: details.loader })
  }
  return i18n.global.t(errorKey(error.code))
}
