import jelto from '@jelto/tauri'
import './style.css'

const field = (id: string) => document.getElementById(id) as HTMLInputElement
const status = (message: string) => { document.getElementById('status')!.textContent = message }
const showIdentity = async () => { document.getElementById('identity')!.textContent = await jelto.installId() || 'None' }
const enabled = (value: boolean) => {
  document.querySelectorAll<HTMLButtonElement>('.actions button').forEach(button => { button.disabled = !value })
  field('enable').disabled = value
}
document.getElementById('enable')!.onclick = async () => {
  if (!field('consent').checked) { status('Choose whether to allow analytics first.'); return }
  await jelto.init(field('key').value, field('app').value, field('endpoint').value)
  const identity = await jelto.installId()
  enabled(identity !== '')
  await showIdentity()
  status(identity ? 'Analytics enabled. A heartbeat is queued.' : 'Initialization unavailable. Check the key, plugin and capability.')
}
document.getElementById('track')!.onclick = async () => { await jelto.track('export', { format: 'pdf' }); status('Export event queued.') }
document.getElementById('onboarding')!.onclick = async () => { await jelto.onboarding('tour', 'ok'); status('Onboarding completion queued.') }
document.getElementById('paid')!.onclick = async () => { await jelto.setProps({ license: 'paid' }); status('Paid license set.') }
document.getElementById('reset')!.onclick = async () => { await jelto.reset(); await showIdentity(); status('Identity reset.') }
document.getElementById('disable')!.onclick = async () => {
  await jelto.disable(); await showIdentity(); enabled(false); field('consent').checked = false
  status('Analytics is off. Local state erased.')
}
