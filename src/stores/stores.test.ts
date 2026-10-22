import { beforeEach, describe, expect, it, vi } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

type Handler = (payload: unknown) => void

const mocks = vi.hoisted(() => {
  const handlers: Record<string, Handler> = {}
  const listen = (name: string) => async (cb: Handler) => {
    handlers[name] = cb
    return () => {}
  }
  return {
    handlers,
    api: {
      play: vi.fn(),
      killGame: vi.fn(),
      cancelInstall: vi.fn(),
      gameStatus: vi.fn(),
      accountSkin: vi.fn(),
      eventsList: vi.fn(),
      discordLogin: vi.fn(),
      discordLogout: vi.fn(),
      discordSession: vi.fn(),
      discordCancelLogin: vi.fn(),
    },
    listenTo: {
      progress: listen('progress'),
      started: listen('started'),
      exited: listen('exited'),
      crashed: listen('crashed'),
      quarantined: listen('quarantined'),
    },
  }
})

vi.mock('@/api/tauri', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/api/tauri')>()),
  api: mocks.api,
  listenTo: mocks.listenTo,
}))

const { useLaunchStore } = await import('./launch')
const { useErrorsStore } = await import('./errors')
const { useEventsStore } = await import('./events')
const { useSkinStore } = await import('./skin')
const { useDiscordStore } = await import('./discord')

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  mocks.api.gameStatus.mockResolvedValue({ runningEvent: null })
})

const event = (id: string, status: string) => ({ id, status, name: {}, art: {}, discord: {} })

describe('events store', () => {
  async function loaded(list: ReturnType<typeof event>[]) {
    mocks.api.eventsList.mockResolvedValue({ events: list, offline: false })
    const events = useEventsStore()
    await events.load()
    return events
  }

  it('opens the live event first, then testing, then any other', async () => {
    let events = await loaded([event('a', 'ended'), event('b', 'testing'), event('c', 'live')])
    expect(events.selected?.id).toBe('c')
    setActivePinia(createPinia())
    events = await loaded([event('a', 'ended'), event('b', 'testing')])
    expect(events.selected?.id).toBe('b')
    setActivePinia(createPinia())
    events = await loaded([event('a', 'ended')])
    expect(events.selected?.id).toBe('a')
    setActivePinia(createPinia())
    events = await loaded([])
    expect(events.selected).toBeNull()
  })

  it('keeps the chosen event until the launcher restarts', async () => {
    let events = await loaded([event('b', 'testing'), event('c', 'live')])
    events.select('b')
    expect(events.selected?.id).toBe('b')
    events.select('gone')
    expect(events.selected?.id).toBe('c')
    setActivePinia(createPinia())
    events = await loaded([event('b', 'testing'), event('c', 'live')])
    expect(events.selected?.id).toBe('c')
  })
})

describe('launch store', () => {
  it('follows backend events through a launch', async () => {
    const launch = useLaunchStore()
    await launch.listenAll()
    mocks.handlers.progress!({ eventId: 'e', step: 'mods', percent: 40, message: null })
    expect(launch.phase).toBe('installing')
    expect(launch.percent).toBe(40)
    mocks.handlers.started!({ eventId: 'e', processId: 'p' })
    expect(launch.phase).toBe('running')
    mocks.handlers.progress!({ eventId: 'e', step: 'launching', percent: null, message: null })
    expect(launch.phase).toBe('running')
    mocks.handlers.exited!({ eventId: 'e', code: 0, crashed: false })
    expect(launch.phase).toBe('idle')
  })

  it('picks up a game that is already running', async () => {
    mocks.api.gameStatus.mockResolvedValue({ runningEvent: 'e' })
    const launch = useLaunchStore()
    await launch.listenAll()
    expect(launch.phase).toBe('running')
    expect(launch.eventId).toBe('e')
  })

  it('shows failures but not the user cancelling', async () => {
    const launch = useLaunchStore()
    mocks.api.play.mockRejectedValueOnce({ code: 'cancelled', message: 'x' })
    await launch.play('e')
    expect(launch.error).toBeNull()
    expect(launch.phase).toBe('idle')

    mocks.api.play.mockRejectedValueOnce({ code: 'network', message: 'x' })
    await launch.play('e')
    expect(launch.error?.code).toBe('network')
    expect(launch.phase).toBe('idle')
  })

  it('asks for a Discord login instead of showing an error', async () => {
    const launch = useLaunchStore()
    mocks.api.play.mockRejectedValueOnce({ code: 'login_required', message: 'x' })
    await launch.play('hc')
    expect(launch.error).toBeNull()
    expect(launch.needsDiscord).toEqual({ eventId: 'hc', expired: false })
    expect(launch.phase).toBe('idle')
  })

  it('a session the API rejects stops showing as logged in', async () => {
    const discord = useDiscordStore()
    discord.session = { user: { id: '1', name: 'tester' }, expiresAt: '2026-11-01T00:00:00Z' }
    const launch = useLaunchStore()
    mocks.api.play.mockRejectedValueOnce({ code: 'login_required', message: 'x' })
    await launch.play('hc')
    expect(launch.needsDiscord).toEqual({ eventId: 'hc', expired: true })
    expect(discord.session).toBeNull()
  })

  it('stop kills a running game and cancels an install', async () => {
    const launch = useLaunchStore()
    launch.phase = 'installing'
    await launch.stop()
    expect(mocks.api.cancelInstall).toHaveBeenCalledOnce()
    launch.phase = 'running'
    await launch.stop()
    expect(mocks.api.killGame).toHaveBeenCalledOnce()
  })
})

