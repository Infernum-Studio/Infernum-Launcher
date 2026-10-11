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
const { useSkinStore } = await import('./skin')

beforeEach(() => {
  setActivePinia(createPinia())
  vi.clearAllMocks()
  mocks.api.gameStatus.mockResolvedValue({ runningEvent: null })
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
