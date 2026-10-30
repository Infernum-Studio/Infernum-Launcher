import { describe, expect, it } from 'vitest'
import { phaseForStep } from './stores/launch'
import { isValidNickname } from './stores/accounts'
import { errorKey, SELF_EXPLANATORY, showsDetails } from './i18n'
import es from './i18n/es.json'
import { toAppError } from './api/tauri'

describe('phaseForStep', () => {
  it('maps backend steps to button phases', () => {
    expect(phaseForStep('checking')).toBe('checking')
    expect(phaseForStep('java')).toBe('installing')
    expect(phaseForStep('mods')).toBe('installing')
    expect(phaseForStep('launching')).toBe('launching')
  })
})

describe('isValidNickname', () => {
  it('matches the backend rule', () => {
    expect(isValidNickname('Abc')).toBe(true)
    expect(isValidNickname('Player_123456789')).toBe(true)
    expect(isValidNickname('ab')).toBe(false)
    expect(isValidNickname('bad-name')).toBe(false)
    expect(isValidNickname('a'.repeat(17))).toBe(false)
  })
})

describe('errors', () => {
  it('maps known codes and falls back to generic', () => {
    expect(errorKey('network')).toBe('errors.network')
    expect(errorKey('something_new')).toBe('errors.generic')
  })

  it('normalizes thrown values', () => {
    expect(toAppError({ code: 'io', message: 'x' })).toEqual({ code: 'io', message: 'x' })
    expect(toAppError('boom')).toEqual({ code: 'internal', message: 'boom' })
  })
})

describe('showsDetails', () => {
  it('hides the code only for errors that explain themselves', () => {
    expect(showsDetails('not_whitelisted')).toBe(false)
    expect(showsDetails('login_required')).toBe(false)
    expect(showsDetails('network')).toBe(true)
    expect(showsDetails('io')).toBe(true)
    expect(showsDetails('something_new')).toBe(true)
  })

  it('every hidden code has its own message', () => {
    for (const code of SELF_EXPLANATORY) expect(es.errors).toHaveProperty(code)
  })
})
