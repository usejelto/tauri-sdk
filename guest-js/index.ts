import { invoke } from '@tauri-apps/api/core'

export type Props = Record<string, string | number | boolean>

async function call(command: string, args?: Record<string, unknown>): Promise<void> {
  try { await invoke(`plugin:jelto|${command}`, args) } catch { /* Telemetry never rejects into the application. */ }
}

/** Register the Rust plugin and grant local capabilities before initializing after consent. */
async function init(key: string, app?: string, endpoint?: string): Promise<void> {
  await call('init', { key, app, endpoint })
}
async function track(name: string, props?: Props): Promise<void> { await call('track', { name, props }) }
async function onboarding(step: string, status: 'ok' | 'fail' | 'skip', reason?: string): Promise<void> {
  await call('onboarding', { step, status, reason })
}
async function setProps(props: Props): Promise<void> { await call('set_props', { props }) }
async function installId(): Promise<string> {
  try {
    const value: unknown = await invoke('plugin:jelto|install_id')
    return typeof value === 'string' ? value : ''
  } catch { return '' }
}
async function reset(): Promise<void> { await call('reset') }
async function disable(): Promise<void> { await call('disable') }

export default { init, track, onboarding, setProps, installId, reset, disable }