describe('errors store', () => {
  it('keeps the latest error and ignores cancels', () => {
    const errors = useErrorsStore()
    errors.report({ code: 'cancelled', message: 'x' })
    expect(errors.current).toBeNull()
    errors.report({ code: 'io', message: 'disk' })
    expect(errors.current?.code).toBe('io')
    errors.report('boom')
    expect(errors.current).toEqual({ code: 'internal', message: 'boom' })
    errors.clear()
    expect(errors.current).toBeNull()
  })
})

describe('skin store', () => {
  it('fetches each skin once and retries failures', async () => {
    const skins = useSkinStore()
    mocks.api.accountSkin.mockResolvedValue({ dataUrl: 'data:a', slim: false })
    await Promise.all([skins.fetchSkin('a'), skins.fetchSkin('a')])
    await skins.fetchSkin('a')
    expect(mocks.api.accountSkin).toHaveBeenCalledTimes(1)

    mocks.api.accountSkin.mockRejectedValueOnce({ code: 'network', message: 'x' })
    await expect(skins.fetchSkin('b')).rejects.toMatchObject({ code: 'network' })
    await expect(skins.fetchSkin('b')).resolves.toMatchObject({ dataUrl: 'data:a' })
    expect(mocks.api.accountSkin).toHaveBeenCalledTimes(3)
  })
})

describe('discord store', () => {
  const session = { user: { id: '1', name: 'tester' }, expiresAt: '2026-11-01T00:00:00Z' }

  it('keeps the session after logging in and drops it on logout', async () => {
    const discord = useDiscordStore()
    mocks.api.discordLogin.mockResolvedValueOnce({ status: 'loggedIn', session })
    expect((await discord.login()).status).toBe('loggedIn')
    expect(discord.session?.user.name).toBe('tester')
    expect(discord.waiting).toBe(false)

    await discord.logout()
    expect(discord.session).toBeNull()
  })

  it('loads the saved session, or none', async () => {
    const discord = useDiscordStore()
    expect(discord.loaded).toBe(false)
    mocks.api.discordSession.mockResolvedValueOnce(session)
    await discord.load()
    expect(discord.loaded).toBe(true)
    expect(discord.session?.user.id).toBe('1')
    mocks.api.discordSession.mockResolvedValueOnce(null)
    await discord.load()
    expect(discord.session).toBeNull()
  })

  it('counts as loaded even if reading the session fails', async () => {
    const discord = useDiscordStore()
    mocks.api.discordSession.mockRejectedValueOnce({ code: 'io', message: 'x' })
    await expect(discord.load()).rejects.toMatchObject({ code: 'io' })
    expect(discord.loaded).toBe(true)
    expect(discord.session).toBeNull()
  })

  it('denied, timed out and cancelled logins leave no session', async () => {
    const discord = useDiscordStore()
    for (const status of ['denied', 'timedOut', 'cancelled'] as const) {
      mocks.api.discordLogin.mockResolvedValueOnce({ status })
      expect((await discord.login()).status).toBe(status)
      expect(discord.session).toBeNull()
      expect(discord.waiting).toBe(false)
    }
  })

  it('a failed login stops waiting and reports the error', async () => {
    const discord = useDiscordStore()
    mocks.api.discordLogin.mockRejectedValueOnce({ code: 'api_unavailable', message: 'x' })
    await expect(discord.login()).rejects.toMatchObject({ code: 'api_unavailable' })
    expect(discord.waiting).toBe(false)
    expect(discord.session).toBeNull()
  })

  it('cancel asks the backend to stop the login', async () => {
    const discord = useDiscordStore()
    mocks.api.discordCancelLogin.mockResolvedValueOnce(undefined)
    await discord.cancel()
    expect(mocks.api.discordCancelLogin).toHaveBeenCalledOnce()
  })

  it('keeps the session if logging out fails', async () => {
    const discord = useDiscordStore()
    discord.session = session
    mocks.api.discordLogout.mockRejectedValueOnce({ code: 'io', message: 'x' })
    await expect(discord.logout()).rejects.toMatchObject({ code: 'io' })
    expect(discord.session).toEqual(session)
  })

  it('a replaced login does not stop the new one from waiting', async () => {
    const discord = useDiscordStore()
    let finishFirst!: (v: unknown) => void
    mocks.api.discordLogin
      .mockReturnValueOnce(new Promise((r) => (finishFirst = r)))
      .mockReturnValueOnce(new Promise(() => {}))
    const first = discord.login()
    void discord.login()
    finishFirst({ status: 'replaced' })
    await first
    expect(discord.waiting).toBe(true)
    expect(discord.session).toBeNull()
  })
})
