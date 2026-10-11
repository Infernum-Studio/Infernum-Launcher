import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export interface AppError {
  code: string
  message: string
  details?: unknown
}

export function toAppError(err: unknown): AppError {
  if (err && typeof err === 'object' && 'code' in err && 'message' in err) {
    return err as AppError
  }
  return { code: 'internal', message: String(err) }
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args)
  } catch (err) {
    throw toAppError(err)
  }
}

export type Language = 'es'

export interface Account {
  id: string
  name: string
  kind: 'offline' | 'microsoft'
}

export interface Settings {
  schemaVersion: number
  onboardingDone: boolean
  language: Language
  ramMb: number
  jvmArgs: string
  discordRpc: boolean
  closeOnLaunch: boolean
  logReports: boolean
  activeAccount: string | null
  accounts: Account[]
}

export type SettingsPatch = Partial<
  Pick<
    Settings,
    'onboardingDone' | 'language' | 'ramMb' | 'jvmArgs' | 'discordRpc' | 'closeOnLaunch' | 'logReports'
  >
>

export interface SystemInfo {
  totalRamMb: number
  maxRamMb: number
  minRamMb: number
  os: string
  version: string
  msaEnabled: boolean
}

export type Localized = Partial<Record<Language, string>>
export type EventStatus = 'testing' | 'live' | 'ended'

export interface InfernumEvent {
  id: string
  name: Localized
  status: EventStatus
  packVersion: string
  ram?: { minMb: number; recommendedMb: number } | null
  startsAt?: string | null
  newsUrl?: string | null
  art: { background?: string | null; logo?: string | null; accent?: string | null }
}

export function eventName(e: InfernumEvent, lang: string): string | null {
  return e.name[lang as Language] ?? e.name.es ?? Object.values(e.name)[0] ?? null
}

export interface EventsList {
  events: InfernumEvent[]
  offline: boolean
}

export interface AccountsList {
  accounts: Account[]
  active: string | null
}

export type FolderKind = 'game' | 'logs' | 'screenshots' | 'quarantine' | 'launcherLogs'

export const api = {
  settingsGet: () => call<Settings>('settings_get'),
  settingsSet: (patch: SettingsPatch) => call<Settings>('settings_set', { patch }),
  systemInfo: () => call<SystemInfo>('system_info'),
  eventsList: (refresh: boolean) => call<EventsList>('events_list', { refresh }),
  accountsList: () => call<AccountsList>('accounts_list'),
  accountsAddOffline: (name: string) => call<AccountsList>('accounts_add_offline', { name }),
  accountsRemove: (id: string) => call<AccountsList>('accounts_remove', { id }),
  accountsSetActive: (id: string) => call<AccountsList>('accounts_set_active', { id }),
  accountsLoginMicrosoft: () => call<AccountsList | null>('accounts_login_microsoft'),
  accountSkin: (id: string) => call<{ dataUrl: string; slim: boolean }>('account_skin', { id }),
  openLink: (link: 'discord' | 'twitter') => call<void>('open_link', { link }),
  play: (eventId: string) => call<void>('play', { eventId }),
  cancelInstall: () => call<void>('cancel_install'),
  killGame: () => call<void>('kill_game'),
  gameStatus: () => call<{ runningEvent: string | null }>('game_status'),
  openFolder: (kind: FolderKind, eventId?: string) => call<void>('open_folder', { kind, eventId }),
}


export type ProgressStep =
  | 'checking'
  | 'java'
  | 'pack'
  | 'minecraft'
  | 'mods'
  | 'integrity'
  | 'launching'

export interface ProgressPayload {
  eventId: string
  step: ProgressStep
  percent: number | null
  message: string | null
}

export interface QuarantinedPayload {
  eventId: string
  folder: string
  files: { name: string; size: number }[]
}

export const listenTo = {
  progress: (cb: (p: ProgressPayload) => void): Promise<UnlistenFn> =>
    listen<ProgressPayload>('launch://progress', (e) => cb(e.payload)),
  started: (cb: (p: { eventId: string; processId: string }) => void) =>
    listen<{ eventId: string; processId: string }>('launch://started', (e) => cb(e.payload)),
  exited: (cb: (p: { eventId: string; code: number | null; crashed: boolean }) => void) =>
    listen<{ eventId: string; code: number | null; crashed: boolean }>('launch://exited', (e) =>
      cb(e.payload),
    ),
  crashed: (cb: (p: { eventId: string; logPath: string }) => void) =>
    listen<{ eventId: string; logPath: string }>('launch://crashed', (e) => cb(e.payload)),
  quarantined: (cb: (p: QuarantinedPayload) => void) =>
    listen<QuarantinedPayload>('integrity://quarantined', (e) => cb(e.payload)),
}
