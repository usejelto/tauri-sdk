import { afterEach, describe, expect, it, vi } from 'vitest'
import { invoke } from '@tauri-apps/api/core'
import jelto from './index'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
afterEach(() => vi.resetAllMocks())

describe('Tauri IPC bindings', () => {
  it('passes optional install origin to initialization without exposing it as a heartbeat property', async () => {
    await jelto.init('prd_conform001', 'desktop', undefined, 'existing')
    expect(invoke).toHaveBeenLastCalledWith('plugin:jelto|init', {
      key: 'prd_conform001', app: 'desktop', endpoint: undefined, installOrigin: 'existing',
    })
  })
  it('routes all seven operations and scalar props to the shared plugin', async () => {
    vi.mocked(invoke).mockResolvedValue('identity')
    await jelto.init('prd_conform001', 'desktop', 'http://localhost:8080/v1/e')
    expect(invoke).toHaveBeenLastCalledWith('plugin:jelto|init', { key: 'prd_conform001', app: 'desktop', endpoint: 'http://localhost:8080/v1/e' })
    const props = { format: 'pdf', count: 2, offline: true }
    await jelto.track('export', props)
    expect(invoke).toHaveBeenLastCalledWith('plugin:jelto|track', { name: 'export', props })
    await jelto.onboarding('permissions', 'fail', 'denied')
    expect(invoke).toHaveBeenLastCalledWith('plugin:jelto|onboarding', { step: 'permissions', status: 'fail', reason: 'denied' })
    await jelto.setProps({ license: 'paid' })
    expect(invoke).toHaveBeenLastCalledWith('plugin:jelto|set_props', { props: { license: 'paid' } })
    expect(await jelto.installId()).toBe('identity')
    await jelto.reset()
    expect(invoke).toHaveBeenLastCalledWith('plugin:jelto|reset', undefined)
    await jelto.disable()
    expect(invoke).toHaveBeenLastCalledWith('plugin:jelto|disable', undefined)
    expect(Object.keys(jelto)).toHaveLength(7)
  })
  it.each(['denied capability', 'missing plugin', 'closed window'])('swallows IPC rejection: %s', async reason => {
    vi.mocked(invoke).mockRejectedValue(new Error(reason))
    await expect(Promise.all([jelto.init('key'), jelto.track('x'), jelto.onboarding('tour', 'ok'), jelto.setProps({}), jelto.reset(), jelto.disable()])).resolves.toEqual(Array(6).fill(undefined))
    expect(await jelto.installId()).toBe('')
  })
  it('swallows synchronous serialization errors and invalid identity replies', async () => {
    vi.mocked(invoke).mockImplementation(() => { throw new TypeError('serialization failed') })
    await expect(jelto.track('x')).resolves.toBeUndefined()
    expect(await jelto.installId()).toBe('')
    vi.mocked(invoke).mockResolvedValue(null)
    expect(await jelto.installId()).toBe('')
  })
})
